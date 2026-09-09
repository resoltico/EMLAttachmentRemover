"""Focused contracts for v3 candidate, existing-output, and abort handling."""

from __future__ import annotations

import os
from typing import TYPE_CHECKING

import pytest

from eml_attachment_remover import batch as batch_module
from eml_attachment_remover.batch import BatchOptions, execute
from eml_attachment_remover.cancellation import CancellationSignal
from eml_attachment_remover.domain import (
    AppError,
    BatchLedger,
    ExitCode,
    FileIdentity,
    ItemStatus,
    LedgerItem,
    PathValue,
    PublicationReceipt,
    RetainedFingerprint,
)
from eml_attachment_remover.native_paths import inspect_source_identity, path_value
from eml_attachment_remover.staged_output import PublishedWithError

if TYPE_CHECKING:
    from pathlib import Path

    from eml_attachment_remover.mime_raw import RawNode


RUNTIME_FAULT = "runtime fault"


def _options(
    *,
    dry_run: bool = True,
    existing: str = "error",
) -> BatchOptions:
    return BatchOptions(
        dry_run=dry_run,
        existing=existing,
        fail_fast=False,
        output=None,
        output_dir=None,
    )


def _plain(path: Path, body: bytes = b"body") -> Path:
    path.write_bytes(b"Content-Type: text/plain\r\n\r\n" + body + b"\r\n")
    return path


def test_runtime_item_abort_preserves_prior_dry_run_receipt(
    tmp_path: Path, monkeypatch: pytest.MonkeyPatch
) -> None:
    sources = [_plain(tmp_path / f"{index}.eml") for index in range(3)]
    real_candidate = batch_module._candidate  # ruff: ignore[private-member-access]
    calls = 0

    def fail_second(item: LedgerItem, identity: FileIdentity) -> None:
        nonlocal calls
        calls += 1
        if calls == 2:
            raise RuntimeError(RUNTIME_FAULT)
        real_candidate(item, identity)

    monkeypatch.setattr(batch_module, "_candidate", fail_second)
    ledger = execute([str(source) for source in sources], _options())
    assert [item.status for item in ledger.items] == [
        ItemStatus.WOULD_CREATE,
        ItemStatus.FAILED,
        ItemStatus.NOT_RUN,
    ]
    assert ledger.batch_error is not None
    assert ledger.items[1].error is not None
    assert ledger.items[1].error.code is ExitCode.INTERNAL_ERROR


def test_dry_run_existing_verify_accepts_only_exact_source_candidate(
    tmp_path: Path,
) -> None:
    source = _plain(tmp_path / "message.eml")
    existing = tmp_path / "message.mime-pruned.eml"
    existing.write_bytes(source.read_bytes())
    ledger = execute([str(source)], _options(existing="verify"))
    assert ledger.items[0].status is ItemStatus.EXISTING_VERIFIED
    assert ledger.items[0].publication is not None
    assert ledger.items[0].publication.visibility == "existing_verified"


def test_dry_run_existing_error_rejects_an_occupied_destination(tmp_path: Path) -> None:
    source = _plain(tmp_path / "message.eml")
    (tmp_path / "message.mime-pruned.eml").write_bytes(b"different")
    ledger = execute([str(source)], _options())
    assert ledger.items[0].status is ItemStatus.FAILED
    assert ledger.items[0].error is not None
    assert ledger.items[0].error.code is ExitCode.OUTPUT_CONFLICT


def test_dry_run_existing_verify_constructs_a_candidate_when_absent(
    tmp_path: Path,
) -> None:
    source = _plain(tmp_path / "message.eml")
    ledger = execute([str(source)], _options(existing="verify"))
    assert ledger.items[0].status is ItemStatus.WOULD_CREATE


def test_non_charset_content_type_parameter_does_not_create_charset_warning(
    tmp_path: Path,
) -> None:
    source = tmp_path / "message.eml"
    source.write_bytes(b"Content-Type: text/plain; format=flowed\r\n\r\nbody\r\n")
    ledger = execute([str(source)], _options())
    assert ledger.items[0].status is ItemStatus.WOULD_CREATE
    assert ledger.items[0].warnings == []


def test_existing_verify_rejects_an_output_that_aliases_the_selected_source(
    tmp_path: Path,
) -> None:
    source = _plain(tmp_path / "message.eml")
    os.link(source, tmp_path / "message.mime-pruned.eml")
    ledger = execute([str(source)], _options(existing="verify"))
    assert ledger.items[0].status is ItemStatus.FAILED
    assert ledger.items[0].error is not None
    assert ledger.items[0].error.code is ExitCode.OUTPUT_CONFLICT


def test_interruption_after_completed_dry_run_keeps_item_receipts(
    tmp_path: Path, monkeypatch: pytest.MonkeyPatch
) -> None:
    source = _plain(tmp_path / "message.eml")
    real_run = batch_module._run_inventory_and_items  # ruff: ignore[private-member-access]

    def interrupt_after_work(
        ledger: BatchLedger, sources: list[str], options: BatchOptions
    ) -> None:
        real_run(ledger, sources, options)
        raise CancellationSignal(1, "SIGHUP")

    monkeypatch.setattr(batch_module, "_run_inventory_and_items", interrupt_after_work)
    ledger = execute([str(source)], _options())
    assert ledger.items[0].status is ItemStatus.WOULD_CREATE
    assert ledger.interruption is not None
    assert ledger.interruption.phase == "inventoried"


