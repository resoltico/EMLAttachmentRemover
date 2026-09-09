"""Exact mutation-resistant contracts for private staging receipts."""

from __future__ import annotations

import hashlib
import os
from typing import TYPE_CHECKING

import pytest

from eml_attachment_remover import staged_output
from eml_attachment_remover.domain import AppError, ExitCode, FileIdentity
from eml_attachment_remover.native_paths import bind_destination

if TYPE_CHECKING:
    from pathlib import Path

    from eml_attachment_remover.staged_output import _PublicationState


def _state(tmp_path: Path, candidate: bytes) -> _PublicationState:
    destination = bind_destination(str(tmp_path / "output.eml"))
    state = staged_output._PublicationState(  # ruff: ignore[private-member-access] - direct lifecycle contract.
        destination, candidate, hashlib.sha256(candidate).hexdigest()
    )
    staged_output._bind_parent(state)  # ruff: ignore[private-member-access] - staged owner setup.
    staged_output._create_stage(state)  # ruff: ignore[private-member-access] - staged owner setup.
    return state


def test_staged_write_accepts_positive_partial_progress(
    tmp_path: Path, monkeypatch: pytest.MonkeyPatch
) -> None:
    """Require each positive partial write to advance to a byte-exact stage."""
    state = _state(tmp_path, b"three")
    original_write = os.write

    def write_one(descriptor: int, data: bytes) -> int:
        return original_write(descriptor, data[:1])

    monkeypatch.setattr(os, "write", write_one)
    staged_output._verify_staged(state)  # ruff: ignore[private-member-access] - positive-progress contract.
    assert state.stage is not None
    os.lseek(state.stage.descriptor, 0, os.SEEK_SET)
    assert os.read(state.stage.descriptor, 16) == b"three"
    staged_output._cleanup(state)  # ruff: ignore[private-member-access] - owned stage cleanup.


def test_staged_write_rejects_nonprogress_and_impossible_progress(
    tmp_path: Path, monkeypatch: pytest.MonkeyPatch
) -> None:
    """A native write result must consume a nonempty proper slice of the candidate."""
    assert (
        staged_output._advanced_write_position(  # ruff: ignore[private-member-access] - exact write offset.
            0, 1, 2
        )
        == 1
    )
    for reported in (-1, 0, 3):
        with pytest.raises(AppError) as captured:
            staged_output._advanced_write_position(  # ruff: ignore[private-member-access] - direct native-write progress validation.
                0, reported, 2
            )
        assert captured.value == AppError(
            ExitCode.WRITE_ERROR, "short write while staging candidate"
        )
    state = _state(tmp_path, b"two")
    for reported in (-1, 4):
        with monkeypatch.context() as context:
            context.setattr(os, "write", lambda *_args, reported=reported: reported)
            with pytest.raises(AppError) as captured:
                staged_output._verify_staged(state)  # ruff: ignore[private-member-access] - bounded write progress.
            assert captured.value == AppError(
                ExitCode.WRITE_ERROR, "short write while staging candidate"
            )
    staged_output._cleanup(state)  # ruff: ignore[private-member-access] - owned stage cleanup.


def test_staged_write_rejects_a_nonadvancing_position_transition(
    tmp_path: Path, monkeypatch: pytest.MonkeyPatch
) -> None:
    """A bounded write loop fails closed when its advancement transition regresses."""
    state = _state(tmp_path, b"two")
    monkeypatch.setattr(
        staged_output,
        "_advanced_write_position",
        lambda position, _written, _size: position,
    )
    with pytest.raises(AppError) as captured:
        staged_output._verify_staged(state)  # ruff: ignore[private-member-access] - defensive helper-progress invariant.
    assert captured.value == AppError(
        ExitCode.WRITE_ERROR, "short write while staging candidate"
    )
    staged_output._cleanup(state)  # ruff: ignore[private-member-access] - owned stage cleanup.


def test_reconciliation_records_every_unproven_receipt_field(
    tmp_path: Path, monkeypatch: pytest.MonkeyPatch
) -> None:
    """Require a failed re-address to retain its complete truthful receipt."""
    state = _state(tmp_path, b"candidate")
    identity = FileIdentity(1, 2, "regular", 3)
    monkeypatch.setattr(
        staged_output,
        "_read_final_receipt",
        lambda _state: (_ for _ in ()).throw(OSError("readdress")),
    )
    monkeypatch.setattr(staged_output, "descriptor_identity", lambda _fd: identity)
    receipt = staged_output._reconcile(state, "unsupported")  # ruff: ignore[private-member-access] - truthful unproven receipt.
    assert receipt.visibility == "not_proven"
    assert receipt.identity == identity
    assert receipt.digest == hashlib.sha256(b"candidate").hexdigest()
    assert receipt.file_sync == "succeeded"
    assert receipt.directory_sync == "unsupported"
    assert receipt.address_verified is False
    assert receipt.final_address is None
    assert receipt.temp_cleanup == "pending"
    staged_output._cleanup(state)  # ruff: ignore[private-member-access] - owned stage cleanup.
