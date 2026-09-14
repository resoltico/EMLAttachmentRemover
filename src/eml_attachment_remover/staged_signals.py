"""Narrow signal deferral used only around the publication visibility edge."""

from __future__ import annotations

import signal
import threading
from contextlib import contextmanager
from typing import TYPE_CHECKING

if TYPE_CHECKING:
    from collections.abc import Iterator


@contextmanager
def defer_signals() -> Iterator[None]:
    """Defer catchable process signals across one nonterminal publication edge."""
    mask = getattr(signal, "pthread_" + "sigmask", None)
    block = getattr(signal, "SIG_" + "BLOCK", None)
    restore = getattr(signal, "SIG_" + "SETMASK", None)
    if (
        not callable(mask)
        or not isinstance(block, int)
        or not isinstance(restore, int)
        or threading.current_thread() is not threading.main_thread()
    ):
        yield
        return
    watched = {signal.SIGINT, signal.SIGTERM}
    if hasattr(signal, "SIGHUP"):
        watched.add(signal.SIGHUP)
    previous = mask(block, watched)
    try:
        yield
    finally:
        mask(restore, previous)
