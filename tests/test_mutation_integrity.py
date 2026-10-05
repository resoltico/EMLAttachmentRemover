"""Mutation evidence is invalidated by source changes or unapproved workspace writes."""

from __future__ import annotations

import json
import os
from typing import TYPE_CHECKING

import pytest
from tools import mutation_integrity

if TYPE_CHECKING:
    from pathlib import Path


def _workspace(directory: Path) -> tuple[Path, Path]:
    """Create an authenticated copied workspace and its private checkpoint.

    Returns:
        The workspace and checkpoint paths.

    """
    (directory / "pyproject.toml").write_text("[project]\n")
    root = directory / "mutants"
    (root / "src").mkdir(parents=True)
    (root / "src/example.py").write_text("VALUE = 1\n")
    return root, directory / "checkpoint.json"


def test_control_plane_updates_do_not_change_the_sealed_input_inventory(
    tmp_path: Path,
) -> None:
    root, checkpoint = _workspace(tmp_path)
    mutation_integrity.capture(root, checkpoint)
    (root / "mutmut-stats.json").write_text("{}")
    (root / "src/example.py.meta").write_text("{}")
    mutation_integrity.verify(root, checkpoint)
    if os.name != "nt":
        assert checkpoint.stat().st_mode & 0o777 == 0o600


def test_changed_source_cannot_be_reapproved_by_a_second_capture(
    tmp_path: Path,
) -> None:
    root, checkpoint = _workspace(tmp_path)
    mutation_integrity.capture(root, checkpoint)
    sealed = checkpoint.read_bytes()
    (root / "src/example.py").write_text("VALUE = 100\n")
    mutation_integrity.capture(root, checkpoint)
    assert checkpoint.read_bytes() == sealed
    with pytest.raises(
        RuntimeError,
        match=r"^mutation workspace source inputs changed: src/example\.py$",
    ):
        mutation_integrity.verify(root, checkpoint)


@pytest.mark.parametrize("stage", ["capture", "verify"])
def test_unapproved_workspace_files_refuse_evidence(tmp_path: Path, stage: str) -> None:
    root, checkpoint = _workspace(tmp_path)
    if stage == "verify":
        mutation_integrity.capture(root, checkpoint)
    (root / "None").write_bytes(b"unapproved mutant output")
    with pytest.raises(RuntimeError, match="unexpected artifacts: None"):
        getattr(mutation_integrity, stage)(root, checkpoint)


@pytest.mark.parametrize("contents", [None, [], {"src/example.py": [True] * 6}])
def test_missing_and_malformed_checkpoints_cannot_approve_a_campaign(
    tmp_path: Path,
    contents: object,
) -> None:
    root, checkpoint = _workspace(tmp_path)
    if contents is not None:
        checkpoint.write_text(json.dumps(contents))
    with pytest.raises(
        RuntimeError, match=r"checkpoint is missing|checkpoint is invalid"
    ):
        mutation_integrity.verify(root, checkpoint)


def test_integrity_failure_preserves_the_primary_command_error(tmp_path: Path) -> None:
    root, checkpoint = _workspace(tmp_path)
    primary = ValueError("primary campaign failure")
    with pytest.raises(ValueError, match="primary campaign failure") as caught:
        with mutation_integrity.guard(root, checkpoint):
            raise primary
    assert caught.value is primary
    assert str(caught.value) == "primary campaign failure"
    assert primary.__notes__ == ["mutation workspace integrity checkpoint is missing"]


def test_a_successful_command_still_requires_an_unchanged_workspace(
    tmp_path: Path,
) -> None:
    root, checkpoint = _workspace(tmp_path)
    mutation_integrity.capture(root, checkpoint)
    with pytest.raises(RuntimeError, match="source inputs changed"):
        with mutation_integrity.guard(root, checkpoint):
            (root / "src/example.py").write_text("VALUE = 100\n")


def test_inventory_restores_the_caller_marker_after_refusal(
    tmp_path: Path, monkeypatch: pytest.MonkeyPatch
) -> None:
    root, checkpoint = _workspace(tmp_path)
    monkeypatch.setenv("MUTANT_UNDER_TEST", "caller-marker")
    (root / "None").write_bytes(b"unexpected output")
    with pytest.raises(RuntimeError, match="unexpected artifacts"):
        mutation_integrity.capture(root, checkpoint)
    assert os.environ["MUTANT_UNDER_TEST"] == "caller-marker"
