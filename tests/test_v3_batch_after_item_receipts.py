"""Exact batch post-item control-flow receipts."""

from __future__ import annotations

from types import SimpleNamespace
from typing import TYPE_CHECKING

from eml_attachment_remover import batch, batch_terminal, report_stream
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
    import pytest

    from eml_attachment_remover.domain import LedgerItem


def _options(*, fail_fast: bool) -> BatchOptions:
    return BatchOptions(
        dry_run=False,
        existing="verify",
        fail_fast=fail_fast,
        output=None,
        output_dir=None,
    )


def _ledger() -> tuple[BatchLedger, LedgerItem, LedgerItem]:
    ledger = BatchLedger.from_requests([path_value("one.eml"), path_value("two.eml")])
    return ledger, ledger.items[0], ledger.items[1]


def test_after_item_records_post_publication_cancellation_exactly() -> None:
    """Post-edge cancellation retains the fixed published phase and signal identity."""
    ledger, item, _later = _ledger()
    assert batch._after_item(  # ruff: ignore[private-member-access] - named cancellation after publication.
        item, ledger, _options(fail_fast=False), CancellationSignal(1, "SIGHUP")
    )
    assert ledger.interruption is not None
    assert (ledger.interruption.signal, ledger.interruption.phase) == (
        "SIGHUP",
        "published",
    )

    ledger, item, _later = _ledger()
    assert batch._after_item(  # ruff: ignore[private-member-access] - keyboard cancellation after publication.
        item, ledger, _options(fail_fast=False), KeyboardInterrupt()
    )
    assert ledger.interruption is not None
    assert (ledger.interruption.signal, ledger.interruption.phase) == (
        "SIGINT",
        "published",
    )


def test_after_item_preserves_internal_and_fail_fast_terminalization() -> None:
    """System exit and ordinary failed items stop later work for distinct reasons."""
    ledger, item, later = _ledger()
    assert batch._after_item(  # ruff: ignore[private-member-access] - post-edge system exit.
        item, ledger, _options(fail_fast=False), SystemExit()
    )
    assert item.error == AppError(
        ExitCode.INTERNAL_ERROR,
        "unexpected SystemExit after publication",
        phase="internal",
    )
    assert later.status is ItemStatus.NOT_RUN
    assert later.error is not None
    assert later.error.message == "not run after internal abort"

    ledger, item, later = _ledger()
    item.finish(ItemStatus.FAILED, AppError(ExitCode.PARSE_ERROR, "bad source"))
    assert batch._after_item(  # ruff: ignore[private-member-access] - fail-fast normal failure.
        item, ledger, _options(fail_fast=True), None
    )
    assert later.status is ItemStatus.NOT_RUN
    assert later.error is not None
    assert later.error.message == "not run after fail-fast failure"

    ledger, item, later = _ledger()
    item.finish(ItemStatus.FAILED, AppError(ExitCode.PARSE_ERROR, "bad source"))
    assert not batch._after_item(  # ruff: ignore[private-member-access] - non-fail-fast continuation.
        item, ledger, _options(fail_fast=False), None
    )
    assert later.status is None


def test_after_item_fail_fast_stops_after_an_unsuccessful_publication() -> None:
    """Fail-fast includes an item that crossed publication but remains unsuccessful."""
    ledger, item, later = _ledger()
    item.finish(
        ItemStatus.PUBLISHED_WITH_ERROR,
        AppError(ExitCode.WRITE_ERROR, "post-publication receipt failed"),
    )
    assert batch._after_item(  # ruff: ignore[private-member-access] - post-edge fail-fast receipt.
        item, ledger, _options(fail_fast=True), None
    )
    assert later.status is ItemStatus.NOT_RUN


def test_after_item_stops_when_terminal_archive_recovery_reports_failure(
    monkeypatch: pytest.MonkeyPatch,
) -> None:
    """A failed terminal archive must prevent any later batch work from starting."""
    ledger, item, later = _ledger()
    item.finish(ItemStatus.FAILED, AppError(ExitCode.PARSE_ERROR, "bad source"))
    monkeypatch.setattr(report_stream, "archive_or_recover", lambda *_args: False)
    assert batch._after_item(  # ruff: ignore[private-member-access] - report recovery stop.
        item, ledger, _options(fail_fast=False), None
    )
    assert later.status is None


def test_inventory_passes_the_exact_fail_fast_boolean_to_terminal_control(
    monkeypatch: pytest.MonkeyPatch,
) -> None:
    """The inventory loop cannot silently downgrade fail-fast to a falsey sentinel."""
    ledger = BatchLedger.from_requests([path_value("one.eml")])
    identity = FileIdentity(1, 2, "regular", 3)
    observed: list[bool] = []

    monkeypatch.setattr(
        batch,
        "_inventory",
        lambda *_args: SimpleNamespace(identities={0: identity}),
    )
    monkeypatch.setattr(report_stream, "archive_or_recover", lambda *_args: True)

    def skip(_item: object, _ledger: object, *, fail_fast: bool) -> bool:
        observed.append(fail_fast)
        return True

    monkeypatch.setattr(batch_terminal, "skip_inventory_failure", skip)
    batch._run_inventory_and_items(  # ruff: ignore[private-member-access] - exact terminal-control argument.
        ledger,
        ["one.eml"],
        _options(fail_fast=True),
    )
    assert observed == [True]
