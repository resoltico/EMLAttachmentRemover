"""Bound nontermination in tests before the mutation runner timeout."""

from __future__ import annotations

import signal
from contextlib import contextmanager
from typing import TYPE_CHECKING

if TYPE_CHECKING:
    from collections.abc import Iterator


@contextmanager
def finite_operation(timeout: float = 5) -> Iterator[None]:
    """Turn a nonterminating Linux mutation into a bounded test failure.

    Yields:
        Control with a generous deadline for these small finite-operation fixtures.

    """
    alarm = getattr(signal, "SIGALRM", None)
    set_timer = getattr(signal, "setitimer", None)
    timer_kind = getattr(signal, "ITIMER_REAL", None)
    if alarm is None or set_timer is None or timer_kind is None:
        yield
        return
    previous = signal.getsignal(alarm)

    def stalled(_number: int, _frame: object) -> None:
        message = "finite report delivery did not complete"
        raise AssertionError(message)

    signal.signal(alarm, stalled)
    timer = set_timer(timer_kind, timeout)
    try:
        yield
    finally:
        set_timer(timer_kind, *timer)
        signal.signal(alarm, previous)
