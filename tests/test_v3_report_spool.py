"""Bounded private terminal-record spool contracts."""

from __future__ import annotations

import gc
import json
import os
import tempfile
import tracemalloc
from pathlib import Path
from typing import cast

import pytest

from eml_attachment_remover import report_spool, report_stream
from eml_attachment_remover.domain import (
    BatchLedger,
    FileIdentity,
    ItemStatus,
    SourceSnapshot,
    TransformationPlan,
)
from eml_attachment_remover.native_paths import path_value


def test_private_spool_preserves_order_and_removes_only_its_owned_path() -> None:
    """Terminal records use a mode-0600 external file and exact owned cleanup."""
    spool = report_spool.ReportSpool.create()
    try:
        assert spool.path.stat().st_mode & 0o777 == 0o600
        spool.append(b'{"index":0}')
        spool.append(b'{"index":1}')
        assert tuple(spool.records()) == (b'{"index":0}', b'{"index":1}')
    finally:
        spool.close()
    assert not spool.path.exists()


def test_environment_temp_root_inside_the_repository_is_rejected(
    monkeypatch: pytest.MonkeyPatch,
) -> None:
    """TMPDIR cannot redirect private report receipts into public project storage."""
    project = Path(__file__).resolve().parents[1]
    monkeypatch.setattr(tempfile, "gettempdir", lambda: str(project))
    with pytest.raises(report_spool.ReportSpoolError) as rejected:
        report_spool.ReportSpool.create()
    assert str(rejected.value) == "private terminal report root is unsafe"


def test_environment_temp_root_symlink_is_rejected_before_resolution(
    tmp_path: Path, monkeypatch: pytest.MonkeyPatch
) -> None:
    """A symlinked TMPDIR cannot conceal the terminal report storage boundary."""
    target = tmp_path / "actual-temp"
    target.mkdir()
    selected = tmp_path / "selected-temp"
    selected.symlink_to(target, target_is_directory=True)
    monkeypatch.setattr(tempfile, "gettempdir", lambda: str(selected))
    with pytest.raises(report_spool.ReportSpoolError) as rejected:
        report_spool.private_temp_root()
    assert str(rejected.value) == "private terminal report root is unsafe"


def test_spool_rejects_oversized_partial_and_corrupt_records(
    monkeypatch: pytest.MonkeyPatch,
) -> None:
    """Capacity and framing faults fail closed before report publication."""
    spool = report_spool.ReportSpool.create()
    try:
        monkeypatch.setattr(report_spool, "MAX_SPOOL_BYTES", 8)
        with pytest.raises(report_spool.ReportSpoolError):
            spool.append(b"12345678")
        monkeypatch.setattr(report_spool, "MAX_SPOOL_BYTES", 64 * 1024 * 1024)
        spool.append(b'{"index":0}')
        spool.path.write_bytes(b'{"index":0}')
        with pytest.raises(report_spool.ReportSpoolError):
            tuple(spool.records())
    finally:
        spool.close()


def test_spool_recovers_an_interrupted_append_without_losing_prior_records(
    monkeypatch: pytest.MonkeyPatch,
) -> None:
    """A failed append truncates its partial bytes back to the last receipt."""
    spool = report_spool.ReportSpool.create()
    original_write = os.write
    calls = 0

    def interrupted(descriptor: int, payload: bytes) -> int:
        nonlocal calls
        calls += 1
        if calls == 1:
            return original_write(descriptor, payload[:1])
        message = "synthetic full spool"
        raise OSError(message)

    try:
        spool.append(b'{"index":0}')
        monkeypatch.setattr(os, "write", interrupted)
        with pytest.raises(report_spool.ReportSpoolError):
            spool.append(b'{"index":1}')
        assert tuple(spool.records()) == (b'{"index":0}',)
        assert spool.record_count == 1
    finally:
        spool.close()


def test_spool_reports_a_failed_partial_append_recovery(
    monkeypatch: pytest.MonkeyPatch,
) -> None:
    """A partial record that cannot be truncated is never accepted as a receipt."""
    spool = report_spool.ReportSpool.create()
    try:
        with monkeypatch.context() as context:

            def fail_write(_descriptor: int, _payload: bytes) -> int:
                message = "synthetic write failure"
                raise OSError(message)

            def fail_truncate(_descriptor: int, _size: int) -> None:
                message = "synthetic truncate failure"
                raise OSError(message)

            context.setattr(os, "write", fail_write)
            context.setattr(os, "ftruncate", fail_truncate)
            with pytest.raises(
                report_spool.ReportSpoolError,
                match="terminal report spool could not recover a partial record",
            ):
                spool.append(b'{"index":0}')
    finally:
        spool.close()


