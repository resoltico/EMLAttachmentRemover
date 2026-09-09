"""Public single-file facade over the v3 batch implementation."""

from __future__ import annotations

from typing import TYPE_CHECKING

from .batch import BatchOptions, execute

if TYPE_CHECKING:
    from .domain import BatchLedger


def process_file(
    source: str,
    destination: str | None = None,
    *,
    existing: str = "error",
    dry_run: bool = False,
) -> BatchLedger:
    """Process one path through the exact same candidate pipeline as the CLI.

    Returns:
        The complete single-item ledger, including candidate and publication evidence.

    """
    options = BatchOptions(
        dry_run=dry_run,
        existing=existing,
        fail_fast=False,
        output=destination,
        output_dir=None,
    )
    return execute([source], options)
