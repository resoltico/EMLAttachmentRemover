"""Bounded recovery staging using retained ledger receipts and memory only."""

from __future__ import annotations

import sys
from contextlib import redirect_stderr, redirect_stdout
from dataclasses import replace
from io import BytesIO, TextIOWrapper
from typing import TYPE_CHECKING, Final, override

from . import report_stream
from .domain import AppError
from .report_delivery import StagedChannels
from .report_spool import MAX_SPOOL_BYTES

if TYPE_CHECKING:
    from contextlib import ExitStack

    from .domain import BatchLedger

# Recovery drops MIME detail. Allow JSON separators and top-level metadata on
# top of the admitted terminal-record capacity; each channel has its own bound.
MEMORY_LIMIT: Final = 2 * MAX_SPOOL_BYTES


class BoundedBuffer(BytesIO):
    """Reject growth beyond the emergency channel's memory budget."""

    @override
    def write(self, payload: object) -> int:
        """Accept a byte payload within the budget.

        Returns:
            The accepted byte count.

        Raises:
            OSError: If the channel would exceed its memory budget.

        """
        view = memoryview(payload)  # type: ignore[arg-type]
        if self.tell() + view.nbytes > MEMORY_LIMIT:
            message = "emergency report exceeds its memory capacity"
            raise OSError(message)
        return super().write(view)


def stage(
    resources: ExitStack,
    ledger: BatchLedger,
    output_format: str,
    mode: str,
    status: int,
) -> StagedChannels:
    """Seal a recovery report without accessing either spool or writable storage.

    Returns:
        Memory channels owned by the publication's resource scope.

    """
    items = [
        replace(
            item,
            source=None,
            destination=None,
            transformation=None,
            verification=None,
            warnings=[],
            error=None
            if item.error is None
            else AppError(item.error.code, item.error.message, None, item.error.phase),
        )
        for item in ledger.items
    ]
    retained = replace(
        ledger,
        items=items,
        report_spool=None,
        emergency_report_spool=None,
        report_spool_failed=False,
    )
    encoding = getattr(sys.stdout, "encoding", None) or "utf-8"
    channels = StagedChannels(
        resources.enter_context(
            TextIOWrapper(
                BoundedBuffer(),
                encoding=encoding if output_format == "human" else "ascii",
                newline="",
            )
        ),
        resources.enter_context(
            TextIOWrapper(
                BoundedBuffer(),
                encoding=getattr(sys.stderr, "encoding", None) or "utf-8",
                newline="",
            )
        ),
    )
    with redirect_stdout(channels.out), redirect_stderr(channels.err):
        if output_format == "json":
            report_stream.write_json(retained, mode, status)
        elif output_format == "paths0":
            report_stream.write_paths0(retained)
        else:
            report_stream.write_human(retained)
    channels.seal()
    return channels
