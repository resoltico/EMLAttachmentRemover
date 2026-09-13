"""Exact lifecycle handoff and staged-progress mutation receipts."""

from __future__ import annotations

import pytest

from eml_attachment_remover import batch, staged_progress
from eml_attachment_remover.batch import BatchOptions
from eml_attachment_remover.cancellation import CancellationSignal
from eml_attachment_remover.domain import (
    AppError,
    BatchLedger,
    ExitCode,
    FileIdentity,
    ItemPhase,
    ItemStatus,
)
from eml_attachment_remover.native_paths import path_value


def _options() -> BatchOptions:
    """Build one ordinary apply-mode batch configuration.

    Returns:
        One fixed non-dry-run option bundle.

    """
    return BatchOptions(
        dry_run=False,
        existing="verify",
        fail_fast=False,
        output=None,
        output_dir=None,
    )


def test_staged_progress_rejects_an_exactly_stalled_transition() -> None:
    """The independent transition boundary cannot accept an unchanged offset."""
    with pytest.raises(AppError) as raised:
        staged_progress._validated_next_position(  # ruff: ignore[private-member-access] - exact internal arithmetic boundary.
            2, 2, 3
        )
    assert raised.value == AppError(
        ExitCode.WRITE_ERROR, "short write while staging candidate"
    )


def test_run_item_forwards_a_postpublication_cancellation_to_terminalization(
    monkeypatch: pytest.MonkeyPatch,
) -> None:
    """A post-edge cancellation must preserve its cause through `_run_item`."""
    ledger = BatchLedger.from_requests([path_value("one.eml"), path_value("two.eml")])
    item, later = ledger.items
    identity = FileIdentity(1, 2, "regular", 3)
    cause = CancellationSignal(1, "SIGHUP")

    def publish_with_interruption(*_arguments: object) -> CancellationSignal:
        item.phase = ItemPhase.PUBLISHED
        item.finish(
            ItemStatus.PUBLISHED_WITH_ERROR,
            AppError(ExitCode.INTERRUPTED, "interrupted after publication"),
        )
        return cause

    monkeypatch.setattr(batch, "_candidate", lambda *_arguments: None)
    monkeypatch.setattr(batch, "_existing_or_publish", publish_with_interruption)

    assert batch._run_item(  # ruff: ignore[private-member-access] - lifecycle cause handoff.
        item, ledger, {0: identity, 1: identity}, {identity}, _options()
    )
    assert ledger.interruption is not None
    assert (ledger.interruption.signal, ledger.interruption.phase) == (
        "SIGHUP",
        "published",
    )
    assert later.status is ItemStatus.NOT_RUN
    assert later.error == AppError(
        ExitCode.INTERRUPTED, "interrupted by SIGHUP", phase="published"
    )
