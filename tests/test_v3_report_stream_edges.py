"""Private spool and streamed report failure-boundary contracts."""

from __future__ import annotations

import json
from pathlib import Path
from typing import cast

import pytest

from eml_attachment_remover import cli, report_spool, report_stream
from eml_attachment_remover.batch import BatchOptions, execute
from eml_attachment_remover.cancellation import CancellationSignal
from eml_attachment_remover.domain import (
    AppError,
    BatchLedger,
    ExitCode,
    ItemStatus,
    LedgerItem,
)
from eml_attachment_remover.native_paths import path_value


def _ledger() -> BatchLedger:
    ledger = BatchLedger.from_requests([path_value("one.eml")])
    ledger.items[0].finish(ItemStatus.FAILED, AppError(ExitCode.PARSE_ERROR, "bad"))
    return ledger


def test_spool_rejects_unsafe_records_and_nonprogress_writes(
    monkeypatch: pytest.MonkeyPatch,
) -> None:
    """Unsafe framing and partial writes never become accepted terminal records."""
    spool = report_spool.ReportSpool.create()
    try:
        for record in (
            b"",
            b'{"index":0}\n',
            b"x" * (report_spool.MAX_RECORD_BYTES + 1),
        ):
            with pytest.raises(report_spool.ReportSpoolError) as unsafe:
                spool.append(record)
            assert str(unsafe.value) == "terminal report record is unsafe"
        spool.bytes_written = report_spool.MAX_SPOOL_BYTES - len(b"{}\n")
        spool.append(b"{}")
        spool.bytes_written = report_spool.MAX_SPOOL_BYTES - len(b"{}")
        with pytest.raises(report_spool.ReportSpoolError) as full:
            spool.append(b"{}")
        assert str(full.value) == "terminal report spool exceeds its bounded capacity"
        monkeypatch.setattr(report_spool.__dict__["os"], "write", lambda *_args: 0)
        with pytest.raises(report_spool.ReportSpoolError):
            spool.append(b'{"index":0}')
    finally:
        spool.close()


def test_spool_creation_and_append_request_the_exact_private_native_flags(
    monkeypatch: pytest.MonkeyPatch,
    tmp_path: Path,
) -> None:
    """Private spool ownership uses its stable prefix and every available flag."""
    descriptor = 71
    created: dict[str, object] = {}
    opened: dict[str, object] = {}
    monkeypatch.setattr(report_spool, "private_temp_root", lambda: tmp_path)
    monkeypatch.setattr(
        report_spool.tempfile,
        "mkstemp",
        lambda **keywords: created.update(keywords)
        or (descriptor, str(tmp_path / "private")),
    )
    monkeypatch.setattr(report_spool.os, "fchmod", lambda *_args: None)
    monkeypatch.setattr(report_spool.os, "close", lambda _descriptor: None)
    spool = report_spool.ReportSpool.create()
    assert spool.path == tmp_path / "private"
    assert created == {
        "prefix": ".eml-attachment-remover-report-",
        "dir": tmp_path,
    }
    assert spool.closed is False
    assert spool.bytes_written == spool.record_count == 0
    original_open = report_spool.os.open
    monkeypatch.setattr(report_spool.os, "O_BINARY", 0x40, raising=False)
    monkeypatch.setattr(report_spool.os, "O_CLOEXEC", 0x80, raising=False)
    monkeypatch.setattr(
        report_spool.os,
        "open",
        lambda path, flags: opened.update(path=path, flags=flags) or descriptor,
    )
    monkeypatch.setattr(report_spool, "_write_all", lambda *_args: None)
    spool.append(b"{}")
    assert opened == {
        "path": spool.path,
        "flags": report_spool.os.O_WRONLY
        | report_spool.os.O_APPEND
        | 0x40
        | 0x80,
    }
    monkeypatch.setattr(report_spool.os, "open", original_open)


