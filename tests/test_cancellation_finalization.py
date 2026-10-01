"""Retired-controller requests remain accessible in API ledgers and CLI status."""

from __future__ import annotations

import hashlib
import json
import os
import signal
import threading
from email import policy
from email.parser import BytesParser
from typing import TYPE_CHECKING

import pytest

from eml_attachment_remover import cancellation, cli, staged_output
from eml_attachment_remover.cancellation_state import CURRENT
from eml_attachment_remover.domain import ItemPhase, ItemStatus
from eml_attachment_remover.processing import process_file
from tests.finalization_signal_support import interrupt_at
from tests.live_report_support import MESSAGE, inputs

if TYPE_CHECKING:
    from pathlib import Path

    from eml_attachment_remover.native_paths import BoundDirectoryHandle


@pytest.mark.parametrize("boundary", ["stop", "join", "reset", "processing_restore"])
def test_api_returns_completed_results_after_shutdown_interruption(
    tmp_path: Path, boundary: str
) -> None:
    source = tmp_path / "public.eml"
    source.write_bytes(MESSAGE)
    previous = signal.getsignal(signal.SIGINT)
    with interrupt_at(boundary) as sent:
        ledger = process_file(str(source))
    assert sent == [1]
    item = ledger.items[0]
    assert ledger.interruption is not None
    assert ledger.interruption.signal == "SIGINT"
    assert ledger.interruption.phase == ItemPhase.INVENTORIED.value
    assert item.status is ItemStatus.CREATED
    assert item.terminalized
    _verify_copy(tmp_path / "public.mime-pruned.eml", source)
    assert item.publication is not None
    assert item.publication.address_verified
    assert (
        item.publication.digest
        == hashlib.sha256(
            (tmp_path / "public.mime-pruned.eml").read_bytes()
        ).hexdigest()
    )
    assert CURRENT.get() is None
    assert signal.getsignal(signal.SIGINT) == previous
    assert not any(
        thread.name == "eml-cancellation-monitor" for thread in threading.enumerate()
    )
    following = tmp_path / "following.eml"
    following.write_bytes(MESSAGE)
    assert (
        process_file(str(following), dry_run=True).items[0].status
        is ItemStatus.WOULD_CREATE
    )


@pytest.mark.parametrize("fmt", ["json", "human", "paths0"])
@pytest.mark.parametrize(
    "boundary", ["stop", "join", "reset", "processing_restore", "delivery_restore"]
)
def test_cli_reflects_signals_received_before_handler_handback(
    tmp_path: Path, capsys: pytest.CaptureFixture[str], fmt: str, boundary: str
) -> None:
    sources = inputs(tmp_path)
    with interrupt_at(boundary) as sent:
        assert cli.main(["--output-format", fmt, *sources]) == 130
    captured = capsys.readouterr()
    assert sent == [1]
    copies = sorted(tmp_path.glob("*.mime-pruned.eml"))
    assert len(copies) == 2
    for source, copy in zip(sources, copies, strict=True):
        _verify_copy(copy, tmp_path / source.rsplit("/", 1)[-1])
    if fmt == "json":
        report = json.loads(captured.out)
        assert report["exit_code"] == 0
        assert not report["interrupted"]
        assert report["summary"]["created"] == 2
        assert all(
            item["terminalized"] and item["publication"]["address_verified"]
            for item in report["items"]
        )
    elif fmt == "paths0":
        assert captured.out == "".join(
            ("\\\\?\\" if os.name == "nt" else "") + str(copy) + "\0" for copy in copies
        )
    else:
        assert captured.out.count("created:") == 2
    assert captured.err.count("interrupted by SIGINT") == 1


def _verify_copy(copy: Path, source: Path) -> None:
    assert source.read_bytes() == MESSAGE
    parsed = BytesParser(policy=policy.default).parsebytes(copy.read_bytes())
    assert parsed.get_body().get_content().strip() == "public body"  # type: ignore[union-attr]
    assert b"PUBLIC-ATTACHMENT" not in copy.read_bytes()


def test_shutdown_interruption_preserves_a_real_publication_error(
    tmp_path: Path, monkeypatch: pytest.MonkeyPatch
) -> None:
    source = tmp_path / "public.eml"
    source.write_bytes(MESSAGE)

    def failed_sync(_parent: BoundDirectoryHandle) -> str:
        message = "public post-publication failure"
        raise OSError(message)

    monkeypatch.setattr(staged_output, "sync_bound_directory", failed_sync)
    with interrupt_at("processing_restore") as sent:
        ledger = process_file(str(source))
    assert sent == [1]
    item = ledger.items[0]
    assert item.status is ItemStatus.PUBLISHED_WITH_ERROR
    assert item.terminalized
    assert item.publication is not None
    assert item.publication.directory_sync == "failed"
    assert item.publication.address_verified
    assert ledger.interruption is not None
    _verify_copy(tmp_path / "public.mime-pruned.eml", source)


def test_guard_cannot_choose_success_before_retirement() -> None:
    with cancellation.delivery_guard() as guard:
        with pytest.raises(
            RuntimeError, match=r"^delivery status requested before handler retirement$"
        ):
            guard.result(0)
    assert guard.result(0) == 0


def test_early_json_error_observes_delivery_handler_restoration(
    capsys: pytest.CaptureFixture[str],
) -> None:
    with interrupt_at("delivery_restore") as sent:
        assert cli.main(["--output-format=json"]) == 130
    captured = capsys.readouterr()
    assert sent == [1]
    assert json.loads(captured.out)["exit_code"] == 2
    assert captured.err.count("interrupted by SIGINT") == 1


def test_delivery_and_controller_retirement_share_one_late_notice(
    tmp_path: Path,
    monkeypatch: pytest.MonkeyPatch,
    capsys: pytest.CaptureFixture[str],
) -> None:
    sources = inputs(tmp_path)
    previous = signal.getsignal(signal.SIGINT)
    original = signal.signal
    sent: list[str] = []

    def restore(number: int, handler: object) -> object:
        current = signal.getsignal(number)
        if number == signal.SIGINT and callable(current) and current != handler:
            kind = "controller" if handler == previous else "delivery"
            retiring = (kind == "controller" and "delivery" in sent) or (
                kind == "delivery"
                and getattr(current, "__name__", "") == "record"
                and getattr(handler, "__self__", None) is not None
            )
            if retiring and kind not in sent:
                sent.append(kind)
                signal.raise_signal(signal.SIGINT)
        return original(number, handler)  # type: ignore[arg-type]

    monkeypatch.setattr(signal, "signal", restore)
    assert cli.main(["--output-format=json", *sources]) == 130
    captured = capsys.readouterr()
    assert sent == ["delivery", "controller"]
    assert captured.err.count("interrupted by SIGINT") == 1
    assert json.loads(captured.out)["summary"]["created"] == 2
