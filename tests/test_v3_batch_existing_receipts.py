"""Exact existing-output verification receipts for the batch boundary."""

from __future__ import annotations

import hashlib

import pytest

from eml_attachment_remover import batch
from eml_attachment_remover.batch import BatchOptions
from eml_attachment_remover.domain import (
    AppError,
    BoundDestination,
    ExistingEntry,
    ExitCode,
    FileIdentity,
    ItemStatus,
    LedgerItem,
    PathValue,
    PublicationReceipt,
    TransformationPlan,
)


def _options(existing: str) -> BatchOptions:
    return BatchOptions(
        dry_run=False,
        existing=existing,
        fail_fast=False,
        output=None,
        output_dir=None,
    )


def _destination() -> BoundDestination:
    request = PathValue("out.eml", "out.eml", "b3V0LmVtbA==")
    return BoundDestination(
        request,
        PathValue(".", ".", "Lg=="),
        b"out.eml",
        FileIdentity(1, 2, "directory", 3),
    )


def _plan() -> TransformationPlan:
    candidate = b"candidate"
    return TransformationPlan(
        (), (), (), hashlib.sha256(candidate).hexdigest(), len(candidate), candidate
    )


def _item() -> LedgerItem:
    return LedgerItem(0, PathValue("source.eml", "source.eml", "c291cmNlLmVtbA=="))


def test_existing_error_mode_checks_only_occupancy(
    monkeypatch: pytest.MonkeyPatch,
) -> None:
    """The strict existing mode distinguishes absence from any occupied entry."""
    destination = _destination()
    plan = _plan()
    monkeypatch.setattr(batch, "existing_identity", lambda _destination: None)
    assert not batch._verify_existing(  # ruff: ignore[private-member-access] - absent strict destination.
        _item(), destination, plan, _options("error"), set()
    )
    identity = FileIdentity(4, 5, "regular", 6)
    monkeypatch.setattr(batch, "existing_identity", lambda _destination: identity)
    with pytest.raises(AppError) as captured:
        batch._verify_existing(  # ruff: ignore[private-member-access] - occupied strict destination.
            _item(), destination, plan, _options("error"), set()
        )
    assert captured.value == AppError(
        ExitCode.OUTPUT_CONFLICT, "destination already exists"
    )


def test_existing_verify_requires_nonaliased_exact_candidate_and_records_receipt(
    monkeypatch: pytest.MonkeyPatch,
) -> None:
    """Verified reuse has exact identity, bytes, terminal state, and receipt facts."""
    destination = _destination()
    plan = _plan()
    identity = FileIdentity(4, 5, "regular", 6)
    monkeypatch.setattr(batch, "read_existing", lambda _destination: None)
    assert not batch._verify_existing(  # ruff: ignore[private-member-access] - absent verified destination.
        _item(), destination, plan, _options("verify"), set()
    )

    for existing, identities, expected in (
        (
            ExistingEntry(identity, plan.candidate),
            {identity},
            AppError(
                ExitCode.OUTPUT_CONFLICT, "existing output aliases a selected source"
            ),
        ),
        (
            ExistingEntry(identity, b"different"),
            set(),
            AppError(
                ExitCode.OUTPUT_CONFLICT,
                "existing output is not the exact current candidate",
            ),
        ),
    ):
        monkeypatch.setattr(
            batch, "read_existing", lambda _destination, value=existing: value
        )
        with pytest.raises(AppError) as captured:
            batch._verify_existing(  # ruff: ignore[private-member-access] - alias and byte mismatch rejection.
                _item(), destination, plan, _options("verify"), identities
            )
        assert captured.value == expected

    item = _item()
    monkeypatch.setattr(
        batch,
        "read_existing",
        lambda _destination: ExistingEntry(identity, plan.candidate),
    )
    assert batch._verify_existing(  # ruff: ignore[private-member-access] - complete verified receipt.
        item, destination, plan, _options("verify"), set()
    )
    assert item.status is ItemStatus.EXISTING_VERIFIED
    assert item.publication == PublicationReceipt(
        visibility="existing_verified",
        identity=identity,
        digest=plan.candidate_sha256,
        file_sync="not_attempted",
        directory_sync="not_attempted",
        address_verified=True,
        final_address=destination.request,
        temp_cleanup="not_applicable",
    )
