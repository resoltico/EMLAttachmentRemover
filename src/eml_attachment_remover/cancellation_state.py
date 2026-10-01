"""Invocation-owned cooperative requests and bounded stalled-processing escape."""

from __future__ import annotations

import threading
import time
from contextvars import ContextVar
from dataclasses import dataclass, field
from typing import Final


@dataclass(slots=True)
class CancellationState:
    """A signal handler only records; safe checkpoints acknowledge requests."""

    signals: list[int] = field(default_factory=list)
    acknowledged: int = 0
    requested_at: float = 0.0
    active: bool = False
    owner_thread: int = field(default_factory=threading.get_ident)
    stop: threading.Event = field(default_factory=threading.Event, repr=False)
    delivering: bool = False
    critical: int = 0

    def record(self, number: int, _frame: object) -> None:
        """Record a request without locks or asynchronous exceptions."""
        if not self.signals:
            self.requested_at = time.monotonic()
        self.signals.append(number)


CURRENT: Final[ContextVar[CancellationState | None]] = ContextVar(
    "eml_invocation_cancellation", default=None
)
