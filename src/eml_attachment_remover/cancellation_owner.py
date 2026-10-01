"""Exception-safe ownership of one context binding and wakeable monitor."""

from __future__ import annotations

from contextlib import contextmanager
from dataclasses import dataclass
from typing import TYPE_CHECKING

from .cancellation_state import CURRENT, CancellationState

if TYPE_CHECKING:
    import threading
    from collections.abc import Callable, Iterable, Iterator
    from contextvars import Token


def cleanup_actions(
    primary: BaseException | None, actions: Iterable[Callable[[], object]]
) -> None:
    """Attempt independent releases, retaining the first failure and later details."""
    failures: list[BaseException] = []
    for action in actions:
        try:
            action()
        except BaseException as error:  # ruff: ignore[blind-except] - release other owners even on interruption.
            failures.append(error)
    if not failures:
        return
    selected = primary if primary is not None else failures.pop(0)
    for failure in failures:
        selected.add_note(f"cancellation cleanup: {type(failure).__name__}: {failure}")
    if primary is None:
        raise selected


@dataclass(slots=True)
class CancellationOwner:
    """Prepare resources before exposing a context, and release every acquisition."""

    state: CancellationState
    worker: threading.Thread
    timeout: float
    token: Token[CancellationState | None] | None = None

    @contextmanager
    def scope(self) -> Iterator[None]:
        """Acquire while cooperative handlers protect setup and all rollback steps.

        Yields:
            Control with a started watchdog and the invocation context bound.

        """
        primary: BaseException | None = None
        try:
            self.state.active = True
            self.worker.start()
            self.token = CURRENT.set(self.state)
            yield
        except BaseException as error:
            primary = error
            raise
        finally:
            self.state.active = False
            cleanup_actions(primary, (self.state.stop.set, self._join, self._reset))

    def _join(self) -> None:
        """Join only a launched monitor, including a start that failed after launch.

        Raises:
            RuntimeError: If a launched watchdog does not acknowledge shutdown.

        """
        if self.worker.ident is None:
            return
        self.worker.join(self.timeout)
        if self.worker.is_alive():
            message = "cancellation watchdog did not stop"
            raise RuntimeError(message)

    def _reset(self) -> None:
        """Restore the prior context independently of monitor shutdown failures."""
        if self.token is not None:
            CURRENT.reset(self.token)
            self.token = None
