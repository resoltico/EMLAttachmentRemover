"""Single-file results are detached, bounded, evidence complete, and resource owned."""

from __future__ import annotations

import hashlib
import json
import tempfile
from email import policy
from email.parser import BytesParser
from typing import TYPE_CHECKING

import pytest

from eml_attachment_remover import (
    batch,
    process_file,
    report_spool,
    report_stream,
    reporting_v3,
)
from eml_attachment_remover.domain import AppError, BatchLedger, ExitCode, ItemStatus
from eml_attachment_remover.native_paths import path_value
from tests.live_report_support import MESSAGE

if TYPE_CHECKING:
    from pathlib import Path


def test_completed_api_snapshot_retains_its_committed_facts(tmp_path: Path) -> None:
    source = tmp_path / "public.eml"
    source.write_bytes(MESSAGE)
    item = process_file(str(source)).items[0]
    receipt = item.publication
    assert receipt is not None
    item.error = AppError(ExitCode.INTERNAL_ERROR, "uncommitted mutable detail")
    item.publication = None
    view = item.completed_view()
    assert view.status is ItemStatus.CREATED
    assert view.error is None
    assert view.publication == receipt


def test_retained_evidence_rejects_multiple_inputs_as_usage_before_processing() -> None:
    sources = ["first.eml", "second.eml"]
    ledger = BatchLedger.from_requests([path_value(source) for source in sources])
    ledger.retain_evidence = True
    with pytest.raises(AppError) as caught:
        batch.execute(
            sources,
            batch.BatchOptions(
                dry_run=False,
                existing="error",
                fail_fast=False,
                output=None,
                output_dir=None,
            ),
            retain_evidence=True,
            ledger=ledger,
        )
    assert caught.value.code is ExitCode.USAGE
    assert caught.value.message == "retained evidence requires exactly one input"
    assert all(item.status is None for item in ledger.items)


@pytest.mark.parametrize("kind", ["apply", "dry-run", "failed", "existing"])
def test_repeated_api_calls_leave_no_report_storage_and_keep_evidence(
    tmp_path: Path,
    monkeypatch: pytest.MonkeyPatch,
    kind: str,
) -> None:
    storage = tmp_path / "private-reports"
    storage.mkdir()
    monkeypatch.setattr(tempfile, "tempdir", str(storage))
    source = tmp_path / "source.eml"
    source.write_bytes(
        MESSAGE if kind != "failed" else b"Content-Type: unsupported/type\r\n\r\nbody"
    )
    body = BytesParser(policy=policy.default).parsebytes(MESSAGE).get_body()
    assert body is not None
    expected_body_hash = hashlib.sha256(body.get_payload(decode=True)).hexdigest()  # type: ignore[arg-type]
    for index in range(20):
        destination = tmp_path / f"copy-{index}.eml"
        if kind == "existing":
            process_file(str(source), str(destination))
        ledger = process_file(
            str(source),
            str(destination),
            dry_run=kind == "dry-run",
            existing="verify" if kind == "existing" else "error",
        )
        assert ledger.report_spool is ledger.emergency_report_spool is None
        assert not list(storage.iterdir())
        item = ledger.items[0]
        if kind == "failed":
            assert item.status is ItemStatus.FAILED
            assert item.error is not None
            assert not destination.exists()
            continue
        assert item.transformation is not None
        assert item.transformation.candidate == b""
        assert item.transformation.retained[0].decoded_sha256 == expected_body_hash
        assert item.transformation.removals[0].path == (1,)
        assert item.verification is not None
        assert item.verification.digest_matches
        document = reporting_v3.report(
            ledger, "dry-run" if kind == "dry-run" else "apply", 0
        )
        record = document["items"][0]  # type: ignore[index]
        assert (
            record["transformation"]["candidate_sha256"]
            == item.transformation.candidate_sha256
        )
        if kind == "dry-run":
            assert item.status is ItemStatus.WOULD_CREATE
            assert not destination.exists()
        else:
            assert item.status is (
                ItemStatus.EXISTING_VERIFIED
                if kind == "existing"
                else ItemStatus.CREATED
            )
            assert item.publication is not None
            assert (
                hashlib.sha256(destination.read_bytes()).hexdigest()
                == item.transformation.candidate_sha256
            )
    assert source.read_bytes() == (
        MESSAGE if kind != "failed" else b"Content-Type: unsupported/type\r\n\r\nbody"
    )


def test_unexpected_api_failure_closes_storage_on_unwind(
    tmp_path: Path,
    monkeypatch: pytest.MonkeyPatch,
) -> None:
    storage = tmp_path / "private-reports"
    storage.mkdir()
    monkeypatch.setattr(tempfile, "tempdir", str(storage))
    source = tmp_path / "source.eml"
    source.write_bytes(MESSAGE)

    def fail(*_args: object) -> None:
        message = "public candidate failure"
        raise MemoryError(message)

    monkeypatch.setattr(batch, "_candidate", fail)
    with pytest.raises(MemoryError, match="public candidate failure"):
        process_file(str(source))
    assert not list(storage.iterdir())
    assert source.read_bytes() == MESSAGE


def test_api_preserves_prepublication_evidence_limit(
    tmp_path: Path,
    monkeypatch: pytest.MonkeyPatch,
) -> None:
    source = tmp_path / "source.eml"
    source.write_bytes(MESSAGE)
    baseline = process_file(str(source), dry_run=True)
    complete_record = json.dumps(
        reporting_v3.item_json(baseline.items[0]), ensure_ascii=True
    )
    # Size the test budget from public evidence so long persistent test roots
    # do not accidentally exhaust the initial minimal request receipt.
    monkeypatch.setattr(
        report_spool, "MAX_RECORD_BYTES", len(complete_record.encode("ascii"))
    )
    ledger = process_file(str(source))
    assert ledger.items[0].status is ItemStatus.FAILED
    assert ledger.items[0].error is not None
    assert "per-item report record limit" in ledger.items[0].error.message
    assert not list(tmp_path.glob("*.mime-pruned.eml"))
    assert ledger.report_spool is ledger.emergency_report_spool is None


def test_single_file_result_is_closed_even_if_consumer_serialization_fails(
    tmp_path: Path,
) -> None:
    source = tmp_path / "source.eml"
    source.write_bytes(MESSAGE)
    ledger = process_file(str(source), dry_run=True)
    assert ledger.report_spool is ledger.emergency_report_spool is None
    report_stream.close(ledger)
    assert ledger.items[0].transformation is not None


def test_retained_evidence_cannot_materialize_an_unbounded_batch() -> None:
    options = batch.BatchOptions(
        dry_run=True, existing="error", fail_fast=False, output=None, output_dir=None
    )
    with pytest.raises(AppError, match="exactly one input"):
        batch.execute(["one.eml", "two.eml"], options, retain_evidence=True)
