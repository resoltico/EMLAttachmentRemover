"""Private terminal-record spooling and bounded schema-3 channel rendering."""

from __future__ import annotations

import json
import os
import sys
from base64 import b64decode
from collections.abc import Iterator, Mapping
from typing import Final, cast

from . import reporting_v3
from ._version import PROGRAM_VERSION
from .domain import (
    PROGRAM_NAME,
    SCHEMA_VERSION,
    SCOPE,
    AppError,
    BatchLedger,
    ExitCode,
    ItemStatus,
    LedgerItem,
)
from .report_spool import ReportSpool, ReportSpoolError

_ITEM_KEY: Final = "items"
_TOP_LEVEL_ORDER: Final = (
    "batch_error",
    "exit_code",
    "interrupted",
    "interruption",
    _ITEM_KEY,
    "mode",
    "ok",
    "program",
    "schema_version",
    "scope",
    "summary",
    "version",
)


def start(ledger: BatchLedger) -> None:
    """Reserve emergency status evidence before a batch can publish anything."""
    emergency = ReportSpool.create()
    try:
        for item in ledger.items:
            emergency.append(_reservation(item))
        ledger.report_spool = ReportSpool.create()
    except BaseException:
        emergency.close()
        raise
    ledger.emergency_report_spool = emergency


def start_or_fail(ledger: BatchLedger) -> bool:
    """Reserve report capacity or terminalize the batch before publication.

    Returns:
        ``True`` only when a normal and emergency report path are both ready.

    """
    try:
        start(ledger)
    except OSError:
        ledger.batch_error = AppError(
            ExitCode.WRITE_ERROR,
            "could not reserve terminal report spool",
            phase="report",
        )
        ledger.finalize_not_run("terminal report spool reservation failed")
        return False
    return True


def _reservation(item: LedgerItem) -> bytes:
    """Return one pre-publication, source-qualified emergency status reservation.

    Returns:
        Canonical, bounded JSON evidence for one requested input.

    """
    reservation: dict[str, object] = {
        "index": item.index,
        "source_request": reporting_v3._path(item.source_request),  # ruff: ignore[private-member-access] - canonical path serializer owner.
    }
    return reporting_v3._canonical_json(  # ruff: ignore[private-member-access] - canonical JSON serializer owner.
        reservation, ensure_ascii=False
    ).encode("utf-8")


def recover(ledger: BatchLedger, item: LedgerItem | None = None) -> None:
    """Switch to the reserved status path after a primary-spool write failure."""
    if ledger.report_spool_failed:
        return
    error = AppError(
        ExitCode.WRITE_ERROR,
        "terminal report spool failed",
        phase="report",
    )
    ledger.report_spool_failed = True
    ledger.batch_error = error
    if item is not None and item.terminalized:
        item.correct_report_failure(error)
    ledger.finalize_not_run("not run after terminal report spool failure")


def archive(ledger: BatchLedger, item: LedgerItem) -> None:
    """Spool one terminal item then release its aggregate terminal detail.

    Raises:
        ReportSpoolError: If the item is nonterminal or spool persistence fails.

    """
    spool = ledger.report_spool
    if spool is None or item.archived:
        return
    if not isinstance(spool, ReportSpool) or not item.terminalized:
        message = "terminal report item cannot be archived"
        raise ReportSpoolError(message)
    record = reporting_v3._canonical_json(  # ruff: ignore[private-member-access] - canonical schema record owner.
        reporting_v3.item_json(item), ensure_ascii=False
    ).encode("utf-8")
    spool.append(record)
    item.transformation = None
    item.warnings.clear()
    item.archived = True


def archive_all(ledger: BatchLedger) -> None:
    """Archive the contiguous terminal prefix without reordering report rows."""
    for item in ledger.items:
        if not item.archived and not item.terminalized:
            return
        archive(ledger, item)


def archive_or_recover(ledger: BatchLedger, item: LedgerItem | None = None) -> bool:
    """Archive an ordered prefix or activate the complete reserved status path.

    Returns:
        ``True`` when the primary spool remains usable for subsequent work.

    """
    try:
        archive_all(ledger)
    except OSError:
        recover(ledger, item)
        return False
    return True