def test_spool_detects_unavailable_corrupt_and_incomplete_cleanup(
    monkeypatch: pytest.MonkeyPatch,
) -> None:
    """Unreadable, malformed, and undeleted owned spools fail closed."""
    unavailable = report_spool.ReportSpool.create()
    unavailable.close()
    unavailable.close()
    with pytest.raises(report_spool.ReportSpoolError):
        tuple(unavailable.records())

    missing = report_spool.ReportSpool.create()
    missing.path.unlink()
    with pytest.raises(report_spool.ReportSpoolError) as absent:
        tuple(missing.records())
    assert str(absent.value) == "terminal report spool is unavailable"
    missing.closed = True

    corrupt = report_spool.ReportSpool.create()
    try:
        corrupt.path.write_bytes(b"not-json-without-newline")
        with pytest.raises(report_spool.ReportSpoolError):
            tuple(corrupt.records())
    finally:
        corrupt.close()

    unsafe = report_spool.ReportSpool.create()
    unsafe.path.unlink()
    with pytest.raises(report_spool.ReportSpoolError):
        unsafe.close()
    unsafe.closed = True

    incomplete = report_spool.ReportSpool.create()
    try:
        with monkeypatch.context() as context:
            context.setattr(Path, "unlink", lambda _path: None)
            with pytest.raises(report_spool.ReportSpoolError):
                incomplete.close()
        incomplete.path.unlink()
        incomplete.closed = True
    finally:
        if incomplete.path.exists():
            incomplete.path.unlink()


def test_spool_detects_record_accounting_drift() -> None:
    """A valid line cannot conceal count or byte-accounting disagreement."""
    spool = report_spool.ReportSpool.create()
    try:
        spool.append(b'{"index":0}')
        spool.record_count = 2
        with pytest.raises(report_spool.ReportSpoolError):
            tuple(spool.records())
    finally:
        spool.close()


def test_report_stream_rejects_invalid_archive_and_record_owners() -> None:
    """Only terminal items in a project-owned spool are archivable or readable."""
    ledger = _ledger()
    report_stream.archive(ledger, ledger.items[0])
    ledger.report_spool = object()
    with pytest.raises(report_spool.ReportSpoolError):
        report_stream.archive(ledger, ledger.items[0])
    with pytest.raises(report_spool.ReportSpoolError):
        tuple(report_stream._records(ledger))  # ruff: ignore[private-member-access] - spool owner boundary.


def test_report_stream_rejects_corrupt_unordered_and_incomplete_records() -> None:
    """Report rendering fails closed for every malformed terminal-record sequence."""
    ledger = _ledger()
    report_stream.start(ledger)
    spool = cast("report_spool.ReportSpool", ledger.report_spool)
    try:
        spool.append(b"not-json")
        ledger.items[0].archived = True
        with pytest.raises(report_spool.ReportSpoolError):
            tuple(report_stream._records(ledger))  # ruff: ignore[private-member-access] - malformed JSON receipt.
    finally:
        report_stream.close(ledger)

    ledger = _ledger()
    report_stream.start(ledger)
    spool = cast("report_spool.ReportSpool", ledger.report_spool)
    try:
        spool.append(b'{"index":1}')
        ledger.items[0].archived = True
        with pytest.raises(report_spool.ReportSpoolError):
            tuple(report_stream._records(ledger))  # ruff: ignore[private-member-access] - input-order receipt.
    finally:
        report_stream.close(ledger)

    ledger = BatchLedger.from_requests([path_value("one.eml"), path_value("two.eml")])
    for item in ledger.items:
        item.finish(ItemStatus.FAILED, AppError(ExitCode.PARSE_ERROR, "bad"))
    report_stream.start(ledger)
    try:
        report_stream.archive(ledger, ledger.items[0])
        with pytest.raises(report_spool.ReportSpoolError):
            tuple(report_stream._records(ledger))  # ruff: ignore[private-member-access] - complete ledger receipt.
    finally:
        report_stream.close(ledger)


def test_report_stream_checks_terminal_summary_and_renders_each_channel(
    capsys: pytest.CaptureFixture[str],
) -> None:
    """Streaming JSON, human, and paths0 channels use one ordered spooled record."""
    unfinished = BatchLedger.from_requests([path_value("one.eml")])
    with pytest.raises(RuntimeError):
        report_stream._summary(unfinished)  # ruff: ignore[private-member-access] - terminal-count boundary.

    ledger = _ledger()
    report_stream.start(ledger)
    try:
        report_stream.archive_all(ledger)
        report_stream.write_json(ledger, "apply", 5)
        document = json.loads(capsys.readouterr().out)
        assert document["items"][0]["error"]["code"] == "PARSE_ERROR"
        report_stream.write_human(ledger)
        rendered = capsys.readouterr()
        assert rendered.out == "failed: one.eml\n"
        assert rendered.err == "one.eml: PARSE_ERROR: bad\n"
    finally:
        report_stream.close(ledger)


