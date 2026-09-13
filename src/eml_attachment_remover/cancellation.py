"""Main-thread catchable process cancellation with restoration of prior handlers."""

from __future__ import annotations

import os
import signal
import threading
from contextlib import contextmanager
from dataclasses import dataclass
from typing import TYPE_CHECKING

if TYPE_CHECKING:
    from collections.abc import Iterator


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
def install_cancellation_handlers() -> Iterator[None]:
    """Install and restore main-thread cancellation handlers for batch execution.

    Yields:
        Control while SIGINT and available POSIX termination signals raise receipts.

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
            signal.signal(watched_signal, _raise_cancellation)
        yield
    finally:
        for watched_signal, handler in previous.items():
            signal.signal(watched_signal, handler)


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
