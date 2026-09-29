"""Main-thread catchable process cancellation with restoration of prior handlers."""

from __future__ import annotations

import os
import signal
import threading
import time
from contextlib import contextmanager
from dataclasses import dataclass, field
from typing import TYPE_CHECKING, Final

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
    try:
        for watched_signal in watched:
            signal.signal(watched_signal, handler)
        yield
    finally:
        for watched_signal, handler_before in previous.items():
            signal.signal(watched_signal, handler_before)


@contextmanager
def install_cancellation_handlers() -> Iterator[None]:
    """Install and restore main-thread cancellation handlers for batch execution.

    Yields:
        Control while SIGINT and available POSIX termination signals raise receipts.

    """
    with _handling(_raise_cancellation):
        yield


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

    def check(self) -> None:
        """End the process when cancellation can no longer wait for delivery."""
        stalled = self.clock() - self.progress >= GRACE_SECONDS
        if len(self.signals) >= REPEAT_SIGNALS or (self.signals and stalled):
            hard_exit(INTERRUPTED_STATUS)


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
    with _handling(guard.record):
        yield guard


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
