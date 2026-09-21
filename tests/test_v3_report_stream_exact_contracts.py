"""Exact observable contracts for bounded terminal report recovery."""

from __future__ import annotations

import json
import os
from base64 import b64encode
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


def _ledger(*statuses: ItemStatus) -> BatchLedger:
    """Build a fully terminal direct ledger with exact source-order diagnostics.

    Returns:
        A ledger whose input rows have the requested terminal statuses.

    """
    ledger = BatchLedger.from_requests([
        path_value(f"source-{index}.eml") for index in range(len(statuses))
    ])
    for item, status in zip(ledger.items, statuses, strict=True):
        item.finish(status, AppError(ExitCode.PARSE_ERROR, f"error-{item.index}"))
    return ledger


@pytest.mark.skipif(os.name == "nt", reason="POSIX literal-open flags")
def test_literal_posix_open_uses_every_required_no_follow_flag(
    monkeypatch: pytest.MonkeyPatch,
) -> None:
    """The literal POSIX path is a no-follow, nonblocking, cloexec open exactly."""
    calls: list[tuple[bytes, int]] = []

    def open_path(path: bytes, flags: int) -> int:
        calls.append((path, flags))
        return 91

    monkeypatch.setattr(os, "open", open_path)
    address = PathValue("/literal.eml", "literal", "L2xpdGVyYWwuZW1s")
    assert native_literal_address.open_final_address(address) == 91
    assert calls == [
        (
            b"/literal.eml",
            os.O_RDONLY
            | getattr(os, "O_NONBLOCK", 0)
            | getattr(os, "O_NOFOLLOW", 0)
            | getattr(os, "O_CLOEXEC", 0),
        )
    ]


@pytest.mark.skipif(os.name == "nt", reason="POSIX literal-open flag fallbacks")
def test_literal_posix_open_defaults_missing_optional_flags_to_zero(
    monkeypatch: pytest.MonkeyPatch,
) -> None:
    """Absent optional POSIX flags remain a safe zero, never a truthy sentinel."""
    calls: list[int] = []

    def open_path(_path: bytes, flags: int) -> int:
        calls.append(flags)
        return 92

    monkeypatch.setattr(os, "open", open_path)
    monkeypatch.delattr(os, "O_NONBLOCK")
    monkeypatch.delattr(os, "O_NOFOLLOW")
    monkeypatch.delattr(os, "O_CLOEXEC")
    assert (
        native_literal_address.open_final_address(
            PathValue("/literal.eml", "literal", "L2xpdGVyYWwuZW1s")
        )
        == 92
    )
    assert calls == [os.O_RDONLY]


def test_reservation_recovery_and_summary_preserve_exact_public_facts() -> None:
    """Reservation, recovery, and every terminal count carry their public values."""
    ledger = _ledger(
        ItemStatus.CREATED,
        ItemStatus.EXISTING_VERIFIED,
        ItemStatus.WOULD_CREATE,
        ItemStatus.FAILED,
        ItemStatus.CANCELLED,
        ItemStatus.NOT_RUN,
        ItemStatus.PUBLISHED_WITH_ERROR,
    )
    reservation = json.loads(report_stream._reservation(ledger.items[0]))  # ruff: ignore[private-member-access] - exact emergency receipt.
    assert reservation == {
        "index": 0,
        "source_request": reporting_v3._path(ledger.items[0].source_request),  # ruff: ignore[private-member-access] - schema path owner.
    }
    assert report_stream._summary(ledger) == {  # ruff: ignore[private-member-access] - exact terminal count receipt.
        "created": 1,
        "existing_verified": 1,
        "would_create": 1,
        "failed": 1,
        "cancelled": 1,
        "not_run": 1,
        "published_with_error": 1,
        "total": 7,
    }

    report_stream.recover(ledger, ledger.items[0])
    assert ledger.report_spool_failed is True
    assert ledger.batch_error == AppError(
        ExitCode.WRITE_ERROR,
        "terminal report spool failed",
        phase="report",
    )
    assert ledger.items[0].status is ItemStatus.PUBLISHED_WITH_ERROR
    assert ledger.items[0].error == ledger.batch_error
    assert [item.status for item in ledger.items[1:]] == [
        ItemStatus.EXISTING_VERIFIED,
        ItemStatus.WOULD_CREATE,
        ItemStatus.FAILED,
        ItemStatus.CANCELLED,
        ItemStatus.NOT_RUN,
        ItemStatus.PUBLISHED_WITH_ERROR,
    ]
    assert ledger.items[1].error == AppError(ExitCode.PARSE_ERROR, "error-1")


