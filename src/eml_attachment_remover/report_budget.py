"""Exact report-capacity accounting: reserve a record's terminal form before publishing.

The terminal record of an item is its prepared record plus fields that only exist
once it has run: a publication receipt with a final address, a status, and an
error. Admission serializes the prepared record *with worst-case values for those
fields*, through the one canonical serializer, so the bound cannot drift from the
report format. The address bound is enforced when the destination is bound, and
error messages are capped when serialized, so every worst case is real.
"""

from __future__ import annotations

import json
from dataclasses import dataclass, replace
from typing import TYPE_CHECKING, Final

from . import report_spool, reporting_v3
from .domain import (
    AppError,
    ExitCode,
    FileIdentity,
    ItemPhase,
    ItemStatus,
    PathValue,
    PublicationReceipt,
)
from .native_values import MAX_ADDRESS_UNITS, path_value

if TYPE_CHECKING:
    from .domain import BatchLedger, LedgerItem

RECORD_LIMIT_MESSAGE: Final = (
    "report evidence for this message exceeds the per-item report record limit"
)
CAPACITY_LIMIT_MESSAGE: Final = "report evidence exceeds the remaining report capacity"
# Every enumerated receipt word is shorter than this; a test proves it.
WORST_WORD: Final = "w" * 32
WORST_NUMBER: Final = 10**30
LONGEST_STATUS: Final = max(ItemStatus, key=len)
LONGEST_PHASE: Final = max(ItemPhase, key=len)
LONGEST_CODE: Final = max(ExitCode, key=lambda code: len(code.name))
NEWLINE_BYTES: Final = 1


def _size(document: dict[str, object]) -> int:
    """Measure a record as the canonical serializer writes it (without newline).

    Returns:
        The number of ASCII bytes of its canonical JSON.

    """
    return len(json.dumps(document, ensure_ascii=True, sort_keys=True, allow_nan=False))


def worst_address() -> PathValue:
    """Build the longest address a receipt may carry, with the costliest characters.

    Control characters expand to six bytes in text and seven in display, more per
    native unit than any other character.

    Returns:
        A maximal-length path value.

    """
    return path_value("\x01" * MAX_ADDRESS_UNITS)


def _worst_error() -> AppError:
    """Build the longest error a terminal record may carry.

    Errors raised after admission come from publication and carry no MIME path.

    Returns:
        A maximal-message error with the longest code name and phase.

    """
    return AppError(
        LONGEST_CODE, "\x01" * (reporting_v3.MAX_ERROR_BYTES // 6), None, WORST_WORD
    )


def _worst_receipt() -> PublicationReceipt:
    identity = FileIdentity(WORST_NUMBER, WORST_NUMBER, WORST_WORD, WORST_NUMBER)
    return PublicationReceipt(
        WORST_WORD,
        identity,
        "f" * 64,
        WORST_WORD,
        WORST_WORD,
        address_verified=True,
        final_address=worst_address(),
        temp_cleanup=WORST_WORD,
    )


def terminal_size(item: LedgerItem) -> int:
    """Measure an item's largest possible terminal record from its prepared state.

    Returns:
        Bytes of the record with a worst-case receipt, status, phase, and error.

    """
    worst = replace(
        item,
        status=LONGEST_STATUS,
        phase=LONGEST_PHASE,
        terminalized=True,
        publication=_worst_receipt(),
        error=_worst_error(),
    )
    return _size(reporting_v3.item_json(worst))


def minimal_record(item: LedgerItem, *, worst: bool = False) -> dict[str, object]:
    """Reduce an item to the evidence every report keeps: its identity and outcome.

    Returns:
        The record without source, destination, transformation, or verification
        detail, and with a bounded error. With ``worst`` the outcome fields take
        their largest values, for reserving room for a later item.

    """
    if worst:
        error: AppError | None = _worst_error()
    elif item.error is None:
        error = None
    else:
        error = AppError(item.error.code, item.error.message, None, item.error.phase)
    kept = replace(
        item,
        status=LONGEST_STATUS if worst else item.status,
        phase=LONGEST_PHASE if worst else item.phase,
        source=None,
        destination=None,
        transformation=None,
        verification=None,
        warnings=[],
        error=error,
    )
    return reporting_v3.item_json(kept)


@dataclass(frozen=True, slots=True)
class ReportBudget:
    """Room that must stay free for the inputs after each one.

    ``tail[i]`` is the spool bytes reserved for inputs ``i + 1`` onward, each at its
    minimal terminal record, so no later input can ever lose its place.
    """

    tail: tuple[int, ...]

    def refusal(self, spool: report_spool.ReportSpool, item: LedgerItem) -> str | None:
        """Say why an item's worst terminal record could not be spooled.

        Returns:
            The refusal message, or ``None`` when the record is guaranteed to fit.

        """
        size = terminal_size(item)
        if size > report_spool.MAX_RECORD_BYTES:
            return RECORD_LIMIT_MESSAGE
        used = spool.bytes_written + size + NEWLINE_BYTES + self.tail[item.index]
        return CAPACITY_LIMIT_MESSAGE if used > report_spool.MAX_SPOOL_BYTES else None

    def fits(self, spool: report_spool.ReportSpool, index: int, size: int) -> bool:
        """Report whether an actual record may be spooled without starving later ones.

        Returns:
            ``True`` when the record and every later input's reserve fit.

        """
        used = spool.bytes_written + size + NEWLINE_BYTES + self.tail[index]
        return size <= report_spool.MAX_RECORD_BYTES and (
            used <= report_spool.MAX_SPOOL_BYTES
        )


def plan(ledger: BatchLedger) -> ReportBudget | None:
    """Reserve every input's minimal terminal record, before any input runs.

    Returns:
        The budget, or ``None`` when even minimal records exceed the spool.

    """
    needs = [
        _size(minimal_record(item, worst=True)) + NEWLINE_BYTES for item in ledger.items
    ]
    tail: list[int] = []
    running = 0
    for need in reversed(needs):
        tail.append(running)
        running += need
    if running > report_spool.MAX_SPOOL_BYTES:
        return None
    return ReportBudget(tuple(reversed(tail)))


def admit(ledger: BatchLedger, item: LedgerItem) -> None:
    """Refuse the item now if its terminal record could not be spooled later.

    A refused item drops its bulky detail so its own failure record is small.

    Raises:
        AppError: If the record exceeds the per-record limit or remaining capacity.

    """
    spool = ledger.report_spool
    budget = ledger.report_budget
    if not isinstance(spool, report_spool.ReportSpool) or not isinstance(
        budget, ReportBudget
    ):
        return
    message = budget.refusal(spool, item)
    if message is None:
        return
    item.transformation = None
    item.warnings.clear()
    raise AppError(ExitCode.PARSE_ERROR, message)
