"""Integrated v3 publication-edge and terminal-ledger regression tests."""

from __future__ import annotations

import os
from typing import TYPE_CHECKING

import pytest

from eml_attachment_remover import batch as batch_module
from eml_attachment_remover.batch import BatchOptions, execute
from eml_attachment_remover.cancellation import CancellationSignal
from eml_attachment_remover.cli import exit_code
from eml_attachment_remover.domain import (
    AppError,
    BatchLedger,
    ExitCode,
    FileIdentity,
    ItemPhase,
    ItemStatus,
    LedgerItem,
    PublicationReceipt,
)
from eml_attachment_remover.native_paths import path_value
from eml_attachment_remover.reporting_v3 import report
from eml_attachment_remover.staged_output import PublishedWithError

if TYPE_CHECKING:
    from pathlib import Path


def _options(*, existing: str = "error") -> BatchOptions:
    return BatchOptions(
        dry_run=False,
        existing=existing,
        fail_fast=False,
        output=None,
        output_dir=None,
    )


@pytest.mark.parametrize("fault", [KeyboardInterrupt(), SystemExit(), OSError("close")])
def test_visible_post_link_fault_is_never_recorded_as_cancelled_or_failed(
    tmp_path: Path, monkeypatch: pytest.MonkeyPatch, fault: BaseException
) -> None:
    source = tmp_path / "message.eml"
    source.write_bytes(b"Content-Type: text/plain\r\n\r\nbody\r\n")
    real_unlink = os.unlink
    injected = False

    def interrupt_first_unlink(path: str | bytes, *, dir_fd: int | None = None) -> None:
        nonlocal injected
        if not injected:
            injected = True
            raise fault
        real_unlink(path, dir_fd=dir_fd)

    monkeypatch.setattr(os, "unlink", interrupt_first_unlink)
    ledger = execute([str(source)], _options())
    item = ledger.items[0]
    assert item.status is ItemStatus.PUBLISHED_WITH_ERROR
    assert item.publication is not None
    assert item.publication.visibility == "visible"
    assert (tmp_path / "message.mime-pruned.eml").is_file()


def test_existing_verify_rejects_final_entry_replacement_race(
    tmp_path: Path, monkeypatch: pytest.MonkeyPatch
) -> None:
    source = tmp_path / "message.eml"
    source.write_bytes(b"Content-Type: text/plain\r\n\r\nbody\r\n")
    first = execute([str(source)], _options())
    assert first.items[0].status is ItemStatus.CREATED
    destination = tmp_path / "message.mime-pruned.eml"
    real_stat = os.stat

    def swapped_entry(
        path: str | bytes,
        *,
        dir_fd: int | None = None,
        follow_symlinks: bool = True,
    ) -> os.stat_result:
        result = real_stat(path, dir_fd=dir_fd, follow_symlinks=follow_symlinks)
        if path == os.fsencode(destination.name) and dir_fd is not None:
            values = list(result)
            values[1] += 1
            return os.stat_result(tuple(values))
        return result

    monkeypatch.setattr(os, "stat", swapped_entry)
    raced = execute([str(source)], _options(existing="verify"))
    assert raced.items[0].status is ItemStatus.FAILED


def test_keyboard_interrupt_preserves_prior_ledger_history_and_json_receipt(
    tmp_path: Path, monkeypatch: pytest.MonkeyPatch
) -> None:
    sources = [tmp_path / "first.eml", tmp_path / "second.eml"]
    for source in sources:
        source.write_bytes(b"Content-Type: text/plain\r\n\r\nbody\r\n")
    real_candidate = batch_module._candidate  # ruff: ignore[private-member-access]
    calls = 0

    def interrupt_second(item: LedgerItem, identity: FileIdentity) -> None:
        nonlocal calls
        calls += 1
        if calls == 2:
            raise KeyboardInterrupt
        real_candidate(item, identity)

    monkeypatch.setattr(batch_module, "_candidate", interrupt_second)
    ledger = execute([str(source) for source in sources], _options())
    assert [item.status for item in ledger.items] == [
        ItemStatus.CREATED,
        ItemStatus.CANCELLED,
    ]
    assert ledger.interruption is not None
    document = report(ledger, "apply", 130)
    assert document["interrupted"] is True
    assert document["interruption"] == {
        "signal": "SIGINT",
        "reason": "interrupted by SIGINT",
        "phase": "inventoried",
    }


def test_unexpected_system_exit_is_an_internal_batch_abort(
    tmp_path: Path, monkeypatch: pytest.MonkeyPatch
) -> None:
    sources = [tmp_path / "first.eml", tmp_path / "second.eml"]
    for source in sources:
        source.write_bytes(b"Content-Type: text/plain\r\n\r\nbody\r\n")

    def abort(*_arguments: object) -> None:
        raise SystemExit

    monkeypatch.setattr(batch_module, "_candidate", abort)
    ledger = execute([str(source) for source in sources], _options())
    assert [item.status for item in ledger.items] == [
        ItemStatus.FAILED,
        ItemStatus.NOT_RUN,
    ]
    assert ledger.items[0].error is not None
    assert ledger.items[0].error.code.value == 70