def test_start_or_fail_reports_exact_typed_reservation_failure(
    monkeypatch: pytest.MonkeyPatch,
) -> None:
    """Unavailable status capacity prevents publication with one typed receipt."""
    ledger = BatchLedger.from_requests([path_value("source.eml")])

    def unavailable(_ledger: BatchLedger) -> None:
        message = "disk unavailable"
        raise OSError(message)

    monkeypatch.setattr(report_stream, "start", unavailable)
    assert report_stream.start_or_fail(ledger) is False
    assert ledger.batch_error == AppError(
        ExitCode.WRITE_ERROR,
        "could not reserve terminal report spool",
        phase="report",
    )
    assert ledger.items[0].status is ItemStatus.NOT_RUN
    assert ledger.items[0].error == AppError(
        ExitCode.BATCH_FAILURE,
        "terminal report spool reservation failed",
        phase="batch",
    )


def test_spooled_record_errors_have_exact_distinct_messages() -> None:
    """Malformed, unordered, incomplete, and emergency receipts never alias errors."""
    cases = (
        (b"not-json", "terminal report spool is corrupt"),
        (b'{"index":1}', "terminal report spool records are unordered"),
    )
    for raw, expected in cases:
        ledger = _ledger(ItemStatus.FAILED)
        report_stream.start(ledger)
        spool = cast("report_spool.ReportSpool", ledger.report_spool)
        try:
            spool.append(raw)
            ledger.items[0].archived = True
            with pytest.raises(report_spool.ReportSpoolError) as rejected:
                tuple(report_stream._records(ledger))  # ruff: ignore[private-member-access] - primary spool error receipt.
            assert str(rejected.value) == expected
        finally:
            report_stream.close(ledger)

    ledger = _ledger(ItemStatus.FAILED)
    report_stream.start(ledger)
    try:
        with pytest.raises(report_spool.ReportSpoolError) as rejected:
            tuple(report_stream._records(ledger))  # ruff: ignore[private-member-access] - incomplete primary receipt.
        assert str(rejected.value) == "terminal report spool is incomplete"
    finally:
        report_stream.close(ledger)

    unfinished = BatchLedger.from_requests([path_value("unfinished.eml")])
    with pytest.raises(RuntimeError) as nonterminal:
        report_stream._summary(unfinished)  # ruff: ignore[private-member-access] - exact terminalization diagnostic.
    assert str(nonterminal.value) == "report requested before ledger terminalization"


def test_emergency_record_retains_only_exact_status_schema_data() -> None:
    """Emergency rendering discards aggregates but keeps the complete source status."""
    ledger = _ledger(ItemStatus.FAILED)
    report_stream.start(ledger)
    ledger.report_spool_failed = True
    try:
        record = next(report_stream._records(ledger))  # ruff: ignore[private-member-access] - emergency record receipt.
        assert record == {
            "index": 0,
            "phase": "requested",
            "status": "failed",
            "terminalized": True,
            "source_request": reporting_v3._path(ledger.items[0].source_request),  # ruff: ignore[private-member-access] - schema path owner.
            "destination_request": None,
            "source": None,
            "destination": None,
            "transformation": None,
            "verification": None,
            "publication": None,
            "warnings": [],
            "error": {
                "code": "PARSE_ERROR",
                "message": "error-0",
                "mime_path": None,
                "phase": None,
            },
        }
    finally:
        report_stream.close(ledger)


