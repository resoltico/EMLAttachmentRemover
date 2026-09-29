"""Report evidence is admitted before publication, never refused after it."""

from __future__ import annotations

import json
from typing import TYPE_CHECKING

import pytest

from eml_attachment_remover import (
    cli,
    report_admission,
    report_spool,
    report_stream,
    reporting_v3,
)
from eml_attachment_remover.batch import BatchOptions, execute
from eml_attachment_remover.domain import (
    AppError,
    BatchLedger,
    ExitCode,
    ItemStatus,
    LedgerItem,
    TransformationPlan,
)
from eml_attachment_remover.native_paths import path_value

if TYPE_CHECKING:
    from pathlib import Path

HEADERS = (
    b"From: a@example.test\r\nMIME-Version: 1.0\r\n"
    b"Content-Type: multipart/mixed; boundary=B\r\n\r\n"
)
ATTACHMENT = (
    b"--B\r\nContent-Type: application/pdf\r\nContent-Disposition: attachment\r\n"
    b"\r\nX\r\n--B--\r\n"
)
OPTIONS = BatchOptions(
    dry_run=False, existing="error", fail_fast=False, output=None, output_dir=None
)


def _parts(count: int) -> bytes:
    text = b"".join(
        b"--B\r\nContent-Type: text/plain\r\n\r\npart " + b"%d" % index + b"\r\n"
        for index in range(count)
    )
    return HEADERS + text + ATTACHMENT


def _canonical(value: object) -> int:
    """Measure a value's canonical JSON independently of the code under test.

    Returns:
        The number of ASCII bytes of the serialization.

    """
    return len(json.dumps(value, ensure_ascii=True, sort_keys=True, allow_nan=False))


def _spooled_item() -> tuple[BatchLedger, report_spool.ReportSpool]:
    ledger = BatchLedger.from_requests([path_value("source.eml")])
    report_stream.start(ledger)
    ledger.report_spool = report_spool.ReportSpool.create()
    return ledger, ledger.report_spool


@pytest.fixture
def stub_record(monkeypatch: pytest.MonkeyPatch) -> int:
    """Replace the item record with one of a known size.

    Returns:
        The size admission must compute for that record.

    """
    record = {"destination_request": {"text": "d" * 40}, "padding": "p" * 500}
    monkeypatch.setattr(reporting_v3, "item_json", lambda _item: record)
    return (
        _canonical(record)
        + report_admission.TERMINAL_RESERVE_BYTES
        + 2 * _canonical(record["destination_request"])
    )


def _detail(item: LedgerItem) -> tuple[object, list[dict[str, object]]]:
    return item.transformation, item.warnings


def _refused(ledger: BatchLedger) -> AppError:
    item = ledger.items[0]
    item.transformation = TransformationPlan((), (), (), "a" * 64, 1, b"x")
    item.warnings.append({"code": "W"})
    with pytest.raises(AppError) as raised:
        report_admission.admit(ledger, item)
    assert _detail(item) == (None, [])
    return raised.value


def test_admission_is_skipped_without_a_spool() -> None:
    """Without a private spool there is no per-record limit to protect."""
    ledger = BatchLedger.from_requests([path_value("source.eml")])
    item = ledger.items[0]
    item.transformation = TransformationPlan((), (), (), "a" * 64, 1, b"x")
    report_admission.admit(ledger, item)
    assert _detail(item)[0] is not None


def test_record_exactly_at_the_limit_is_admitted_and_one_over_is_refused(
    stub_record: int, monkeypatch: pytest.MonkeyPatch
) -> None:
    """The bound is inclusive and counts the terminal fields it must still hold."""
    ledger, _spool = _spooled_item()
    try:
        monkeypatch.setattr(report_spool, "MAX_RECORD_BYTES", stub_record)
        report_admission.admit(ledger, ledger.items[0])
        monkeypatch.setattr(report_spool, "MAX_RECORD_BYTES", stub_record - 1)
        error = _refused(ledger)
    finally:
        report_stream.close(ledger)
    assert error == AppError(
        ExitCode.PARSE_ERROR,
        "report evidence for this message exceeds the per-item report record limit",
    )


