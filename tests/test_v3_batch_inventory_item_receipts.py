"""Exact batch inventory-item exception and ledger receipts."""

from __future__ import annotations

from typing import TYPE_CHECKING

import pytest

from eml_attachment_remover import batch
from eml_attachment_remover.batch import BatchOptions
from eml_attachment_remover.cancellation import CancellationSignal
from eml_attachment_remover.domain import AppError, BatchLedger, ExitCode, ItemStatus
from eml_attachment_remover.native_paths import path_value

if TYPE_CHECKING:
    from eml_attachment_remover.batch import _Inventory
    from eml_attachment_remover.domain import LedgerItem


def _options() -> BatchOptions:
    return BatchOptions(
        dry_run=False,
        existing="verify",
        fail_fast=False,
        output=None,
        output_dir=None,
    )


def _state() -> tuple[BatchLedger, LedgerItem, LedgerItem, _Inventory]:
    ledger = BatchLedger.from_requests([path_value("one.eml"), path_value("two.eml")])
    return ledger, ledger.items[0], ledger.items[1], batch._Inventory.empty()  # ruff: ignore[private-member-access] - direct inventory boundary.


def test_inventory_item_marks_ordinary_input_error_without_stopping(
    monkeypatch: pytest.MonkeyPatch,
) -> None:
    """Expected input failures belong to their one active ledger row only."""
    ledger, item, later, inventory = _state()
    error = AppError(ExitCode.INPUT_ERROR, "unreadable source", phase="requested")
    monkeypatch.setattr(
        batch._Inventory,  # ruff: ignore[private-member-access] - inventory boundary fault injection.
        "add",
        lambda *_arguments: (_ for _ in ()).throw(error),
    )
    assert not batch._inventory_item(  # ruff: ignore[private-member-access] - ordinary inventory error.
        item, "one.eml", _options(), inventory, ledger
    )
    assert item.status is ItemStatus.FAILED
    assert item.error is error
    assert later.status is None
    assert ledger.batch_error is None


def test_inventory_item_records_cancellation_and_stops(
    monkeypatch: pytest.MonkeyPatch,
) -> None:
    """Inventory cancellation records the active phase and finalizes later entries."""
    for failure, signal in (
        (CancellationSignal(1, "SIGHUP"), "SIGHUP"),
        (KeyboardInterrupt(), "SIGINT"),
    ):
        ledger, item, later, inventory = _state()
        monkeypatch.setattr(
            batch._Inventory,  # ruff: ignore[private-member-access] - inventory boundary fault injection.
            "add",
            lambda *_arguments, cause=failure: (_ for _ in ()).throw(cause),
        )
        assert batch._inventory_item(  # ruff: ignore[private-member-access] - inventory cancellation.
            item, "one.eml", _options(), inventory, ledger
        )
        assert item.status is ItemStatus.CANCELLED
        assert item.error == AppError(
            ExitCode.INTERRUPTED,
            f"interrupted by {signal}",
            phase="requested",
        )
        assert later.status is ItemStatus.NOT_RUN
        assert later.error == AppError(
            ExitCode.INTERRUPTED,
            f"interrupted by {signal}",
            phase="requested",
        )


def test_inventory_item_internalizes_system_and_unknown_failures(
    monkeypatch: pytest.MonkeyPatch,
) -> None:
    """SystemExit and unknown faults retain their stable inventory receipts."""
    for failure, message in (
        (SystemExit(), "unexpected SystemExit"),
        (RuntimeError(), "RuntimeError"),
    ):
        ledger, item, later, inventory = _state()
        monkeypatch.setattr(
            batch._Inventory,  # ruff: ignore[private-member-access] - inventory boundary fault injection.
            "add",
            lambda *_arguments, cause=failure: (_ for _ in ()).throw(cause),
        )
        assert batch._inventory_item(  # ruff: ignore[private-member-access] - inventory internal abort.
            item, "one.eml", _options(), inventory, ledger
        )
        expected = AppError(ExitCode.INTERNAL_ERROR, message, phase="inventory")
        assert item.error == expected
        assert ledger.batch_error == expected
        assert later.status is ItemStatus.NOT_RUN
        assert later.error is not None
        assert later.error.message == "not run after internal abort"


def test_inventory_item_does_not_translate_memory_exhaustion(
    monkeypatch: pytest.MonkeyPatch,
) -> None:
    """MemoryError is never converted into a recoverable ledger item error."""
    ledger, item, _later, inventory = _state()
    monkeypatch.setattr(
        batch._Inventory,  # ruff: ignore[private-member-access] - inventory boundary fault injection.
        "add",
        lambda *_arguments: (_ for _ in ()).throw(MemoryError()),
    )
    with pytest.raises(MemoryError):
        batch._inventory_item(  # ruff: ignore[private-member-access] - memory exhaustion escape.
            item, "one.eml", _options(), inventory, ledger
        )