def test_emergency_and_archive_failures_preserve_exact_terminal_semantics() -> None:
    """Every recovery-only error is distinct, and archive releases only receipts."""
    ledger = _ledger(ItemStatus.FAILED)
    ledger.report_spool_failed = True
    with pytest.raises(report_spool.ReportSpoolError) as unavailable:
        tuple(report_stream._records(ledger))  # ruff: ignore[private-member-access] - absent emergency receipt.
    assert str(unavailable.value) == "terminal emergency report spool is unavailable"

    ledger = BatchLedger.from_requests([path_value("source.eml")])
    report_stream.start(ledger)
    try:
        with pytest.raises(report_spool.ReportSpoolError) as nonterminal:
            report_stream.archive(ledger, ledger.items[0])
        assert str(nonterminal.value) == "terminal report item cannot be archived"
        ledger.items[0].finish(ItemStatus.FAILED, AppError(ExitCode.PARSE_ERROR, "bad"))
        report_stream.archive(ledger, ledger.items[0])
        assert ledger.items[0].archived is True
        assert ledger.items[0].warnings == []
        report_stream.archive(ledger, ledger.items[0])
        record = next(iter(report_stream._records(ledger)))  # ruff: ignore[private-member-access] - stored schema receipt.
        assert record["error"] == {
            "code": "PARSE_ERROR",
            "message": "bad",
            "mime_path": None,
            "phase": None,
        }
    finally:
        report_stream.close(ledger)


def test_archive_spool_uses_the_ascii_canonical_json_projection() -> None:
    """The private spool stores the same portable JSON evidence as public output."""
    ledger = BatchLedger.from_requests([path_value("søurce.eml")])
    ledger.items[0].finish(ItemStatus.FAILED, AppError(ExitCode.PARSE_ERROR, "ø"))
    report_stream.start(ledger)
    spool = cast("report_spool.ReportSpool", ledger.report_spool)
    try:
        report_stream.archive(ledger, ledger.items[0])
        raw = next(spool.records())
        assert b"s\\u00f8urce.eml" in raw
        assert b"\xc3\xb8" not in raw
    finally:
        report_stream.close(ledger)


def test_emergency_corruption_messages_and_archive_recovery_are_exact(
    monkeypatch: pytest.MonkeyPatch,
) -> None:
    """Reserved evidence is mandatory and an archive fault returns ``False`` once."""
    ledger = _ledger(ItemStatus.FAILED)
    report_stream.start(ledger)
    ledger.report_spool_failed = True
    emergency = cast("report_spool.ReportSpool", ledger.emergency_report_spool)
    try:
        emergency.path.write_bytes(b"not-json\n")
        with pytest.raises(report_spool.ReportSpoolError) as corrupt:
            tuple(report_stream._records(ledger))  # ruff: ignore[private-member-access] - corrupt reservation receipt.
        assert str(corrupt.value) == "terminal emergency report spool is corrupt"
    finally:
        report_stream.close(ledger)

    ledger = _ledger(ItemStatus.FAILED)
    report_stream.start(ledger)
    try:
        monkeypatch.setattr(
            report_stream,
            "archive_all",
            lambda _ledger: (_ for _ in ()).throw(OSError("full")),
        )
        assert report_stream.archive_or_recover(ledger, ledger.items[0]) is False
        assert ledger.batch_error == AppError(
            ExitCode.WRITE_ERROR,
            "terminal report spool failed",
            phase="report",
        )
        assert report_stream.archive_or_recover(ledger, ledger.items[0]) is False
    finally:
        report_stream.close(ledger)


def test_top_level_and_json_writers_are_exact_for_apply_and_dry_run(
    capsys: pytest.CaptureFixture[str],
) -> None:
    """Every streamed delimiter, field, and channel decision is externally stable."""
    ledger = _ledger(ItemStatus.WOULD_CREATE)
    assert report_stream._top_level(ledger, "apply", 9)["ok"] is False  # ruff: ignore[private-member-access] - apply acceptance contract.
    assert report_stream._top_level(ledger, "dry-run", 0)["ok"] is True  # ruff: ignore[private-member-access] - dry-run acceptance contract.
    report_stream._write_pair("n", {"value": "ü"}, terminal=False)  # ruff: ignore[private-member-access] - JSON pair contract.
    report_stream._write_pair("last", None, terminal=True)  # ruff: ignore[private-member-access] - final JSON pair contract.
    assert capsys.readouterr().out == '"n": {"value": "\\u00fc"}, "last": null'

    expected_items = ", ".join(
        json.dumps(reporting_v3.item_json(item), ensure_ascii=False, sort_keys=True)
        for item in ledger.items
    )
    report_stream._write_item_array(ledger)  # ruff: ignore[private-member-access] - item stream contract.
    assert capsys.readouterr().out == f'"items": [{expected_items}]'
    report_stream.write_json(ledger, "dry-run", 0)
    document = json.loads(capsys.readouterr().out)
    assert list(document) == [
        "batch_error",
        "exit_code",
        "interrupted",
        "interruption",
        "items",
        "mode",
        "ok",
        "program",
        "schema_version",
        "scope",
        "summary",
        "version",
    ]
    assert document["items"] == [reporting_v3.item_json(ledger.items[0])]


