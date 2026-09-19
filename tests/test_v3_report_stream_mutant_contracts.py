"""Behavioral regression contracts derived from terminal-report mutation evidence."""

from __future__ import annotations

import json
import os
from typing import cast

import pytest

from eml_attachment_remover import (
    cli,
    native_literal_address,
    report_spool,
    report_stream,
    reporting_v3,
)
from eml_attachment_remover.batch import BatchOptions
from eml_attachment_remover.cancellation import CancellationSignal
from eml_attachment_remover.domain import (
    AppError,
    BatchLedger,
    ExitCode,
    ItemStatus,
    PathValue,
)
from eml_attachment_remover.native_paths import path_value


def _failed(count: int = 1) -> BatchLedger:
    """Build one direct ledger with exactly ``count`` failed input rows.

    Returns:
        A complete terminal ledger.

    """
    ledger = BatchLedger.from_requests([
        path_value(f"source-{index}.eml") for index in range(count)
    ])
    for item in ledger.items:
        item.finish(ItemStatus.FAILED, AppError(ExitCode.PARSE_ERROR, "bad"))
    return ledger


def test_summary_counts_repeated_statuses_and_batch_error_rejects_ok() -> None:
    """Counts add rather than overwrite and any batch error makes reports not-ok."""
    ledger = _failed(2)
    assert report_stream._summary(ledger)["failed"] == 2  # ruff: ignore[private-member-access] - duplicate status count.
    ledger.items[0].status = ItemStatus.WOULD_CREATE
    ledger.items[1].status = ItemStatus.WOULD_CREATE
    ledger.batch_error = AppError(ExitCode.WRITE_ERROR, "spool")
    assert report_stream._top_level(ledger, "dry-run", 7)["ok"] is False  # ruff: ignore[private-member-access] - batch error precedence.
    assert reporting_v3.report(ledger, "dry-run", 7)["ok"] is False


def test_primary_and_emergency_spool_failures_have_exact_safe_diagnostics() -> None:
    """Invalid owner and every emergency mismatch expose the canonical error text."""
    ledger = _failed()
    ledger.report_spool = object()
    with pytest.raises(report_spool.ReportSpoolError) as invalid_owner:
        tuple(report_stream._records(ledger))  # ruff: ignore[private-member-access] - invalid primary owner.
    assert str(invalid_owner.value) == "terminal report spool has an invalid owner"

    ledger = _failed()
    report_stream.start(ledger)
    ledger.report_spool_failed = True
    emergency = cast("report_spool.ReportSpool", ledger.emergency_report_spool)
    try:
        emergency.path.write_bytes(b'{"index":0,"source_request":null}\n')
        with pytest.raises(report_spool.ReportSpoolError) as mismatch:
            tuple(report_stream._records(ledger))  # ruff: ignore[private-member-access] - reservation source mismatch.
        assert str(mismatch.value) == "terminal emergency report spool is corrupt"
    finally:
        report_stream.close(ledger)

    ledger = _failed()
    report_stream.start(ledger)
    ledger.report_spool_failed = True
    emergency = cast("report_spool.ReportSpool", ledger.emergency_report_spool)
    try:
        emergency.record_count = 0
        with pytest.raises(report_spool.ReportSpoolError) as incomplete:
            tuple(report_stream._records(ledger))  # ruff: ignore[private-member-access] - emergency count mismatch.
        assert str(incomplete.value) == "terminal emergency report spool is incomplete"
    finally:
        emergency.record_count = 1
        report_stream.close(ledger)


