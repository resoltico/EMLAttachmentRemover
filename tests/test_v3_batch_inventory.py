"""Focused contracts for v3 batch inventory and its terminal ledger outcomes."""

from __future__ import annotations

import os
from typing import TYPE_CHECKING

import pytest

from eml_attachment_remover import batch as batch_module
from eml_attachment_remover.batch import BatchOptions, execute
from eml_attachment_remover.cancellation import CancellationSignal
from eml_attachment_remover.domain import ExitCode, ItemStatus

if TYPE_CHECKING:
    from pathlib import Path


INVENTORY_FAULT = "inventory fault"


def _options(
    *,
    fail_fast: bool = False,
    output_dir: str | None = None,
) -> BatchOptions:
    return BatchOptions(
        dry_run=True,
        existing="error",
        fail_fast=fail_fast,
        output=None,
        output_dir=output_dir,
    )


def _plain(path: Path, body: bytes = b"body") -> Path:
    path.write_bytes(b"Content-Type: text/plain\r\n\r\n" + body + b"\r\n")
    return path


def test_inventory_cancellation_marks_active_item_cancelled_and_later_not_run(
    monkeypatch: pytest.MonkeyPatch,
) -> None:
    def cancel(_source: str) -> object:
        raise CancellationSignal(15, "SIGTERM")

    monkeypatch.setattr(batch_module, "inspect_source_identity", cancel)
    ledger = execute(["first.eml", "second.eml"], _options())
    assert [item.status for item in ledger.items] == [
        ItemStatus.CANCELLED,
        ItemStatus.NOT_RUN,
    ]
    assert ledger.interruption is not None
    assert ledger.interruption.signal == "SIGTERM"
    assert ledger.items[0].error is not None
    assert ledger.items[0].error.phase == "requested"


def test_inventory_unknown_exception_is_an_internal_abort_with_full_ledger(
    monkeypatch: pytest.MonkeyPatch,
) -> None:
    def explode(_source: str) -> object:
        raise RuntimeError(INVENTORY_FAULT)

    monkeypatch.setattr(batch_module, "inspect_source_identity", explode)
    ledger = execute(["first.eml", "second.eml"], _options())
    assert [item.status for item in ledger.items] == [
        ItemStatus.FAILED,
        ItemStatus.NOT_RUN,
    ]
    assert ledger.batch_error is not None
    assert ledger.batch_error.code is ExitCode.INTERNAL_ERROR
    assert ledger.items[0].error is not None
    assert ledger.items[0].error.phase == "inventory"


def test_inventory_keyboard_interrupt_is_an_active_cancellation(
    monkeypatch: pytest.MonkeyPatch,
) -> None:
    def interrupt(_source: str) -> object:
        raise KeyboardInterrupt

    monkeypatch.setattr(batch_module, "inspect_source_identity", interrupt)
    ledger = execute(["first.eml", "second.eml"], _options())
    assert [item.status for item in ledger.items] == [
        ItemStatus.CANCELLED,
        ItemStatus.NOT_RUN,
    ]
    assert ledger.interruption is not None
    assert ledger.interruption.signal == "SIGINT"


def test_inventory_memory_error_remains_fatal(
    monkeypatch: pytest.MonkeyPatch,
) -> None:
    def exhaust(_source: str) -> object:
        raise MemoryError

    monkeypatch.setattr(batch_module, "inspect_source_identity", exhaust)
    with pytest.raises(MemoryError):
        execute(["first.eml"], _options())


def test_fail_fast_keeps_prior_dry_run_and_stops_after_expected_failure(
    tmp_path: Path,
) -> None:
    good = _plain(tmp_path / "good.eml")
    bad = _plain(tmp_path / "bad.eml", b"!")
    bad.write_bytes(
        b"Content-Type: text/plain\r\nContent-Transfer-Encoding: base64\r\n\r\n!\r\n"
    )
    later = _plain(tmp_path / "later.eml")
    ledger = execute([str(good), str(bad), str(later)], _options(fail_fast=True))
    assert [item.status for item in ledger.items] == [
        ItemStatus.WOULD_CREATE,
        ItemStatus.FAILED,
        ItemStatus.NOT_RUN,
    ]
    assert ledger.batch_error is None
    assert ledger.items[2].error is not None
    assert ledger.items[2].error.message == "not run after fail-fast failure"


def test_source_alias_inventory_marks_every_duplicate_request(tmp_path: Path) -> None:
    source = _plain(tmp_path / "first.eml")
    alias = tmp_path / "alias.eml"
    os.link(source, alias)
    ledger = execute([str(source), str(alias)], _options())
    assert [item.status for item in ledger.items] == [
        ItemStatus.FAILED,
        ItemStatus.FAILED,
    ]
    assert all(item.error is not None for item in ledger.items)
    assert {item.error.code for item in ledger.items if item.error is not None} == {
        ExitCode.INPUT_ERROR
    }


def test_destination_collision_inventory_marks_every_exact_output(
    tmp_path: Path,
) -> None:
    left = tmp_path / "left"
    right = tmp_path / "right"
    output = tmp_path / "output"
    left.mkdir()
    right.mkdir()
    output.mkdir()
    first = _plain(left / "message.eml")
    second = _plain(right / "message.eml")
    ledger = execute([str(first), str(second)], _options(output_dir=str(output)))
    assert [item.status for item in ledger.items] == [
        ItemStatus.FAILED,
        ItemStatus.FAILED,
    ]
    assert all(item.error is not None for item in ledger.items)
    assert {item.error.code for item in ledger.items if item.error is not None} == {
        ExitCode.OUTPUT_CONFLICT
    }


def test_preflight_collision_respects_fail_fast_without_erasing_colliders(
    tmp_path: Path,
) -> None:
    source = _plain(tmp_path / "first.eml")
    alias = tmp_path / "alias.eml"
    os.link(source, alias)
    ledger = execute([str(source), str(alias)], _options(fail_fast=True))
    assert [item.status for item in ledger.items] == [
        ItemStatus.FAILED,
        ItemStatus.FAILED,
    ]


def test_resource_limited_batch_still_terminalizes_every_preallocated_item() -> None:
    sources = [f"item-{index}.eml" for index in range(batch_module.MAX_BATCH_ITEMS + 1)]
    ledger = execute(sources, _options())
    assert len(ledger.items) == len(sources)
    assert {item.status for item in ledger.items} == {ItemStatus.NOT_RUN}


def test_exact_batch_limit_enters_inventory_before_refusing_the_next_item(
    monkeypatch: pytest.MonkeyPatch,
) -> None:
    """Keep the public item limit inclusive and avoid an accidental off-by-one."""
    observed: list[int] = []

    def inventory(_ledger: object, sources: list[str], _options: object) -> None:
        observed.append(len(sources))

    monkeypatch.setattr(batch_module, "_run_inventory_and_items", inventory)
    sources = [f"item-{index}.eml" for index in range(batch_module.MAX_BATCH_ITEMS)]
    execute(sources, _options())
    assert observed == [batch_module.MAX_BATCH_ITEMS]
