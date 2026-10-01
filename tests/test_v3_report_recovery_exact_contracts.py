"""Exact diagnostics and recovery contracts for terminal report evidence."""

from __future__ import annotations

import hashlib
import os

import pytest

from eml_attachment_remover import report_spool, report_stream, staged_output
from eml_attachment_remover.domain import (
    BatchLedger,
    BoundDestination,
    FileIdentity,
    ItemStatus,
    PathValue,
)
from eml_attachment_remover.native_paths import path_value
from tests.report_spool_support import append_data, replace_data


def _raise_os_error(message: str) -> None:
    raise OSError(message)


def test_unrecoverable_partial_record_has_an_exact_diagnostic_and_cause(
    monkeypatch: pytest.MonkeyPatch,
) -> None:
    """A failed truncation names the lost recovery, not merely some spool fault."""
    monkeypatch.setattr(
        os, "ftruncate", lambda _fd, _size: _raise_os_error("synthetic truncate")
    )
    with pytest.raises(report_spool.ReportSpoolError) as raised:
        report_spool._truncate_or_raise(9, 0)  # ruff: ignore[private-member-access] - exact recovery diagnostic.
    assert str(raised.value) == (
        "terminal report spool could not recover a partial record"
    )
    assert str(raised.value.__cause__) == "synthetic truncate"


def test_failed_write_has_an_exact_diagnostic_and_keeps_prior_records(
    monkeypatch: pytest.MonkeyPatch,
) -> None:
    """A rejected append reports the write failure and leaves earlier receipts."""
    spool = report_spool.ReportSpool.create()
    try:
        spool.append(b'{"index":0}')
        with monkeypatch.context() as context:
            context.setattr(
                os, "write", lambda _fd, _payload: _raise_os_error("synthetic write")
            )
            with pytest.raises(report_spool.ReportSpoolError) as raised:
                spool.append(b'{"index":1}')
        assert str(raised.value) == "terminal report spool write failed"
        assert str(raised.value.__cause__) == "synthetic write"
        assert tuple(spool.records()) == (b'{"index":0}',)
    finally:
        spool.close()


def test_unterminated_and_miscounted_spools_have_exact_diagnostics() -> None:
    """Framing and accounting corruption are distinguished precisely."""
    spool = report_spool.ReportSpool.create()
    try:
        spool.append(b'{"index":0}')
        append_data(spool, b'{"index":1}\n')
        with pytest.raises(report_spool.ReportSpoolError) as miscounted:
            tuple(spool.records())
        assert str(miscounted.value) == "terminal report spool accounting is corrupt"
        replace_data(spool, b'{"index":0}')
        with pytest.raises(report_spool.ReportSpoolError) as unterminated:
            tuple(spool.records())
        assert str(unterminated.value) == "terminal report spool is corrupt"
    finally:
        spool.close()


def test_cleanup_failures_have_exact_diagnostics() -> None:
    """A close failure is reported without substituting pathname cleanup."""
    spool = report_spool.ReportSpool.create()
    real = spool.file

    class Unclosable:
        @staticmethod
        def close() -> None:
            message = "public close failure"
            raise OSError(message)

    spool.file = Unclosable()  # type: ignore[assignment]
    try:
        with pytest.raises(report_spool.ReportSpoolError) as failed:
            spool.close()
        assert (
            str(failed.value)
            == "terminal report spool could not close its owned handle"
        )
        assert str(failed.value.__cause__) == "public close failure"
        assert not spool.closed
    finally:
        spool.file = real
        spool.close()


def test_emergency_status_rejects_a_reservation_longer_than_the_batch() -> None:
    """An extra reserved status record fails instead of being silently ignored."""
    ledger = BatchLedger.from_requests([path_value("source.eml")])
    ledger.items[0].finish(ItemStatus.WOULD_CREATE)
    report_stream.start(ledger)
    try:
        emergency = ledger.emergency_report_spool
        assert isinstance(emergency, report_spool.ReportSpool)
        append_data(emergency, b'{"index":1}\n')
        records = report_stream._emergency_records(ledger)  # ruff: ignore[private-member-access] - reservation length contract.
        with pytest.raises(ValueError, match=r"zip\(\) argument 2 is shorter"):
            list(records)
    finally:
        report_stream.close(ledger)


def test_reconciling_without_a_stage_reports_an_unproven_receipt() -> None:
    """A lifecycle that never created its stage has no identity to report."""
    value = PathValue("out", "out", "b3V0")
    destination = BoundDestination(
        value, value, b"out", FileIdentity(1, 2, "directory", 3)
    )
    state = staged_output._PublicationState(  # ruff: ignore[private-member-access] - unstaged reconciliation.
        destination, b"candidate", hashlib.sha256(b"candidate").hexdigest()
    )
    receipt = staged_output._reconcile(state, "failed")  # ruff: ignore[private-member-access] - unstaged reconciliation.
    assert receipt.visibility == "not_proven"
    assert receipt.identity is None
    assert receipt.directory_sync == "failed"
    assert receipt.temp_cleanup == "pending"