def test_write_all_requires_exact_forward_progress_and_a_finite_completion(
    monkeypatch: pytest.MonkeyPatch,
) -> None:
    """Every partial write advances exactly once; a stalled sequence is bounded."""
    requests: list[bytes] = []
    chunks = iter((1, 2))

    def write(_descriptor: int, payload: bytes) -> int:
        requests.append(payload)
        return next(chunks)

    monkeypatch.setattr(os, "write", write)
    report_spool._write_all(9, b"")  # ruff: ignore[private-member-access] - empty private write is inert.
    assert requests == []
    report_spool._write_all(9, b"abc")  # ruff: ignore[private-member-access] - exact progress loop.
    assert requests == [b"abc", b"bc"]

    monkeypatch.setattr(os, "write", lambda _fd, payload: len(payload) + 1)
    with pytest.raises(report_spool.ReportSpoolError) as oversized:
        report_spool._write_all(9, b"a")  # ruff: ignore[private-member-access] - oversized progress rejection.
    assert str(oversized.value) == "terminal report spool write made no progress"


def test_reserved_emergency_status_spool_exists_before_any_item_is_terminal() -> None:
    """Every requested source has durable, private status capacity before work."""
    ledger = BatchLedger.from_requests([path_value("one.eml"), path_value("two.eml")])
    report_stream.start(ledger)
    try:
        emergency = cast("report_spool.ReportSpool", ledger.emergency_report_spool)
        assert emergency.path.stat().st_mode & 0o777 == 0o600
        assert emergency.record_count == 2
        assert [
            record["index"]
            for record in (json.loads(raw) for raw in emergency.records())
        ] == [0, 1]
    finally:
        report_stream.close(ledger)


def test_archiving_releases_terminal_fingerprints_but_preserves_schema_record() -> None:
    """A completed item releases its transformation aggregate from the ledger row."""
    ledger = BatchLedger.from_requests([path_value("source.eml")])
    item = ledger.items[0]
    item.transformation = TransformationPlan((), (), (), "a" * 64, 1, b"x")
    item.warnings.append({"code": "WARN", "message": "synthetic"})
    item.finish(ItemStatus.WOULD_CREATE)
    report_stream.start(ledger)
    try:
        report_stream.archive(ledger, item)
        assert item.archived
        transformation = cast("object", item.transformation)
        assert transformation is None
        assert item.warnings == []
        records = tuple(report_stream._records(ledger))  # ruff: ignore[private-member-access] - spool/reporter boundary.
        assert records[0]["transformation"] is not None
        assert records[0]["warnings"] == [{"code": "WARN", "message": "synthetic"}]
    finally:
        report_stream.close(ledger)


def _terminal_peak(item_count: int) -> tuple[int, int]:
    """Measure sequential multi-MiB terminalization without retaining active bytes.

    Returns:
        Current and peak traced Python allocation totals in bytes.

    """
    source = path_value("source.eml")
    ledger = BatchLedger.from_requests([source for _ in range(item_count)])
    report_stream.start(ledger)
    tracemalloc.start()
    try:
        for index, item in enumerate(ledger.items):
            payload = bytes([index]) * (1024 * 1024)
            snapshot = SourceSnapshot(
                source,
                source,
                source,
                b"source.eml",
                source,
                FileIdentity(1, index, "-rw-------", 3),
                0o600,
                payload,
                "a" * 64,
                len(payload),
            )
            item.source = snapshot
            item.transformation = TransformationPlan(
                (), (), (), "a" * 64, len(payload), payload
            )
            item.finish(ItemStatus.WOULD_CREATE)
            report_stream.archive_all(ledger)
            del payload, snapshot
        gc.collect()
        current, peak = tracemalloc.get_traced_memory()
        assert all(
            item.transformation is None and item.warnings == [] for item in ledger.items
        )
        return current, peak
    finally:
        tracemalloc.stop()
        report_stream.close(ledger)


def test_terminal_spooling_has_bounded_multi_mib_peak_memory() -> None:
    """Eight sequential terminal payloads do not retain eight active byte pairs."""
    one_current, one_peak = _terminal_peak(1)
    eight_current, eight_peak = _terminal_peak(8)
    tolerance = 512 * 1024
    assert eight_current <= one_current + tolerance
    assert eight_peak <= one_peak + tolerance