def test_cumulative_capacity_is_admitted_up_to_its_limit(
    stub_record: int, monkeypatch: pytest.MonkeyPatch
) -> None:
    """A record that would overflow the whole spool is refused beforehand."""
    ledger, spool = _spooled_item()
    try:
        monkeypatch.setattr(report_spool, "MAX_SPOOL_BYTES", 10 * stub_record)
        spool.bytes_written = 9 * stub_record
        report_admission.admit(ledger, ledger.items[0])
        spool.bytes_written += 1
        error = _refused(ledger)
    finally:
        report_stream.close(ledger)
    assert error == AppError(
        ExitCode.PARSE_ERROR, "report evidence exceeds the remaining report capacity"
    )


def test_the_record_limit_is_reported_before_the_capacity_limit(
    stub_record: int, monkeypatch: pytest.MonkeyPatch
) -> None:
    """When both would be exceeded, the record itself is what is too large."""
    ledger, spool = _spooled_item()
    try:
        monkeypatch.setattr(report_spool, "MAX_RECORD_BYTES", stub_record - 1)
        monkeypatch.setattr(report_spool, "MAX_SPOOL_BYTES", stub_record)
        spool.bytes_written = stub_record
        error = _refused(ledger)
    finally:
        report_stream.close(ledger)
    assert error.message == report_admission.RECORD_LIMIT_MESSAGE


def test_oversized_message_is_refused_before_any_copy_exists(
    tmp_path: Path, monkeypatch: pytest.MonkeyPatch
) -> None:
    """A refused input publishes nothing and later inputs are unaffected."""
    # A small item's record is a few KiB on POSIX and more on Windows (longer paths,
    # UTF-16 forms); 64 KiB clears every host while 400 parts (~110 KiB) exceed it.
    monkeypatch.setattr(report_spool, "MAX_RECORD_BYTES", 64 * 1024)
    big = tmp_path / "big.eml"
    small = tmp_path / "small.eml"
    big.write_bytes(_parts(400))
    small.write_bytes(_parts(1))
    ledger = execute([str(big), str(small)], OPTIONS)
    try:
        assert [item.status for item in ledger.items] == [
            ItemStatus.FAILED,
            ItemStatus.CREATED,
        ]
        assert ledger.items[0].error == AppError(
            ExitCode.PARSE_ERROR, report_admission.RECORD_LIMIT_MESSAGE
        )
        assert ledger.batch_error is None
        assert not ledger.report_spool_failed
    finally:
        report_stream.close(ledger)
    assert not (tmp_path / "big.mime-pruned.eml").exists()
    assert (tmp_path / "small.mime-pruned.eml").exists()


def test_audit_case_with_4000_retained_parts_fails_cleanly(
    tmp_path: Path, capsys: pytest.CaptureFixture[str]
) -> None:
    """The reported scenario: no copy, a precise failure, and a complete report."""
    source = tmp_path / "big.eml"
    source.write_bytes(_parts(4000))
    status = cli.main(["--output-format", "json", "--", str(source)])
    document = json.loads(capsys.readouterr().out)
    item = document["items"][0]
    assert (status, item["status"]) == (int(ExitCode.PARSE_ERROR), "failed")
    assert item["error"]["message"] == report_admission.RECORD_LIMIT_MESSAGE
    assert document["batch_error"] is None
    assert list(tmp_path.iterdir()) == [source]


def test_message_just_inside_the_limit_is_created_and_fully_reported(
    tmp_path: Path, capsys: pytest.CaptureFixture[str]
) -> None:
    """3,000 retained parts are admitted and every fingerprint is reported."""
    source = tmp_path / "large.eml"
    source.write_bytes(_parts(3000))
    status = cli.main(["--output-format", "json", "--", str(source)])
    item = json.loads(capsys.readouterr().out)["items"][0]
    assert (status, item["status"]) == (0, "created")
    assert len(item["transformation"]["retained"]) == 3000
    assert (tmp_path / "large.mime-pruned.eml").exists()
