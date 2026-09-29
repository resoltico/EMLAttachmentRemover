"""Refuse an input whose report evidence cannot be spooled, before any copy exists."""

from __future__ import annotations

import json
from typing import TYPE_CHECKING, Final

from . import report_spool, reporting_v3
from .domain import AppError, ExitCode

if TYPE_CHECKING:
    from .domain import BatchLedger, LedgerItem

# Fields the terminal record gains at publication: its receipt, final address, and
# sync results. The destination request's own size stands in for the final address.
TERMINAL_RESERVE_BYTES: Final = 2048
RECORD_LIMIT_MESSAGE: Final = (
    "report evidence for this message exceeds the per-item report record limit"
)
CAPACITY_LIMIT_MESSAGE: Final = "report evidence exceeds the remaining report capacity"


def _size(value: object) -> int:
    """Measure a value as the canonical report serializer would write it.

    Returns:
        The number of ASCII bytes of its canonical JSON.

    """
    return len(json.dumps(value, ensure_ascii=True, sort_keys=True, allow_nan=False))


def admit(ledger: BatchLedger, item: LedgerItem) -> None:
    """Refuse the item now if its terminal record could not be spooled later.

    The prepared evidence is measured before publication so an oversized record
    can never surface after a copy already exists. A refused item drops its bulky
    detail so its own failure record is small enough to be reported.

    Raises:
        AppError: If the record exceeds the per-record limit or remaining capacity.

    """
    spool = ledger.report_spool
    if not isinstance(spool, report_spool.ReportSpool):
        return
    record = reporting_v3.item_json(item)
    size = (
        _size(record)
        + TERMINAL_RESERVE_BYTES
        + 2 * _size(record["destination_request"])
    )
    if size > report_spool.MAX_RECORD_BYTES:
        message = RECORD_LIMIT_MESSAGE
    elif spool.bytes_written + size > report_spool.MAX_SPOOL_BYTES:
        message = CAPACITY_LIMIT_MESSAGE
    else:
        return
    item.transformation = None
    item.warnings.clear()
    raise AppError(ExitCode.PARSE_ERROR, message)
