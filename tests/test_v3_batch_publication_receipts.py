"""Exact existing-or-publish transition receipts for batch items."""

from __future__ import annotations

import hashlib
from typing import TYPE_CHECKING

from eml_attachment_remover import batch
from eml_attachment_remover.batch import BatchOptions
from eml_attachment_remover.domain import (
    AppError,
    BoundDestination,
    ExitCode,
    FileIdentity,
    ItemPhase,
    ItemStatus,
    LedgerItem,
    PathValue,
    PublicationReceipt,
    TransformationPlan,
)
from eml_attachment_remover.staged_output import PublishedWithError

if TYPE_CHECKING:
    import pytest


def _options(*, dry_run: bool) -> BatchOptions:
    return BatchOptions(
        dry_run=dry_run,
        existing="verify",
        fail_fast=False,
        output=None,
        output_dir=None,
    )


def _inputs() -> tuple[LedgerItem, BoundDestination, TransformationPlan]:
    request = PathValue("out.eml", "out.eml", "b3V0LmVtbA==")
    destination = BoundDestination(
        request,
        PathValue(".", ".", "Lg=="),
        b"out.eml",
        FileIdentity(1, 2, "directory", 3),
    )
    candidate = b"candidate"
    plan = TransformationPlan(
        (), (), (), hashlib.sha256(candidate).hexdigest(), len(candidate), candidate
    )
    item = LedgerItem(0, PathValue("source.eml", "source.eml", "c291cmNlLmVtbA=="))
    return item, destination, plan


def test_existing_or_publish_preserves_verified_dry_and_created_transitions(
    monkeypatch: pytest.MonkeyPatch,
) -> None:
    """Each successful publication branch has a complete distinct terminal receipt."""
    item, destination, plan = _inputs()
    monkeypatch.setattr(batch, "_publication_inputs", lambda _item: (destination, plan))
    monkeypatch.setattr(batch, "_verify_existing", lambda *_arguments: True)
    assert batch._existing_or_publish(item, _options(dry_run=False), set()) is None  # ruff: ignore[private-member-access] - verified branch short-circuits publication.
    assert item.phase is ItemPhase.REQUESTED

    item, destination, plan = _inputs()
    monkeypatch.setattr(batch, "_publication_inputs", lambda _item: (destination, plan))
    monkeypatch.setattr(batch, "_verify_existing", lambda *_arguments: False)
    assert batch._existing_or_publish(item, _options(dry_run=True), set()) is None  # ruff: ignore[private-member-access] - dry publication receipt.
    assert item.status is ItemStatus.WOULD_CREATE
    assert item.publication == PublicationReceipt(
        visibility="not_attempted",
        identity=None,
        digest=None,
        file_sync="not_attempted",
        directory_sync="not_attempted",
        address_verified=False,
        final_address=None,
        temp_cleanup="not_attempted",
    )

    item, destination, plan = _inputs()
    created = PublicationReceipt(
        visibility="visible",
        identity=FileIdentity(4, 5, "regular", 6),
        digest=plan.candidate_sha256,
        file_sync="succeeded",
        directory_sync="succeeded",
        address_verified=True,
        final_address=destination.request,
        temp_cleanup="succeeded",
    )
    monkeypatch.setattr(batch, "_publication_inputs", lambda _item: (destination, plan))
    monkeypatch.setattr(batch, "_verify_existing", lambda *_arguments: False)
    monkeypatch.setattr(batch, "publish", lambda _destination, _candidate: created)
    assert batch._existing_or_publish(item, _options(dry_run=False), set()) is None  # ruff: ignore[private-member-access] - publication success receipt.
    assert item.phase is ItemPhase.PUBLISHED
    assert item.status is ItemStatus.CREATED
    assert item.publication == created


def test_existing_or_publish_preserves_visible_post_edge_failure_receipt(
    monkeypatch: pytest.MonkeyPatch,
) -> None:
    """A post-edge error retains visibility evidence and classifies its cause."""
    item, destination, plan = _inputs()
    receipt = PublicationReceipt(
        visibility="visible",
        identity=FileIdentity(4, 5, "regular", 6),
        digest=plan.candidate_sha256,
        file_sync="failed",
        directory_sync="not_attempted",
        address_verified=True,
        final_address=destination.request,
        temp_cleanup="succeeded",
    )
    cause = AppError(ExitCode.VERIFICATION_ERROR, "receipt failed")

    def fail_visible(*_arguments: object) -> PublicationReceipt:
        raise PublishedWithError(receipt, cause)

    monkeypatch.setattr(batch, "_publication_inputs", lambda _item: (destination, plan))
    monkeypatch.setattr(batch, "_verify_existing", lambda *_arguments: False)
    monkeypatch.setattr(batch, "publish", fail_visible)
    assert (
        batch._existing_or_publish(  # ruff: ignore[private-member-access] - post-edge error return.
            item, _options(dry_run=False), set()
        )
        is cause
    )
    assert item.status is ItemStatus.PUBLISHED_WITH_ERROR
    assert item.error is cause
    assert item.publication == receipt
