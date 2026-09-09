"""Additional BaseException and raw-option receipts for final mutation evidence."""

from __future__ import annotations

import hashlib
from typing import TYPE_CHECKING

import pytest

from eml_attachment_remover import cli_parser, staged_output
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


def test_stage_owner_transfer_preserves_a_lone_keyboard_interrupt_exactly(
    tmp_path: Path, monkeypatch: pytest.MonkeyPatch
) -> None:
    """A clean compensation path must not group or relabel a cancellation signal."""
    state = _state(tmp_path)
    state.parent = BoundDirectory(41, windows=False)
    interruption = KeyboardInterrupt()
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

    with pytest.raises(KeyboardInterrupt) as raised:
        staged_output._create_stage(state)  # ruff: ignore[private-member-access] - cancellation-safe owner transfer.
    assert raised.value is interruption
    assert closed == [51]


def test_raw_parser_detects_removed_formats_without_interpreting_literal_sources() -> (
    None
):
    """Raw option recognition stops exactly at the end-of-options marker."""
    assert cli_parser._removed_existing("--skip-existing=yes")  # ruff: ignore[private-member-access] - removed long assignment.
    assert cli_parser._removed_paths(  # ruff: ignore[private-member-access] - split removed output channel.
        ["--output-format", "paths"]
    )
    assert cli_parser.raw_json_requested(["--output-format", "json"])
    assert not cli_parser.raw_json_requested(["--", "--output-format", "json"])