def test_json_writers_preserve_unicode_and_sort_nested_mapping_keys(
    capsys: pytest.CaptureFixture[str],
) -> None:
    """Canonical report writers never regress to ASCII escaping or insertion order."""
    report_stream._write_pair(  # ruff: ignore[private-member-access] - exact pair encoding.
        "pair", {"z": "ø", "a": "ä"}, terminal=True
    )
    assert capsys.readouterr().out == '"pair": {"a": "ä", "z": "ø"}'

    ledger = _failed()
    ledger.items[0].source_request = path_value("søurce.eml")
    report_stream._write_item_array(ledger)  # ruff: ignore[private-member-access] - item encoding.
    raw = capsys.readouterr().out
    assert "søurce.eml" in raw
    assert "\\u00f8" not in raw
    assert (
        json.loads("{" + raw + "}")["items"][0]["source_request"]["text"]
        == "søurce.eml"
    )


def test_emergency_boolean_guards_reject_each_independent_mismatch() -> None:
    """Source spelling and terminality are independent emergency evidence facts."""
    ledger = _failed()
    report_stream.start(ledger)
    ledger.report_spool_failed = True
    emergency = cast("report_spool.ReportSpool", ledger.emergency_report_spool)
    try:
        original = next(emergency.records())
        malformed = json.loads(original)
        malformed["source_request"] = None
        emergency.path.write_text(json.dumps(malformed) + "\n", encoding="utf-8")
        with pytest.raises(report_spool.ReportSpoolError, match="corrupt"):
            tuple(report_stream._records(ledger))  # ruff: ignore[private-member-access] - source evidence mismatch.
    finally:
        report_stream.close(ledger)


def test_primary_spool_requires_both_count_and_archival_completeness() -> None:
    """A matching count cannot disguise a ledger row that was never archived."""
    ledger = _failed()
    report_stream.start(ledger)
    try:
        spool = cast("report_spool.ReportSpool", ledger.report_spool)
        spool.append(json.dumps(reporting_v3.item_json(ledger.items[0])).encode())
        with pytest.raises(report_spool.ReportSpoolError) as incomplete:
            tuple(report_stream._records(ledger))  # ruff: ignore[private-member-access] - unarchived primary row.
        assert str(incomplete.value) == "terminal report spool is incomplete"
    finally:
        report_stream.close(ledger)


def test_reservation_and_unknown_diagnostic_preserve_exact_unicode_and_label(
    capsys: pytest.CaptureFixture[str],
) -> None:
    """Emergency reservations retain Unicode while absent source evidence is stable."""
    ledger = _failed()
    ledger.items[0].source_request = path_value("søurce.eml")
    assert b"s\xc3\xb8urce.eml" in report_stream._reservation(ledger.items[0])  # ruff: ignore[private-member-access] - reservation UTF-8.
    report_stream._write_record_diagnostics(  # ruff: ignore[private-member-access] - unknown source diagnostic.
        {"source_request": None, "error": {"code": "E", "message": "bad"}}
    )
    assert capsys.readouterr().err == "<unknown>: E: bad\n"


def test_second_partial_write_cannot_exceed_its_remaining_payload(
    monkeypatch: pytest.MonkeyPatch,
) -> None:
    """The bounded writer checks each partial write against remaining bytes."""
    writes = iter((1, 3))
    monkeypatch.setattr(os, "write", lambda *_args: next(writes))
    with pytest.raises(report_spool.ReportSpoolError) as oversized:
        report_spool._write_all(9, b"abc")  # ruff: ignore[private-member-access] - second-write bound.
    assert str(oversized.value) == "terminal report spool write made no progress"


def test_literal_receipt_missing_text_has_exact_error() -> None:
    """The literal native boundary never weakens the missing-address diagnostic."""
    with pytest.raises(OSError, match=r"^final address has no native text$") as missing:
        native_literal_address.open_final_address(PathValue(None, "missing", None))
    assert str(missing.value) == "final address has no native text"


def test_recovery_not_run_reason_and_close_clear_both_spool_owners() -> None:
    """Recovery terminalizes later work and close leaves no report owner behind."""
    ledger = BatchLedger.from_requests([
        path_value("first.eml"),
        path_value("later.eml"),
    ])
    ledger.items[0].finish(ItemStatus.FAILED, AppError(ExitCode.PARSE_ERROR, "bad"))
    report_stream.start(ledger)
    try:
        report_stream.recover(ledger)
        error = ledger.items[1].error
        assert error == AppError(
            ExitCode.BATCH_FAILURE,
            "not run after terminal report spool failure",
            phase="batch",
        )
    finally:
        report_stream.close(ledger)
    assert ledger.report_spool is None
    assert ledger.emergency_report_spool is None


