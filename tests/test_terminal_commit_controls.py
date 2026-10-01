"""Coherent snapshots reject unfinished rows and preserve real publication errors."""

from __future__ import annotations

import json
import signal
from typing import TYPE_CHECKING

import pytest

from eml_attachment_remover import cli, report_stream, staged_output
from eml_attachment_remover.domain import BatchLedger, ItemStatus
from eml_attachment_remover.native_values import path_value
from tests.live_report_support import MESSAGE, inputs

if TYPE_CHECKING:
    from pathlib import Path

    from eml_attachment_remover.native_paths import BoundDirectoryHandle


def test_unfinished_status_is_rejected_before_external_output(
    capsys: pytest.CaptureFixture[str],
) -> None:
    ledger = BatchLedger.from_requests([path_value("public.eml")])
    ledger.items[0].status = ItemStatus.CREATED
    with pytest.raises(RuntimeError, match="before ledger terminalization"):
        report_stream.write_json(ledger, "create", 0)
    assert not capsys.readouterr().out


def test_terminal_snapshot_survives_incomplete_mirror_fields() -> None:
    item = BatchLedger.from_requests([path_value("public.eml")]).items[0]
    assert item.completed_view() is item
    item.finish(ItemStatus.NOT_RUN)
    item.status = None
    item.terminalized = False
    snapshot = item.completed_view()
    assert snapshot.status is ItemStatus.NOT_RUN
    assert snapshot.terminalized


@pytest.mark.parametrize("fmt", ["json", "human", "paths0"])
def test_signal_does_not_upgrade_a_real_postpublication_failure(
    tmp_path: Path,
    monkeypatch: pytest.MonkeyPatch,
    capsys: pytest.CaptureFixture[str],
    fmt: str,
) -> None:
    sources = inputs(tmp_path)

    def fail_sync(_parent: BoundDirectoryHandle) -> str:
        signal.raise_signal(signal.SIGINT)
        message = "public sync failure"
        raise OSError(message)

    monkeypatch.setattr(staged_output, "sync_bound_directory", fail_sync)
    assert cli.main(["--output-format", fmt, *sources]) == 130
    captured = capsys.readouterr()
    copies = list(tmp_path.glob("*.mime-pruned.eml"))
    assert len(copies) == 1
    assert all(
        (tmp_path / source.rsplit("/", 1)[-1]).read_bytes() == MESSAGE
        for source in sources
    )
    assert b"PUBLIC-ATTACHMENT" not in copies[0].read_bytes()
    if fmt == "json":
        report = json.loads(captured.out)
        item = report["items"][0]
        assert item["status"] == "published_with_error"
        assert item["terminalized"]
        assert item["publication"]["visibility"] == "visible"
        assert item["publication"]["directory_sync"] == "failed"
        assert report["summary"]["created"] == 0
    else:
        assert captured.err.count("Interrupted:") == 1
        if fmt == "paths0":
            assert not captured.out
        else:
            assert "published_with_error:" in captured.out
