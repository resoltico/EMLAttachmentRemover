"""The documented precedence that turns a terminal ledger into one process status."""

from __future__ import annotations

from typing import Final

from .domain import BatchLedger, ExitCode, ItemStatus, LedgerItem

ACCEPTED: Final = frozenset({
    ItemStatus.CREATED,
    ItemStatus.EXISTING_VERIFIED,
    ItemStatus.WOULD_CREATE,
})


def exit_code(ledger: BatchLedger) -> int:
    """Apply the documented terminal-state precedence to a completed ledger.

    Returns:
        The stable process code corresponding to the terminal ledger states.

    """
    if _is_interrupted(ledger):
        return int(ExitCode.INTERRUPTED)
    if (
        (
            ledger.batch_error is not None
            and ledger.batch_error.code is ExitCode.INTERNAL_ERROR
        )
        or _has_publication_error(ledger, ExitCode.INTERNAL_ERROR)
        or any(
            item.status is ItemStatus.FAILED
            and item.error is not None
            and item.error.code is ExitCode.INTERNAL_ERROR
            for item in ledger.items
        )
    ):
        return int(ExitCode.INTERNAL_ERROR)
    if ledger.batch_error is not None:
        return int(ledger.batch_error.code)
    if _has_status(ledger, ItemStatus.PUBLISHED_WITH_ERROR):
        return int(
            ExitCode.PUBLICATION_INCOMPLETE
            if len(ledger.items) == 1
            else ExitCode.BATCH_FAILURE
        )
    return _failure_exit(ledger)


def _is_interrupted(ledger: BatchLedger) -> bool:
    return (
        ledger.interruption is not None
        or _has_status(ledger, ItemStatus.CANCELLED)
        or _has_publication_error(ledger, ExitCode.INTERRUPTED)
    )


def _has_status(ledger: BatchLedger, status: ItemStatus) -> bool:
    return any(item.status is status for item in ledger.items)


def _has_publication_error(ledger: BatchLedger, code: ExitCode) -> bool:
    return any(
        item.status is ItemStatus.PUBLISHED_WITH_ERROR
        and item.error is not None
        and item.error.code is code
        for item in ledger.items
    )


def _failure_exit(ledger: BatchLedger) -> int:
    failed = [item for item in ledger.items if item.status is ItemStatus.FAILED]
    if not failed:
        return _remaining_exit(ledger)
    if len(ledger.items) != 1:
        return int(ExitCode.BATCH_FAILURE)
    return _single_failure_exit(failed[0])


def _remaining_exit(ledger: BatchLedger) -> int:
    return (
        int(ExitCode.BATCH_FAILURE)
        if any(item.status not in ACCEPTED for item in ledger.items)
        else int(ExitCode.SUCCESS)
    )


def _single_failure_exit(item: LedgerItem) -> int:
    return int(ExitCode.INTERNAL_ERROR if item.error is None else item.error.code)