def test_report_stream_handles_direct_records_and_nonpath_diagnostics(
    capfd: pytest.CaptureFixture[str],
) -> None:
    """Direct ledgers preserve nonpath errors while invalid warnings are ignored."""
    ledger = _ledger()
    record = next(iter(report_stream._records(ledger)))  # ruff: ignore[private-member-access] - direct ledger fallback.
    assert record["status"] == "failed"
    report_stream.write_paths0(ledger)
    _output, errors = capfd.readouterr()
    assert errors == "one.eml: PARSE_ERROR: bad\n"
    report_stream._write_record_diagnostics(  # ruff: ignore[private-member-access] - malformed warning display boundary.
        {"source_request": None, "error": None, "warnings": ["not-a-warning"]}
    )
    report_stream._write_record_diagnostics(  # ruff: ignore[private-member-access] - absent warning boundary.
        {"source_request": None, "error": None, "warnings": None}
    )


def test_cancelled_cli_uses_the_streamed_channels_when_a_spool_is_active(
    monkeypatch: pytest.MonkeyPatch,
) -> None:
    """Cancellation cannot silently fall back to a full in-memory report document."""
    calls: list[str] = []
    original_close = report_stream.close
    monkeypatch.setattr(
        report_stream, "write_json", lambda *_args: calls.append("json")
    )
    monkeypatch.setattr(
        report_stream, "write_human", lambda *_args: calls.append("human")
    )

    def close(ledger: BatchLedger) -> None:
        calls.append("close")
        original_close(ledger)

    monkeypatch.setattr(report_stream, "close", close)
    for raw, expected in ((["--output-format=json"], "json"), ([], "human")):
        ledger = _ledger()
        report_stream.start(ledger)
        report_stream.archive_all(ledger)
        state = cli._RunState(ledger)  # ruff: ignore[private-member-access] - cancellation state boundary.
        assert (
            cli._cancelled(  # ruff: ignore[private-member-access] - spooled cancellation renderer.
                raw, state, CancellationSignal(2, "SIGINT")
            )
            == 130
        )
        assert calls[-2:] == [expected, "close"]


def test_primary_spool_fault_uses_reserved_complete_status_report(
    monkeypatch: pytest.MonkeyPatch, tmp_path: Path, capsys: pytest.CaptureFixture[str]
) -> None:
    """A post-publication archive fault reports visibility truth without a crash."""
    source = tmp_path / "source.eml"
    later = tmp_path / "later.eml"
    raw = b"Content-Type: text/plain\r\n\r\nbody\r\n"
    source.write_bytes(raw)
    later.write_bytes(raw)

    def exhausted(_ledger: BatchLedger, _item: object) -> None:
        message = "synthetic primary spool exhaustion"
        raise report_spool.ReportSpoolError(message)

    monkeypatch.setattr(report_stream, "archive", exhausted)
    ledger = execute(
        [str(source), str(later)],
        BatchOptions(
            dry_run=False,
            existing="error",
            fail_fast=False,
            output=None,
            output_dir=None,
        ),
    )
    try:
        assert ledger.report_spool_failed
        assert ledger.batch_error is not None
        assert [item.status for item in ledger.items] == [
            ItemStatus.PUBLISHED_WITH_ERROR,
            ItemStatus.NOT_RUN,
        ]
        report_stream.write_json(ledger, "apply", 7)
        document = json.loads(capsys.readouterr().out)
        assert document["ok"] is False
        assert document["batch_error"]["code"] == "WRITE_ERROR"
        assert [item["status"] for item in document["items"]] == [
            "published_with_error",
            "not_run",
        ]
    finally:
        report_stream.close(ledger)


def test_corrupt_primary_spool_is_detected_before_json_output(
    capsys: pytest.CaptureFixture[str],
) -> None:
    """A corrupt committed receipt cannot be mislabeled as a complete document."""
    ledger = _ledger()
    report_stream.start(ledger)
    spool = cast("report_spool.ReportSpool", ledger.report_spool)
    try:
        report_stream.archive_all(ledger)
        spool.path.write_bytes(b"corrupt")
        with pytest.raises(report_spool.ReportSpoolError):
            report_stream.write_json(ledger, "apply", 70)
        assert not capsys.readouterr().out
    finally:
        report_stream.close(ledger)


def test_reservation_start_failure_is_terminal_before_any_publication(
    monkeypatch: pytest.MonkeyPatch,
) -> None:
    """Failure to reserve report capacity yields a typed no-publication ledger."""
    emergency = report_spool.ReportSpool.create()
    calls = 0

    def create() -> report_spool.ReportSpool:
        nonlocal calls
        calls += 1
        if calls == 1:
            return emergency
        message = "synthetic reservation failure"
        raise OSError(message)

    monkeypatch.setattr(report_spool.ReportSpool, "create", staticmethod(create))
    ledger = BatchLedger.from_requests([path_value("one.eml")])
    assert not report_stream.start_or_fail(ledger)
    assert emergency.closed
    assert ledger.batch_error is not None
    assert ledger.items[0].status is ItemStatus.NOT_RUN


