"""Terminal evidence is coherent, and every report rejects incomplete rows."""

from __future__ import annotations

from typing import TYPE_CHECKING

import pytest

from eml_attachment_remover import report_document, report_spool, report_stream
from eml_attachment_remover.domain import (
    AppError,
    BatchLedger,
    ExitCode,
    ItemStatus,
    LedgerItem,
)
from eml_attachment_remover.native_paths import path_value
from eml_attachment_remover.processing import process_file
from tests.live_report_support import MESSAGE

if TYPE_CHECKING:
    from pathlib import Path


def test_report_failure_snapshot_preserves_a_real_publication(tmp_path: Path) -> None:
    source = tmp_path / "public.eml"
    source.write_bytes(MESSAGE)
    item = process_file(str(source)).items[0]
    receipt = item.publication
    assert receipt is not None
    error = AppError(ExitCode.WRITE_ERROR, "report failure", phase="report")
    item.correct_report_failure(error)
    view = item.completed_view()
    assert view is not item
    assert view.status is ItemStatus.PUBLISHED_WITH_ERROR
    assert view.error == error
    assert view.publication == receipt
    assert source.read_bytes() == MESSAGE
    assert (tmp_path / "public.mime-pruned.eml").is_file()


def test_spool_append_rejects_an_independently_closed_handle() -> None:
    spool = report_spool.ReportSpool.create()
    try:
        spool.file.close()
        with pytest.raises(report_spool.ReportSpoolError, match="record is unsafe"):
            spool.append(b'{"index":0}')
        assert spool.record_count == 0
        assert spool.bytes_written == 0
    finally:
        spool.close()


@pytest.mark.parametrize(
    ("status", "terminalized"), [(None, True), (ItemStatus.FAILED, False)]
)
def test_every_report_entrypoint_rejects_partially_terminal_rows(
    capsys: pytest.CaptureFixture[str], status: ItemStatus | None, *, terminalized: bool
) -> None:
    item = LedgerItem(
        0, path_value("source.eml"), status=status, terminalized=terminalized
    )
    ledger = BatchLedger([item])
    message = r"^report requested before ledger terminalization$"
    with pytest.raises(RuntimeError, match=message):
        report_document.report(ledger, "apply", 0)
    with pytest.raises(RuntimeError, match=message):
        report_stream._summary(ledger)  # ruff: ignore[private-member-access] - shared terminal admission.
    with pytest.raises(RuntimeError, match=message):
        report_stream.write_human(ledger)
    assert not capsys.readouterr().out