def close(ledger: BatchLedger) -> None:
    """Release the exact private terminal-record spool owned by one ledger."""
    errors: list[ReportSpoolError] = []
    for spool in (ledger.report_spool, ledger.emergency_report_spool):
        if isinstance(spool, ReportSpool):
            try:
                spool.close()
            except ReportSpoolError as error:
                errors.append(error)
    ledger.report_spool = None
    ledger.emergency_report_spool = None
    if errors:
        raise errors[0]


def _records(ledger: BatchLedger) -> Iterator[dict[str, object]]:
    """Yield ordered terminal schema records from a spool or direct test ledger.

    Yields:
        Each complete terminal schema record in input order.

    Raises:
        ReportSpoolError: If archived records are malformed or unordered.

    """
    if ledger.report_spool_failed:
        yield from _emergency_records(ledger)
        return
    spool = ledger.report_spool
    if spool is None:
        yield from (reporting_v3.item_json(item) for item in ledger.items)
        return
    if not isinstance(spool, ReportSpool):
        message = "terminal report spool has an invalid owner"
        raise ReportSpoolError(message)
    records = spool.records()
    try:
        for index, raw in enumerate(records):
            try:
                record = json.loads(raw)
            except json.JSONDecodeError as error:
                message = "terminal report spool is corrupt"
                raise ReportSpoolError(message) from error
            if not isinstance(record, dict) or record.get("index") != index:
                message = "terminal report spool records are unordered"
                raise ReportSpoolError(message)
            yield cast("dict[str, object]", record)
    finally:
        records.close()
    if spool.record_count != len(ledger.items) or not all(
        item.archived for item in ledger.items
    ):
        message = "terminal report spool is incomplete"
        raise ReportSpoolError(message)


def _emergency_records(ledger: BatchLedger) -> Iterator[dict[str, object]]:
    """Yield minimal complete status records from the pre-reserved path.

    The reservation is checked before use.  Detailed records already committed
    to a primary spool are intentionally not trusted in this recovery mode:
    every item is represented once, in order, without retaining large MIME
    fingerprint or warning aggregates.

    Yields:
        One minimal complete schema record per terminal input, in input order.

    Raises:
        ReportSpoolError: If the pre-reserved status evidence is unusable.

    """
    spool = ledger.emergency_report_spool
    if not isinstance(spool, ReportSpool):
        message = "terminal emergency report spool is unavailable"
        raise ReportSpoolError(message)
    if spool.record_count != len(ledger.items):
        message = "terminal emergency report spool is incomplete"
        raise ReportSpoolError(message)
    records = spool.records()
    pairs = zip(records, ledger.items, strict=True)

    def render() -> Iterator[dict[str, object]]:
        """Validate and render one closed-owner emergency record stream."""
        for index, (raw, item) in enumerate(pairs):
            try:
                reservation = json.loads(raw)
            except json.JSONDecodeError as error:
                message = "terminal emergency report spool is corrupt"
                raise ReportSpoolError(message) from error
            if (
                not isinstance(reservation, dict)
                or reservation.get("index") != index
                or reservation.get("source_request")
                != reporting_v3._path(item.source_request)  # ruff: ignore[private-member-access] - canonical path serializer owner.
                or not item.terminalized
            ):
                message = "terminal emergency report spool is corrupt"
                raise ReportSpoolError(message)
            record = reporting_v3.item_json(item)
            record["transformation"] = None
            record["warnings"] = []
            yield record

    try:
        yield from render()
    finally:
        records.close()


def _summary(ledger: BatchLedger) -> dict[str, int]:
    """Return exact terminal counts without materializing item report records.

    Returns:
        A count for every terminal status and the complete input total.

    Raises:
        RuntimeError: If a caller requests output before terminalization.

    """
    counts = dict.fromkeys((status.value for status in ItemStatus), 0)
    for item in ledger.items:
        if item.status is None:
            message = "report requested before ledger terminalization"
            raise RuntimeError(message)
        counts[item.status.value] += 1
    counts["total"] = len(ledger.items)
    return counts


def _preflight(ledger: BatchLedger) -> None:
    """Validate a complete report source before any selected channel is emitted.

    This deliberately makes a second bounded pass over private spool data.  It
    prevents a corrupt committed record from producing a misleading partial JSON
    document or a partial set of Finder paths.

    """
    for _ in _records(ledger):
        pass