def test_emergency_record_reader_rejects_every_untrusted_reservation_shape() -> None:
    """Recovery refuses an absent, corrupt, mismatched, or incomplete skeleton."""
    unavailable = _ledger()
    unavailable.report_spool_failed = True
    with pytest.raises(report_spool.ReportSpoolError, match="unavailable"):
        tuple(report_stream._records(unavailable))  # ruff: ignore[private-member-access] - emergency absence boundary.

    corrupt = _ledger()
    report_stream.start(corrupt)
    corrupt.report_spool_failed = True
    emergency = cast("report_spool.ReportSpool", corrupt.emergency_report_spool)
    try:
        emergency.path.write_bytes(b"not-json\n")
        with pytest.raises(report_spool.ReportSpoolError, match="corrupt"):
            tuple(report_stream._records(corrupt))  # ruff: ignore[private-member-access] - corrupt emergency JSON.
    finally:
        report_stream.close(corrupt)

    mismatched = _ledger()
    report_stream.start(mismatched)
    mismatched.report_spool_failed = True
    emergency = cast("report_spool.ReportSpool", mismatched.emergency_report_spool)
    try:
        emergency.path.write_bytes(b'{"index":1,"source_request":null}\n')
        with pytest.raises(report_spool.ReportSpoolError, match="corrupt"):
            tuple(report_stream._records(mismatched))  # ruff: ignore[private-member-access] - mismatched source reservation.
    finally:
        report_stream.close(mismatched)

    incomplete = _ledger()
    report_stream.start(incomplete)
    incomplete.report_spool_failed = True
    emergency = cast("report_spool.ReportSpool", incomplete.emergency_report_spool)
    try:
        emergency.record_count = 0
        with pytest.raises(report_spool.ReportSpoolError, match="incomplete"):
            tuple(report_stream._records(incomplete))  # ruff: ignore[private-member-access] - incomplete status skeleton.
    finally:
        emergency.record_count = 1
        report_stream.close(incomplete)


def test_report_stream_recovers_idempotently_and_attempts_every_owned_close(
    monkeypatch: pytest.MonkeyPatch,
) -> None:
    """Repeated recovery is stable and cleanup releases both owned spools."""
    ledger = _ledger()
    report_stream.start(ledger)
    primary = cast("report_spool.ReportSpool", ledger.report_spool)
    emergency = cast("report_spool.ReportSpool", ledger.emergency_report_spool)
    report_stream.recover(ledger)
    report_stream.recover(ledger)
    assert ledger.items[0].status is ItemStatus.FAILED

    calls: list[report_spool.ReportSpool] = []
    original_close = report_spool.ReportSpool.close

    def close(spool: report_spool.ReportSpool) -> None:
        calls.append(spool)
        if spool is primary:
            message = "synthetic primary cleanup failure"
            raise report_spool.ReportSpoolError(message)
        original_close(spool)

    monkeypatch.setattr(report_spool.ReportSpool, "close", close)
    with pytest.raises(report_spool.ReportSpoolError, match="synthetic primary"):
        report_stream.close(ledger)
    assert calls == [primary, emergency]
    primary.path.unlink()
    primary.closed = True


def test_terminal_receipt_reclassifies_for_report_failure() -> None:
    """Only a terminal created/existing receipt can be corrected for report loss."""
    item = LedgerItem(0, path_value("one.eml"))
    error = AppError(ExitCode.WRITE_ERROR, "report failure", phase="report")
    with pytest.raises(RuntimeError):
        item.correct_report_failure(error)
    item.finish(ItemStatus.EXISTING_VERIFIED)
    item.correct_report_failure(error)
    assert item.status is ItemStatus.FAILED
    assert item.error == error


def test_report_failure_never_erases_a_visible_publication_receipt() -> None:
    """A post-edge report failure retains the published-with-error outcome."""
    item = LedgerItem(0, path_value("one.eml"))
    item.finish(
        ItemStatus.PUBLISHED_WITH_ERROR,
        AppError(ExitCode.WRITE_ERROR, "post-publication sync failed"),
    )
    item.correct_report_failure(AppError(ExitCode.WRITE_ERROR, "report failure"))
    assert item.status is ItemStatus.PUBLISHED_WITH_ERROR
    assert item.error == AppError(ExitCode.WRITE_ERROR, "post-publication sync failed")
