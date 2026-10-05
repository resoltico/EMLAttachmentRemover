"""Advisory attempt counts independent of authoritative batch outcomes."""

from __future__ import annotations

from typing import TYPE_CHECKING

from .domain import ItemStatus

if TYPE_CHECKING:
    from collections.abc import Callable

    from .domain import LedgerItem


class BatchProgress:
    """Count completed attempts once, without treating them as successful copies."""

    def __init__(self, total: int, observer: Callable[[int, int], None] | None) -> None:
        """Start one bounded batch's optional observer."""
        self.total = total
        self.completed = 0
        self.observer = observer
        self.notify()

    def observe(self, item: LedgerItem) -> None:
        """Notify after a completed attempt, excluding cancelled/unprocessed rows."""
        if item.status not in {None, ItemStatus.CANCELLED, ItemStatus.NOT_RUN}:
            self.completed += 1
            self.notify()

    def notify(self) -> None:
        """Keep ordinary observer faults outside authoritative outcomes.

        Raises:
            MemoryError: If the observer exhausts memory.

        """
        if self.observer is None:
            return
        try:
            self.observer(self.completed, self.total)
        except MemoryError:
            raise
        except Exception:  # ruff: ignore[blind-except] - advisory observers cannot alter batch outcomes; process-control and memory failures propagate.
            self.observer = None