def test_sigterm_receipt_cancels_active_work_and_preserves_later_requests(
    tmp_path: Path, monkeypatch: pytest.MonkeyPatch
) -> None:
    sources = [tmp_path / "first.eml", tmp_path / "second.eml"]
    for source in sources:
        source.write_bytes(b"Content-Type: text/plain\r\n\r\nbody\r\n")

    def terminate(*_arguments: object) -> None:
        raise CancellationSignal(15, "SIGTERM")

    monkeypatch.setattr(batch_module, "_candidate", terminate)
    ledger = execute([str(source) for source in sources], _options())
    assert [item.status for item in ledger.items] == [
        ItemStatus.CANCELLED,
        ItemStatus.NOT_RUN,
    ]
    assert ledger.interruption is not None
    assert ledger.interruption.signal == "SIGTERM"
    assert ledger.items[1].error is not None
    assert ledger.items[1].error.code.value == 130


def test_terminalization_preserves_last_operational_phase(tmp_path: Path) -> None:
    source = tmp_path / "message.eml"
    source.write_bytes(
        b"Content-Type: text/plain\r\nContent-Transfer-Encoding: base64\r\n\r\n!\r\n"
    )
    ledger = execute([str(source)], _options())
    item = ledger.items[0]
    assert item.status is ItemStatus.FAILED
    assert item.terminalized is True
    assert item.phase is ItemPhase.CLASSIFIED
    document = report(ledger, "apply", 5)
    items = document["items"]
    assert isinstance(items, list)
    first = items[0]
    assert isinstance(first, dict)
    assert first["terminalized"] is True


def test_interruption_after_terminal_items_forces_exit_130() -> None:
    ledger = BatchLedger.from_requests([])
    ledger.record_interruption("SIGTERM", "report")
    assert exit_code(ledger) == 130


def test_multi_item_internal_abort_has_exit_70_not_batch_9() -> None:
    requests = [path_value("first.eml"), path_value("second.eml")]
    ledger = BatchLedger.from_requests(requests)
    ledger.items[0].finish(
        ItemStatus.FAILED, AppError(ExitCode.INTERNAL_ERROR, "internal")
    )
    ledger.finalize_not_run("not run after internal abort")
    assert exit_code(ledger) == 70


def test_inventory_system_exit_is_ledgered_as_an_internal_abort(
    tmp_path: Path, monkeypatch: pytest.MonkeyPatch
) -> None:
    sources = [tmp_path / "first.eml", tmp_path / "second.eml"]
    for source in sources:
        source.write_bytes(b"Content-Type: text/plain\r\n\r\nbody\r\n")

    def abort_inventory(_request: str) -> FileIdentity:
        raise SystemExit

    monkeypatch.setattr(batch_module, "inspect_source_identity", abort_inventory)
    ledger = execute([str(source) for source in sources], _options())
    assert [item.status for item in ledger.items] == [
        ItemStatus.FAILED,
        ItemStatus.NOT_RUN,
    ]
    assert ledger.batch_error is not None
    assert exit_code(ledger) == 70


def _visible_publication_receipt() -> PublicationReceipt:
    return PublicationReceipt(
        visibility="visible",
        identity=None,
        digest=None,
        file_sync="succeeded",
        directory_sync="succeeded",
        address_verified=True,
        final_address=None,
        temp_cleanup="succeeded",
    )


def test_post_publish_cancellation_keeps_pwe_and_forces_exit_130(
    tmp_path: Path, monkeypatch: pytest.MonkeyPatch
) -> None:
    source = tmp_path / "message.eml"
    source.write_bytes(b"Content-Type: text/plain\r\n\r\nbody\r\n")

    def interrupt_publish(*_arguments: object) -> PublicationReceipt:
        raise PublishedWithError(
            _visible_publication_receipt(), CancellationSignal(15, "SIGTERM")
        )

    monkeypatch.setattr(batch_module, "publish", interrupt_publish)
    ledger = execute([str(source)], _options())
    item = ledger.items[0]
    assert item.status is ItemStatus.PUBLISHED_WITH_ERROR
    assert item.error is not None
    assert item.error.code is ExitCode.INTERRUPTED
    assert ledger.interruption is not None
    assert ledger.interruption.signal == "SIGTERM"
    assert exit_code(ledger) == 130


def test_post_publish_system_exit_is_internal_not_interruption(
    tmp_path: Path, monkeypatch: pytest.MonkeyPatch
) -> None:
    source = tmp_path / "message.eml"
    source.write_bytes(b"Content-Type: text/plain\r\n\r\nbody\r\n")

    def abort_publish(*_arguments: object) -> PublicationReceipt:
        raise PublishedWithError(_visible_publication_receipt(), SystemExit())

    monkeypatch.setattr(batch_module, "publish", abort_publish)
    ledger = execute([str(source)], _options())
    item = ledger.items[0]
    assert item.status is ItemStatus.PUBLISHED_WITH_ERROR
    assert item.error is not None
    assert item.error.code is ExitCode.INTERNAL_ERROR
    assert ledger.interruption is None
    assert ledger.batch_error is not None
    assert exit_code(ledger) == 70
