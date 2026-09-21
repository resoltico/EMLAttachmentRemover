"""Small terminal-state decisions shared by the batch orchestration path."""

from __future__ import annotations

from typing import TYPE_CHECKING

from . import report_stream
from .cancellation import CancellationSignal
from .domain import ItemPhase, ItemStatus

if TYPE_CHECKING:
    from collections.abc import Callable

    from .domain import BatchLedger, LedgerItem


def skip_inventory_failure(
    item: LedgerItem, ledger: BatchLedger, *, fail_fast: bool
) -> bool:
    """Apply the fail-fast terminalization rule after an inventory failure.

    Returns:
        ``True`` only when the caller must stop starting later inputs.

    """
    if item.status is None or not fail_fast:
        return False
    ledger.finalize_not_run("not run after fail-fast failure")
    return True


def after_item(
    item: LedgerItem,
    ledger: BatchLedger,
    *,
    fail_fast: bool,
    publication_cause: BaseException | None,
    internal_abort: Callable[[str], None],
) -> bool:
    """Archive one completed item and apply terminal batch-control precedence.

    Returns:
        ``True`` when no later input may be started.

    """
    if item.terminalized and not report_stream.archive_or_recover(ledger, item):
        return True
    if isinstance(publication_cause, CancellationSignal):
        ledger.record_interruption(publication_cause.name, ItemPhase.PUBLISHED.value)
        return True
    if isinstance(publication_cause, KeyboardInterrupt):
        ledger.record_interruption("SIGINT", ItemPhase.PUBLISHED.value)
        return True
    if isinstance(publication_cause, SystemExit):
        internal_abort("unexpected SystemExit after publication")
        return True
    if (
        item.status in {ItemStatus.FAILED, ItemStatus.PUBLISHED_WITH_ERROR}
        and fail_fast
    ):
        ledger.finalize_not_run("not run after fail-fast failure")
        return True
    return False
