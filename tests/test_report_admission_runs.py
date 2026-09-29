"""End to end: refused before any copy, fully reported when admitted."""

from __future__ import annotations

import json
from typing import TYPE_CHECKING

from eml_attachment_remover import (
    cli,
    report_budget,
    report_spool,
    report_stream,
)
from eml_attachment_remover.batch import BatchOptions, execute
from eml_attachment_remover.domain import (
    AppError,
    BatchLedger,
    ExitCode,
    ItemStatus,
)
from eml_attachment_remover.native_paths import path_value

if TYPE_CHECKING:
    from pathlib import Path

    import pytest

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


def test_oversized_message_is_refused_before_any_copy_exists(
    tmp_path: Path, monkeypatch: pytest.MonkeyPatch
) -> None:
    """A refused input publishes nothing and later inputs are unaffected."""
    # A small item's reserved terminal record is ~62 KiB plus a few KiB of evidence
    # on any host; 128 KiB clears that while 400 parts (~110 KiB more) exceed it.
    monkeypatch.setattr(report_spool, "MAX_RECORD_BYTES", 128 * 1024)
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
            ExitCode.PARSE_ERROR, report_budget.RECORD_LIMIT_MESSAGE
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
    assert item["error"]["message"] == report_budget.RECORD_LIMIT_MESSAGE
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


def test_a_batch_whose_minimal_records_cannot_fit_falls_back_to_the_reserved_report(
    tmp_path: Path, monkeypatch: pytest.MonkeyPatch, capsys: pytest.CaptureFixture[str]
) -> None:
    """No plan means no work: a complete status report, never a partial batch."""
    source = tmp_path / "a.eml"
    source.write_bytes(_parts(1))
    probe = BatchLedger.from_requests([path_value(str(source))])
    minimal = _canonical(report_budget.minimal_record(probe.items[0], worst=True))
    # The reservation is a subset of the minimal record, so it still fits.
    monkeypatch.setattr(report_spool, "MAX_SPOOL_BYTES", minimal)
    status = cli.main(["--output-format", "json", "--", str(source)])
    document = json.loads(capsys.readouterr().out)
    assert (status, document["exit_code"]) == (int(ExitCode.WRITE_ERROR),) * 2
    assert document["batch_error"]["message"] == "terminal report spool failed"
    assert [item["status"] for item in document["items"]] == ["not_run"]
    assert list(tmp_path.iterdir()) == [source]
