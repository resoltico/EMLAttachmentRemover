"""Exact `_run_item` exception and terminal-ledger receipts."""

from __future__ import annotations

from typing import TYPE_CHECKING

import pytest

from eml_attachment_remover import batch
from eml_attachment_remover.batch import BatchOptions
from eml_attachment_remover.cancellation import CancellationSignal
from eml_attachment_remover.domain import (
    AppError,
    BatchLedger,
    ExitCode,
    FileIdentity,
    ItemStatus,
)
from eml_attachment_remover.native_paths import path_value

if TYPE_CHECKING:
    from eml_attachment_remover.domain import LedgerItem


def _options() -> BatchOptions:
    return BatchOptions(
        dry_run=False,
        existing="verify",
        fail_fast=False,
        output=None,
        output_dir=None,
    )


def _state() -> tuple[BatchLedger, LedgerItem, LedgerItem, dict[int, FileIdentity]]:
    ledger = BatchLedger.from_requests([path_value("one.eml"), path_value("two.eml")])
    identity = FileIdentity(1, 2, "regular", 3)
    return ledger, ledger.items[0], ledger.items[1], {0: identity, 1: identity}


def test_run_item_marks_ordinary_app_errors_but_continues(
    monkeypatch: pytest.MonkeyPatch,
) -> None:
    """A normal per-item failure is terminal only for its own ledger row."""
    ledger, item, later, identities = _state()
    error = AppError(ExitCode.PARSE_ERROR, "bad MIME", phase="parsed")

    def reject(_item: LedgerItem, _identity: FileIdentity) -> None:
        raise error

    monkeypatch.setattr(batch, "_candidate", reject)
    assert not batch._run_item(  # ruff: ignore[private-member-access] - ordinary item error path.
        item, ledger, identities, set(identities.values()), _options()
    )
    assert item.status is ItemStatus.FAILED
    assert item.error is error
    assert later.status is None
    assert ledger.batch_error is None


def test_run_item_internal_and_unexpected_failures_abort_later_rows(
    monkeypatch: pytest.MonkeyPatch,
) -> None:
    """Internal, process-exit, and unknown errors preserve distinct public receipts."""
    cases: tuple[tuple[BaseException, str, str], ...] = (
        (
            AppError(ExitCode.INTERNAL_ERROR, "invariant", phase="classified"),
            "invariant",
            "classified",
        ),
        (SystemExit(), "unexpected SystemExit", "internal"),
        (RuntimeError(), "RuntimeError", "internal"),
    )
    for failure, message, phase in cases:
        ledger, item, later, identities = _state()

        def fail(
            _item: LedgerItem, _identity: FileIdentity, cause: BaseException = failure
        ) -> None:
            raise cause

        monkeypatch.setattr(batch, "_candidate", fail)
        assert batch._run_item(  # ruff: ignore[private-member-access] - internal terminalization table.
            item, ledger, identities, set(identities.values()), _options()
        )
        assert item.error == AppError(ExitCode.INTERNAL_ERROR, message, phase=phase)
        assert ledger.batch_error == AppError(
            ExitCode.INTERNAL_ERROR, message, phase=phase
        )
        assert later.status is ItemStatus.NOT_RUN
        assert later.error is not None
        assert later.error.message == "not run after internal abort"


def test_run_item_records_active_cancellation_without_reclassifying_it(
    monkeypatch: pytest.MonkeyPatch,
) -> None:
    """Cancellation terminalizes only the active row and records its true phase."""
    for failure, signal in (
        (CancellationSignal(1, "SIGHUP"), "SIGHUP"),
        (KeyboardInterrupt(), "SIGINT"),
    ):
        ledger, item, later, identities = _state()

        def cancel(
            _item: LedgerItem, _identity: FileIdentity, cause: BaseException = failure
        ) -> None:
            raise cause

        monkeypatch.setattr(batch, "_candidate", cancel)
        assert batch._run_item(  # ruff: ignore[private-member-access] - active cancellation receipt.
            item, ledger, identities, set(identities.values()), _options()
        )
        assert item.status is ItemStatus.CANCELLED
        assert item.error == AppError(
            ExitCode.INTERRUPTED, f"interrupted by {signal}", phase="requested"
        )
        assert ledger.interruption is not None
        assert (ledger.interruption.signal, ledger.interruption.phase) == (
            signal,
            "requested",
        )
        assert later.status is ItemStatus.NOT_RUN
        assert later.error == AppError(
            ExitCode.INTERRUPTED,
            f"interrupted by {signal}",
            phase="requested",
        )


def test_run_item_does_not_translate_memory_exhaustion(
    monkeypatch: pytest.MonkeyPatch,
) -> None:
    """Memory exhaustion must remain visible to the caller rather than ledgerized."""
    ledger, item, _later, identities = _state()

    def exhaust(_item: LedgerItem, _identity: FileIdentity) -> None:
        raise MemoryError

    monkeypatch.setattr(batch, "_candidate", exhaust)
    with pytest.raises(MemoryError):
        batch._run_item(  # ruff: ignore[private-member-access] - memory error escape.
            item, ledger, identities, set(identities.values()), _options()
        )