def _top_level(ledger: BatchLedger, mode: str, exit_code: int) -> dict[str, object]:
    """Create non-item report facts whose size is bounded by batch metadata.

    Returns:
        The report's bounded top-level facts, excluding its item array.

    """
    accepted = {ItemStatus.CREATED, ItemStatus.EXISTING_VERIFIED}
    if mode == "dry-run":
        accepted.add(ItemStatus.WOULD_CREATE)
    return {
        "batch_error": reporting_v3._error(ledger.batch_error),  # ruff: ignore[private-member-access] - canonical report field serializer.
        "exit_code": exit_code,
        "interrupted": ledger.interruption is not None,
        "interruption": reporting_v3._interruption(ledger.interruption),  # ruff: ignore[private-member-access] - canonical report field serializer.
        "mode": mode,
        "ok": ledger.batch_error is None
        and all(item.status in accepted for item in ledger.items),
        "program": PROGRAM_NAME,
        "schema_version": SCHEMA_VERSION,
        "scope": SCOPE,
        "summary": _summary(ledger),
        "version": PROGRAM_VERSION,
    }


def _write_pair(name: str, value: object, *, terminal: bool) -> None:
    """Write one canonical top-level JSON pair and its required separator."""
    sys.stdout.write(json.dumps(name) + ": ")
    sys.stdout.write(json.dumps(value, ensure_ascii=False, sort_keys=True))
    if not terminal:
        sys.stdout.write(", ")


def _write_item_array(ledger: BatchLedger) -> None:
    """Write the sole streamed JSON array without retaining its records."""
    sys.stdout.write(json.dumps(_ITEM_KEY) + ": [")
    for index, record in enumerate(_records(ledger)):
        if index:
            sys.stdout.write(", ")
        sys.stdout.write(json.dumps(record, ensure_ascii=False, sort_keys=True))
    sys.stdout.write("]")


def write_json(ledger: BatchLedger, mode: str, exit_code: int) -> None:
    """Write one ordered JSON document while streaming each terminal item record."""
    _preflight(ledger)
    values = _top_level(ledger, mode, exit_code)
    sys.stdout.write("{")
    for name in _TOP_LEVEL_ORDER:
        if name == _ITEM_KEY:
            _write_item_array(ledger)
        else:
            _write_pair(name, values[name], terminal=name == _TOP_LEVEL_ORDER[-1])
        if name == _ITEM_KEY:
            sys.stdout.write(", ")
    sys.stdout.write("}\n")


def write_human(ledger: BatchLedger) -> None:
    """Render each terminal spool record without retaining the complete document."""
    _preflight(ledger)
    for record in _records(ledger):
        reporting_v3.write_human({_ITEM_KEY: [record]})


def _write_path_record(record: Mapping[str, object]) -> None:
    """Write one accepted native path only when its final receipt is usable."""
    publication = record.get("publication")
    final = (
        publication.get("final_address") if isinstance(publication, Mapping) else None
    )
    text = final.get("text") if isinstance(final, Mapping) else None
    accepted = {ItemStatus.CREATED.value, ItemStatus.EXISTING_VERIFIED.value}
    if record.get("status") in accepted and isinstance(text, str):
        native = final.get("native_base64") if isinstance(final, Mapping) else None
        value = b64decode(native) if isinstance(native, str) else os.fsencode(text)
        sys.stdout.buffer.write(value + b"\0")


def _write_record_diagnostics(record: Mapping[str, object]) -> None:
    """Write source-qualified error and warning facts for one spooled item."""
    source = record.get("source_request")
    display = source.get("display") if isinstance(source, Mapping) else "<unknown>"
    error = record.get("error")
    if isinstance(error, Mapping):
        reporting_v3._safe(  # ruff: ignore[private-member-access] - canonical display writer.
            sys.stderr,
            f"{display}: {error.get('code')}: {error.get('message')}",
        )
    warnings = record.get("warnings")
    if isinstance(warnings, list):
        for warning in warnings:
            if isinstance(warning, Mapping):
                reporting_v3._safe(  # ruff: ignore[private-member-access] - canonical display writer.
                    sys.stderr,
                    f"{display}: {warning.get('code')}: {warning.get('message')}",
                )


def write_paths0(ledger: BatchLedger) -> None:
    """Emit accepted paths and diagnostics from each streamed terminal record."""
    _preflight(ledger)
    for record in _records(ledger):
        _write_path_record(record)
        _write_record_diagnostics(record)
