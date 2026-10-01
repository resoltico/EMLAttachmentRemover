"""Cooperative process cancellation with restoration of prior signal handlers."""

from __future__ import annotations

import os
import signal
import threading
import time
from contextlib import contextmanager
from dataclasses import dataclass, field
from functools import partial
from typing import TYPE_CHECKING, Final

from .cancellation_owner import CancellationOwner, cleanup_actions
from .cancellation_state import CURRENT, CancellationState

if TYPE_CHECKING:
    from collections.abc import Callable, Iterator

GRACE_SECONDS: Final = 10.0
REPEAT_SIGNALS: Final = 2
INTERRUPTED_STATUS: Final = 130


@dataclass
class CancellationSignal(BaseException):
    """A catchable process-control signal with its stable display name."""

    number: int
    name: str

    def __init__(self, number: int, name: str) -> None:
        """Initialize BaseException context as well as the typed signal receipt.

        Args:
            number: Native signal number delivered by the runtime.
            name: Stable signal name recorded in the ledger.

        """
        super().__init__(number, name)
        self.number = number
        self.name = name


def cancellation_name(number: int) -> str:
    """Return a stable signal receipt name without depending on enum formatting.

    Returns:
        A recognized POSIX signal name, or a numeric fallback for a platform signal.

    """
    try:
        return signal.Signals(number).name
    except ValueError:
        return f"signal-{number}"


@contextmanager
def _handling(handler: Callable[[int, object], None]) -> Iterator[None]:
    """Route watched main-thread signals to one handler, restoring prior handlers.

    Yields:
        Control while SIGINT and available POSIX termination signals use ``handler``.

    """
    if threading.current_thread() is not threading.main_thread():
        yield
        return
    watched = _watched_signals()
    previous = {
        watched_signal: signal.getsignal(watched_signal) for watched_signal in watched
    }
    primary: BaseException | None = None
    try:
        for watched_signal in watched:
            signal.signal(watched_signal, handler)
        yield
    except BaseException as error:
        primary = error
        raise
    finally:
        cleanup_actions(
            primary,
            (
                partial(signal.signal, number, before)
                for number, before in reversed(previous.items())
            ),
        )


@contextmanager
def install_cancellation_handlers() -> Iterator[None]:
    """Install and restore main-thread cancellation handlers for batch execution.

    Yields:
        Control while signals record requests for acknowledgment at safe checkpoints.

    Raises:
        RuntimeError: If a same-thread enclosing context is no longer active.

    """
    enclosing = CURRENT.get()
    if enclosing is not None and enclosing.owner_thread == threading.get_ident():
        if not enclosing.active:
            message = "inactive cancellation context"
            raise RuntimeError(message)
        yield
        return
    state = CancellationState()
    worker = threading.Thread(
        target=_watch_processing,
        args=(state,),
        daemon=True,
        name="eml-cancellation-monitor",
    )
    owner = CancellationOwner(state, worker, GRACE_SECONDS)
    with _handling(state.record), owner.scope():
        yield
    _acknowledge(state)


POLL_SECONDS: Final = 0.05


def checkpoint(*, before_visibility: bool = False) -> None:
    """Acknowledge pending cancellation only at a coherent application boundary."""
    state = CURRENT.get()
    if state is not None and (not state.critical or before_visibility):
        _acknowledge(state)


def _acknowledge(state: CancellationState) -> None:
    """Observe pending requests even after the owner's context has been restored.

    Raises:
        CancellationSignal: For an unacknowledged interruption request.

    """
    if len(state.signals) > state.acknowledged:
        number = state.signals[state.acknowledged]
        state.acknowledged = len(state.signals)
        raise CancellationSignal(number, cancellation_name(number))


@contextmanager
def coherent_operation() -> Iterator[None]:
    """Keep receipt transfer and item construction inside one cooperative operation.

    Yields:
        Control until the authoritative outcome has been committed.

    """
    checkpoint()
    state = CURRENT.get()
    if state is not None:
        state.critical += 1
    try:
        yield
    finally:
        if state is not None:
            state.critical -= 1


