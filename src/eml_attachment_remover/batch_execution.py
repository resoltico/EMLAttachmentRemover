"""Batch report-storage ownership around the shared inventory and item pipeline."""

from __future__ import annotations

import os
from typing import TYPE_CHECKING

from . import report_stream
from .cancellation import CancellationSignal, checkpoint, install_cancellation_handlers
from .domain import (
    AppError,
    BatchLedger,
    ExitCode,
    FileIdentity,
    ItemPhase,
    ItemStatus,
    LedgerItem,
)

if TYPE_CHECKING:
    from collections.abc import Callable

    from .batch import BatchOptions


def run(
    ledger: BatchLedger,
    sources: list[str],
    options: BatchOptions,
    process: Callable[[BatchLedger, list[str], BatchOptions], None],
    *,
    limits: tuple[int, int],
) -> BatchLedger:
    """Transfer storage only with a terminal result; otherwise close it on unwind.

    Returns:
        The terminal ledger, optionally retaining bounded single-file evidence.

    Raises:
        AppError: If evidence retention is requested for more than one input.

    """
    if ledger.retain_evidence and len(sources) != 1:
        raise AppError(ExitCode.USAGE, "retained evidence requires exactly one input")
    try:
        with install_cancellation_handlers():
            _execute(ledger, sources, options, process, limits)
    except CancellationSignal as cancellation:
        if ledger.interruption is None:
            ledger.record_interruption(cancellation.name, ItemPhase.INVENTORIED.value)
    return ledger


def _process(
    ledger: BatchLedger,
    sources: list[str],
    options: BatchOptions,
    process: Callable[[BatchLedger, list[str], BatchOptions], None],
) -> None:
    """Turn processing interruption into the same terminal ledger as the CLI."""
    try:
        process(ledger, sources, options)
    except CancellationSignal as cancellation:
        ledger.record_interruption(cancellation.name, ItemPhase.INVENTORIED.value)
    except KeyboardInterrupt:
        ledger.record_interruption("SIGINT", ItemPhase.INVENTORIED.value)


def _execute(
    ledger: BatchLedger,
    sources: list[str],
    options: BatchOptions,
    process: Callable[[BatchLedger, list[str], BatchOptions], None],
    limits: tuple[int, int],
) -> None:
    """Run storage, inventory, and finalization with explicit safe checkpoints."""
    checkpoint()
    if not report_stream.start_or_fail(ledger):
        return
    checkpoint()
    argument_bytes = sum(len(os.fsencode(source)) for source in sources)
    if len(sources) > limits[0] or argument_bytes > limits[1]:
        ledger.finalize_not_run("batch exceeds native argument resource limit")
    else:
        _process(ledger, sources, options, process)
    ledger.finalize_not_run("not run")
    report_stream.archive_or_recover(ledger)
    checkpoint()


def prepare_candidate(
    item: LedgerItem,
    identity: FileIdentity,
    candidate: Callable[[LedgerItem, FileIdentity], None],
) -> bool:
    """Contain item-local preparation faults without hiding backend invariants.

    Returns:
        Whether candidate preparation completed for this item.

    Raises:
        AppError: If preparation reports a deliberate invariant or input failure.
        CancellationSignal: If processing is interrupted.
        MemoryError: If allocation fails.

    """
    try:
        candidate(item, identity)
    except AppError, CancellationSignal, MemoryError:
        raise
    except Exception as error:
        if item.phase not in {ItemPhase.BOUND, ItemPhase.PARSED, ItemPhase.CLASSIFIED}:
            raise
        item.finish(
            ItemStatus.FAILED,
            AppError(
                ExitCode.INTERNAL_ERROR,
                str(error) or type(error).__name__,
                phase=item.phase.value,
            ),
        )
        return False
    return True