def test_cli_preserves_every_direct_and_spooled_routing_argument(
    monkeypatch: pytest.MonkeyPatch,
) -> None:
    """Every CLI report channel receives its real ledger, mode, and exit code."""
    calls: list[tuple[object, ...]] = []
    options = BatchOptions(
        dry_run=False,
        existing="error",
        fail_fast=False,
        output=None,
        output_dir=None,
    )
    direct = _failed()
    monkeypatch.setattr(
        cli, "write_paths0", lambda ledger: calls.append(("paths", ledger))
    )
    cli._write_selected("paths0", direct, options, 5)  # ruff: ignore[private-member-access] - direct paths receipt.
    assert calls == [("paths", direct)]

    spooled = _failed()
    spooled.report_spool = object()
    monkeypatch.setattr(
        report_stream,
        "write_json",
        lambda ledger, mode, status: calls.append(("json", ledger, mode, status)),
    )
    cli._write_selected("json", spooled, options, 7)  # ruff: ignore[private-member-access] - spooled mode receipt.
    assert calls[-1] == ("json", spooled, "apply", 7)

    monkeypatch.setattr(
        report_stream,
        "write_human",
        lambda ledger: calls.append(("human", ledger)),
    )
    monkeypatch.setattr(
        report_stream, "close", lambda ledger: calls.append(("close", ledger))
    )
    state = cli._RunState(spooled)  # ruff: ignore[private-member-access] - cancellation routing state.
    assert cli._cancelled([], state, CancellationSignal(2, "SIGINT")) == 130  # ruff: ignore[private-member-access] - spooled human cancellation.
    assert calls[-2:] == [("human", spooled), ("close", spooled)]


def test_spooled_cleanup_failure_emits_no_contradictory_success_document(
    monkeypatch: pytest.MonkeyPatch,
    capsys: pytest.CaptureFixture[str],
) -> None:
    """Terminal cleanup must complete before the staged JSON can reach stdout."""
    ledger = _failed()
    report_stream.start(ledger)
    report_stream.archive_all(ledger)
    original_close = report_stream.close

    def fail_close(_ledger: BatchLedger) -> None:
        message = "synthetic cleanup failure"
        raise report_spool.ReportSpoolError(message)

    monkeypatch.setattr(report_stream, "close", fail_close)
    with pytest.raises(report_spool.ReportSpoolError, match="synthetic cleanup"):
        cli._write_then_close(  # ruff: ignore[private-member-access] - cleanup-before-output contract.
            "json",
            ledger,
            BatchOptions(
                dry_run=False,
                existing="error",
                fail_fast=False,
                output=None,
                output_dir=None,
            ),
            0,
        )
    captured = capsys.readouterr()
    assert not captured.out
    assert not captured.err
    monkeypatch.setattr(report_stream, "close", original_close)
    original_close(ledger)


def test_emergency_index_mismatch_is_not_masked_by_other_valid_fields() -> None:
    """A valid source spelling cannot compensate for an out-of-order reservation row."""
    ledger = _failed()
    report_stream.start(ledger)
    ledger.report_spool_failed = True
    emergency = cast("report_spool.ReportSpool", ledger.emergency_report_spool)
    try:
        raw = json.loads(next(emergency.records()))
        raw["index"] = 1
        emergency.path.write_text(json.dumps(raw) + "\n", encoding="utf-8")
        with pytest.raises(report_spool.ReportSpoolError, match="corrupt"):
            tuple(report_stream._records(ledger))  # ruff: ignore[private-member-access] - index evidence mismatch.
    finally:
        report_stream.close(ledger)
