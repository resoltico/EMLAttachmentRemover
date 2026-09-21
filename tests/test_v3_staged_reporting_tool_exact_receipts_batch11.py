"""Additional BaseException and raw-option receipts for final mutation evidence."""

from __future__ import annotations

import hashlib
from typing import TYPE_CHECKING

import pytest

from eml_attachment_remover import staged_output
from eml_attachment_remover.cancellation import CancellationSignal
from eml_attachment_remover.domain import BoundDirectory
from eml_attachment_remover.native_paths import bind_destination

if TYPE_CHECKING:
    from pathlib import Path

    from eml_attachment_remover.staged_output import _PublicationState


def _state(tmp_path: Path) -> _PublicationState:
    """Build direct private-stage state.

    Returns:
        A bound lifecycle state for immediate owner-transfer testing.

    """
    candidate = b"candidate"
    return staged_output._PublicationState(  # ruff: ignore[private-member-access] - direct ownership boundary.
        bind_destination(str(tmp_path / "output.eml")),
        candidate,
        hashlib.sha256(candidate).hexdigest(),
    )


def test_stage_owner_transfer_preserves_a_lone_cancellation_signal_exactly(
    tmp_path: Path, monkeypatch: pytest.MonkeyPatch
) -> None:
    """A clean compensation path must not group or relabel a cancellation signal."""
    state = _state(tmp_path)
    state.parent = BoundDirectory(41, windows=False)
    interruption = CancellationSignal(2, "SIGINT")
    closed: list[int] = []
    monkeypatch.setattr(staged_output, "private_stage_name", lambda: b"stage")
    monkeypatch.setattr(staged_output, "create_private_stage", lambda *_args: 51)
    monkeypatch.setattr(
        staged_output,
        "_Stage",
        lambda *_args: (_ for _ in ()).throw(interruption),
    )
    monkeypatch.setattr(
        staged_output,
        "discard_private_stage",
        lambda *_args: (_ for _ in ()).throw(FileNotFoundError()),
    )
    monkeypatch.setattr(staged_output, "_close_descriptor", closed.append)

    with pytest.raises(CancellationSignal) as raised:
        staged_output._create_stage(state)  # ruff: ignore[private-member-access] - cancellation-safe owner transfer.
    assert raised.value is interruption
    assert closed == [51]
