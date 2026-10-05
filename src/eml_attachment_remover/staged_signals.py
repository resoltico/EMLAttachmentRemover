"""Narrow signal deferral used only around the publication visibility edge."""

from __future__ import annotations

import signal
import threading
from contextlib import contextmanager
from typing import TYPE_CHECKING

from .cancellation_state import CURRENT

if TYPE_CHECKING:
    from collections.abc import Generator


@contextmanager
def defer_signals() -> Generator[None]:
    """Defer catchable process signals across one nonterminal publication edge."""
    # Cooperative handlers must see the request immediately; they cannot throw
    # through this operation. Standalone callers retain the POSIX mask boundary.
    if CURRENT.get() is not None:
        yield
        return
    # Capability lookups keep standalone publication usable on non-POSIX hosts.
    mask = getattr(signal, "pthread_sigmask", None)
    block = getattr(signal, "SIG_BLOCK", None)
    restore = getattr(signal, "SIG_SETMASK", None)
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