def _watch_processing(state: CancellationState) -> None:
    """Bound unresponsive native/CPU work without interrupting Python assignments."""
    while state.active and not state.stop.wait(POLL_SECONDS):
        if state.signals and not state.delivering:
            expired = time.monotonic() - state.requested_at >= GRACE_SECONDS
            if len(state.signals) >= REPEAT_SIGNALS or expired:
                hard_exit(INTERRUPTED_STATUS)


@dataclass(slots=True)
class DeliveryGuard:
    """Absorb cancellation while a complete report is being written, boundedly.

    Signals are recorded, never raised, so a document is not cut in half by an
    exception. A first signal grants a grace of ``GRACE_SECONDS`` since the last
    output progress; a second signal, or a stall through that grace, ends the
    process at once with the interruption status.
    """

    clock: Callable[[], float] = time.monotonic
    signals: list[int] = field(default_factory=list)
    progress: float = 0.0
    report_units: int = 0
    diagnostic_units: int = 0
    diagnostic_newline: bool = True
    owner: CancellationState | None = field(default=None, repr=False)
    forwarded: int = 0
    retired: bool = False

    def __post_init__(self) -> None:
        """Start the grace clock at creation."""
        self.progress = self.clock()

    def record(self, number: int, _frame: object) -> None:
        """Remember one delivered signal; the first one starts the grace clock."""
        if not self.signals:
            self.progress = self.clock()
        self.signals.append(number)

    def note_progress(self) -> None:
        """Restart the grace clock after output was accepted by its channel."""
        self.progress = self.clock()

    def accepted(self, chunk: bytes | str, *, report: bool) -> None:
        """Record channel acceptance separately from shared cancellation progress."""
        if report:
            self.report_units += len(chunk)
        else:
            self.diagnostic_units += len(chunk)
            self.diagnostic_newline = chunk[-1:] in {"\n", b"\n"}
        self.note_progress()

    def check(self) -> None:
        """End the process when cancellation can no longer wait for delivery."""
        stalled = self.clock() - self.progress >= GRACE_SECONDS
        if len(self.signals) >= REPEAT_SIGNALS or (self.signals and stalled):
            hard_exit(INTERRUPTED_STATUS)

    def result(self, status: int) -> int:
        """Select status from the retired guard and consume only its forwarded requests.

        Returns:
            The processing status or interruption after complete guard retirement.

        Raises:
            RuntimeError: If status is selected while the guard still accepts signals.

        """
        if not self.retired:
            message = "delivery status requested before handler retirement"
            raise RuntimeError(message)
        selected = INTERRUPTED_STATUS if self.signals else status
        if self.owner is not None:
            self.owner.acknowledged = max(self.owner.acknowledged, self.forwarded)
        return selected


def hard_exit(status: int) -> None:
    """End the process immediately, without flushing a channel that is stalled."""
    os._exit(status)


@contextmanager
def delivery_guard(
    clock: Callable[[], float] = time.monotonic,
) -> Iterator[DeliveryGuard]:
    """Hold cancellation while one report is delivered, under a bounded grace.

    Yields:
        The guard whose ``signals`` tell the caller what arrived meanwhile.

    """
    guard = DeliveryGuard(clock)
    state = CURRENT.get()
    guard.owner = state
    if state is not None:
        state.delivering = True
        guard.signals.extend(state.signals)
        guard.forwarded = len(state.signals)

    def record(number: int, frame: object) -> None:
        guard.record(number, frame)
        if state is not None:
            state.record(number, frame)
            guard.forwarded = len(state.signals)

    try:
        with _handling(record):
            yield guard
    finally:
        if state is not None:
            state.delivering = False
        guard.retired = True


def _watched_signals() -> tuple[int, ...]:
    """Return supported catchable process-control signals for this platform.

    Returns:
        SIGINT plus SIGTERM and SIGHUP where the platform exposes them.

    """
    if os.name == "nt":
        return (signal.SIGINT,)
    candidates = [signal.SIGINT, signal.SIGTERM]
    if hasattr(signal, "SIGHUP"):
        candidates.append(signal.SIGHUP)
    return tuple(candidates)


def _raise_cancellation(number: int, _frame: object) -> None:
    """Convert a delivered catchable signal into a ledger-ownable exception.

    Raises:
        CancellationSignal: Always, with the delivered signal's stable name.

    """
    raise CancellationSignal(number, cancellation_name(number))