def test_outer_keyboard_interrupt_terminalizes_unstarted_ledger_rows(
    monkeypatch: pytest.MonkeyPatch,
) -> None:
    def interrupt(
        _ledger: BatchLedger, _sources: list[str], _options: BatchOptions
    ) -> None:
        raise KeyboardInterrupt

    monkeypatch.setattr(batch_module, "_run_inventory_and_items", interrupt)
    ledger = execute(["first.eml"], _options())
    assert ledger.items[0].status is ItemStatus.NOT_RUN
    assert ledger.interruption is not None
    assert ledger.interruption.signal == "SIGINT"


def test_cancel_receipt_keeps_an_already_terminal_item_unchanged() -> None:
    ledger = BatchLedger.from_requests([path_value("message.eml")])
    item = ledger.items[0]
    item.finish(ItemStatus.FAILED, AppError(ExitCode.PARSE_ERROR, "bad source"))
    batch_module._cancel_active_item(  # ruff: ignore[private-member-access]
        item, ledger, "SIGTERM", "candidate"
    )
    assert item.status is ItemStatus.FAILED
    assert ledger.interruption is not None


def test_candidate_rejects_missing_native_source_address() -> None:
    item = LedgerItem(0, PathValue(None, "<none>", None))
    identity = FileIdentity(0, 0, "regular", 0)
    with pytest.raises(AppError) as captured:
        batch_module._candidate(item, identity)  # ruff: ignore[private-member-access]
    assert captured.value.code is ExitCode.INPUT_ERROR


def test_candidate_rejects_source_identity_change(tmp_path: Path) -> None:
    source = _plain(tmp_path / "message.eml")
    item = BatchLedger.from_requests([path_value(str(source))]).items[0]
    wrong_identity = FileIdentity(0, 0, "regular", 0)
    with pytest.raises(AppError) as captured:
        batch_module._candidate(  # ruff: ignore[private-member-access]
            item, wrong_identity
        )
    assert captured.value.code is ExitCode.INPUT_ERROR


def test_candidate_rejects_mismatched_independent_fingerprints(
    tmp_path: Path, monkeypatch: pytest.MonkeyPatch
) -> None:
    source = _plain(tmp_path / "message.eml")
    item = BatchLedger.from_requests([path_value(str(source))]).items[0]
    identity = inspect_source_identity(str(source))

    def empty_fingerprints(
        _raw: bytes, _nodes: list[RawNode]
    ) -> tuple[RetainedFingerprint, ...]:
        return ()

    monkeypatch.setattr(batch_module, "fingerprint_retained", empty_fingerprints)
    with pytest.raises(AppError) as captured:
        batch_module._candidate(item, identity)  # ruff: ignore[private-member-access]
    assert captured.value.code is ExitCode.VERIFICATION_ERROR


def test_candidate_publication_requires_a_complete_candidate_plan() -> None:
    item = LedgerItem(0, path_value("message.eml"))
    with pytest.raises(AppError) as captured:
        batch_module._existing_or_publish(  # ruff: ignore[private-member-access]
            item, _options(), set()
        )
    assert captured.value.code is ExitCode.INTERNAL_ERROR


def test_published_app_error_keeps_the_visible_incomplete_terminal_state(
    tmp_path: Path, monkeypatch: pytest.MonkeyPatch
) -> None:
    source = _plain(tmp_path / "message.eml")

    def incomplete(*_arguments: object) -> PublicationReceipt:
        receipt = PublicationReceipt(
            visibility="visible",
            identity=None,
            digest=None,
            file_sync="failed",
            directory_sync="not_attempted",
            address_verified=True,
            final_address=None,
            temp_cleanup="succeeded",
        )
        raise PublishedWithError(receipt, AppError(ExitCode.WRITE_ERROR, "receipt"))

    monkeypatch.setattr(batch_module, "publish", incomplete)
    ledger = execute([str(source)], _options(dry_run=False))
    assert ledger.items[0].status is ItemStatus.PUBLISHED_WITH_ERROR
    assert ledger.items[0].error is not None
    assert ledger.items[0].error.code is ExitCode.WRITE_ERROR


def test_internal_app_error_stops_the_remaining_batch(
    tmp_path: Path, monkeypatch: pytest.MonkeyPatch
) -> None:
    sources = [_plain(tmp_path / f"{index}.eml") for index in range(2)]

    def internal(_item: LedgerItem, _identity: FileIdentity) -> None:
        raise AppError(ExitCode.INTERNAL_ERROR, "invariant")

    monkeypatch.setattr(batch_module, "_candidate", internal)
    ledger = execute([str(source) for source in sources], _options())
    assert [item.status for item in ledger.items] == [
        ItemStatus.FAILED,
        ItemStatus.NOT_RUN,
    ]
    assert ledger.batch_error is not None


def test_memory_error_is_not_translated_into_an_internal_item_failure(
    tmp_path: Path, monkeypatch: pytest.MonkeyPatch
) -> None:
    source = _plain(tmp_path / "message.eml")

    def exhaust(_item: LedgerItem, _identity: FileIdentity) -> None:
        raise MemoryError

    monkeypatch.setattr(batch_module, "_candidate", exhaust)
    with pytest.raises(MemoryError):
        execute([str(source)], _options())