def test_paths_and_diagnostics_render_exact_accepted_and_rejected_values(
    capfd: pytest.CaptureFixture[str],
) -> None:
    """paths0 and stderr have exact native and source-qualified behaviors."""
    native = b"/native-output.eml"
    accepted = {
        "status": "created",
        "publication": {
            "final_address": {
                "text": "/display-output.eml",
                "native_base64": b64encode(native).decode(),
            }
        },
    }
    fallback = {
        "status": "existing_verified",
        "publication": {"final_address": {"text": "/fallback.eml"}},
    }
    rejected = {
        "status": "published_with_error",
        "publication": accepted["publication"],
    }
    report_stream._write_path_record(accepted)  # ruff: ignore[private-member-access] - native paths0 contract.
    report_stream._write_path_record(fallback)  # ruff: ignore[private-member-access] - fallback paths0 contract.
    report_stream._write_path_record(rejected)  # ruff: ignore[private-member-access] - rejected paths0 contract.
    output, errors = capfd.readouterr()
    assert output.encode() == native + b"\0/fallback.eml\0"
    assert not errors

    report_stream._write_record_diagnostics(  # ruff: ignore[private-member-access] - safe diagnostic contract.
        {
            "source_request": {"display": "source.eml"},
            "error": {"code": "WRITE_ERROR", "message": "failed"},
            "warnings": [
                {"code": "WARN", "message": "warning"},
                "ignored",
            ],
        }
    )
    assert capfd.readouterr().err == (
        "source.eml: WRITE_ERROR: failed\nsource.eml: WARN: warning\n"
    )


def test_cli_selected_and_cancelled_channels_keep_exact_report_mode_and_status(
    capsys: pytest.CaptureFixture[str],
) -> None:
    """CLI selection never silently swaps direct and spooled report semantics."""
    options = BatchOptions(
        dry_run=False,
        existing="error",
        fail_fast=False,
        output=None,
        output_dir=None,
    )
    direct = _ledger(ItemStatus.FAILED)
    cli._write_selected("json", direct, options, 5)  # ruff: ignore[private-member-access] - direct JSON channel.
    document = json.loads(capsys.readouterr().out)
    assert (document["mode"], document["exit_code"], document["ok"]) == (
        "apply",
        5,
        False,
    )
    cli._write_selected("human", direct, options, 5)  # ruff: ignore[private-member-access] - direct human channel.
    rendered = capsys.readouterr()
    assert rendered.out == "failed: source-0.eml\n"
    assert rendered.err == "source-0.eml: PARSE_ERROR: error-0\n"

    spooled = _ledger(ItemStatus.FAILED)
    report_stream.start(spooled)
    report_stream.archive_all(spooled)
    try:
        state = cli._RunState(spooled)  # ruff: ignore[private-member-access] - interruption state receipt.
        assert (
            cli._cancelled(  # ruff: ignore[private-member-access] - spooled JSON cancellation channel.
                ["--output-format=json"],
                state,
                CancellationSignal(2, "SIGINT"),
            )
            == 130
        )
        cancelled = json.loads(capsys.readouterr().out)
        assert cancelled["exit_code"] == 130
        assert cancelled["mode"] == "apply"
        assert cancelled["interrupted"] is True
        assert cancelled["interruption"] == {
            "signal": "SIGINT",
            "reason": "interrupted by SIGINT",
            "phase": "report",
        }
    finally:
        report_stream.close(spooled)
