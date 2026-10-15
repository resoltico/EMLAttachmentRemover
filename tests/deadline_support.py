"""Bound nontermination in tests before the mutation runner timeout."""

from __future__ import annotations

import signal
import threading
from contextlib import contextmanager
from functools import wraps
from typing import TYPE_CHECKING

if TYPE_CHECKING:
    from collections.abc import Callable, Generator


def finite_main_call[**P, R](
    operation: Callable[P, R], timeout: float = 10
) -> Callable[P, R]:
    """Bound a main-thread operation; its worker stays under the caller's guard.

    Returns:
        A metadata-preserving wrapper that executes on the original caller thread.

    """

    @wraps(operation)
    def bounded(*args: P.args, **kwargs: P.kwargs) -> R:
        if threading.current_thread() is not threading.main_thread():
            return operation(*args, **kwargs)
        with finite_operation(timeout):
            return operation(*args, **kwargs)

    return bounded


@contextmanager
def finite_operation(timeout: float = 5) -> Generator[None]:
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
