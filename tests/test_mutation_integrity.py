"""Mutation evidence is invalidated by source changes or unapproved workspace writes."""

from __future__ import annotations

import json
import os
from typing import TYPE_CHECKING

import pytest
from tools import mutation_integrity, mutmut_workspace

if TYPE_CHECKING:
    from contextlib import AbstractContextManager
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
    (root / "src/unchanged.py").write_text("VALUE = 2\n")
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
    message = (
        "mutation workspace integrity checkpoint is missing"
        if contents is None
        else "mutation workspace integrity checkpoint is invalid"
    )
    with pytest.raises(RuntimeError, match="^" + message + "$"):
        mutation_integrity.verify(root, checkpoint)


def test_source_inventory_reports_added_and_removed_inputs(tmp_path: Path) -> None:
    root, checkpoint = _workspace(tmp_path)
    (root / "src/unchanged.py").write_text("VALUE = 2\n")
    mutation_integrity.capture(root, checkpoint)
    (root / "src/example.py").unlink()
    (root / "src/added.py").write_text("VALUE = 3\n")
    with pytest.raises(
        RuntimeError,
        match=(
            r"^mutation workspace source inputs changed: "
            r"src/added\.py, src/example\.py$"
        ),
    ):
        mutation_integrity.verify(root, checkpoint)


def test_inventory_selects_the_generated_artifact_policy_for_the_parent_audit(
    tmp_path: Path, monkeypatch: pytest.MonkeyPatch
) -> None:
    root, checkpoint = _workspace(tmp_path)
    policy_marker = mutmut_workspace.policy_marker
    selected: list[str] = []

    def select(marker: str) -> AbstractContextManager[None]:
        selected.append(marker)
        return policy_marker(marker)

    monkeypatch.setattr(mutmut_workspace, "policy_marker", select)
    mutation_integrity.capture(root, checkpoint)
    assert selected == ["stats"]


def test_inventory_identifies_every_unapproved_artifact(tmp_path: Path) -> None:
    root, checkpoint = _workspace(tmp_path)
    (root / "None").write_bytes(b"unexpected output")
    (root / "other").write_bytes(b"unexpected output")
    with pytest.raises(RuntimeError) as caught:
        mutation_integrity.capture(root, checkpoint)
    assert str(caught.value) == (
        "mutation workspace contains unexpected artifacts: "
        "None: unexpected top-level regular file; document or remove this path; "
        "other: unexpected top-level regular file; document or remove this path"
    )


def test_integrity_failure_preserves_the_primary_command_error(tmp_path: Path) -> None:
    root, checkpoint = _workspace(tmp_path)
    primary = ValueError("primary campaign failure")
    with pytest.raises(ValueError, match="primary campaign failure") as caught:
        with mutation_integrity.guard(root, checkpoint, tmp_path / "fixtures"):
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
        with mutation_integrity.guard(root, checkpoint, tmp_path / "fixtures"):
            (root / "src/example.py").write_text("VALUE = 100\n")


def test_inventory_never_changes_the_execution_selector(tmp_path: Path) -> None:
    root, checkpoint = _workspace(tmp_path)
    selector = os.environ.get("MUTANT_UNDER_TEST")
    (root / "None").write_bytes(b"unexpected output")
    with pytest.raises(RuntimeError, match="unexpected artifacts"):
        mutation_integrity.capture(root, checkpoint)
    assert os.environ.get("MUTANT_UNDER_TEST") == selector


def test_owned_directory_repair_traverses_nested_fixtures_and_preserves_files(
    tmp_path: Path,
) -> None:
    root = tmp_path / "fixtures"
    nested = root / "nested"
    nested.mkdir(parents=True)
    data = nested / "data"
    data.write_bytes(b"owned fixture")
    before = data.stat().st_mode
    mutation_integrity.restore_test_directories(root)
    assert nested.is_dir()
    assert data.read_bytes() == b"owned fixture"
    assert data.stat().st_mode == before


@pytest.mark.skipif(
    os.name == "nt", reason="POSIX mutation fixture permission recovery"
)
def test_owned_directory_repair_leaves_links_and_external_permissions_untouched(
    tmp_path: Path,
) -> None:
    root = tmp_path / "fixtures"
    nested = root / "nested"
    nested.mkdir(parents=True)
    data = nested / "data"
    data.write_bytes(b"owned fixture")
    external = tmp_path / "external"
    external.mkdir()
    (root / "link").symlink_to(external, target_is_directory=True)
    before = external.stat().st_mode
    nested.chmod(0o000)
    root.chmod(0o000)
    try:
        mutation_integrity.restore_test_directories(root)
        assert root.stat().st_mode & 0o777 == 0o700
        assert nested.stat().st_mode & 0o777 == 0o700
        assert data.read_bytes() == b"owned fixture"
        assert (root / "link").is_symlink()
        assert external.stat().st_mode == before
    finally:
        root.chmod(0o700)
        nested.chmod(0o700)


@pytest.mark.parametrize("primary", [False, True])
def test_fixture_repair_failure_does_not_replace_an_existing_error(
    tmp_path: Path,
    monkeypatch: pytest.MonkeyPatch,
    *,
    primary: bool,
) -> None:
    root, checkpoint = _workspace(tmp_path)
    failure = OSError("controlled fixture repair failure")

    def refused(_root: Path) -> None:
        raise failure

    monkeypatch.setattr(mutation_integrity, "restore_test_directories", refused)
    if primary:
        error = ValueError("primary command error")
        with pytest.raises(ValueError, match="primary command error") as caught:
            with mutation_integrity.guard(root, checkpoint, tmp_path / "fixtures"):
                raise error
        assert caught.value is error
        assert error.__notes__ == [
            "mutation workspace integrity checkpoint is missing",
            str(failure),
        ]
    else:
        with pytest.raises(
            RuntimeError, match="checkpoint is missing"
        ) as integrity_failure:
            with mutation_integrity.guard(root, checkpoint, tmp_path / "fixtures"):
                pass
        assert integrity_failure.value.__notes__ == [str(failure)]


@pytest.mark.parametrize("primary", [False, True])
def test_clean_source_guard_retires_owned_storage_and_preserves_a_primary_error(
    tmp_path: Path,
    *,
    primary: bool,
) -> None:
    root, checkpoint = _workspace(tmp_path)
    mutation_integrity.capture(root, checkpoint)
    fixtures = tmp_path / "fixtures"
    fixtures.mkdir()
    if primary:
        error = ValueError("primary command failure")
        with pytest.raises(ValueError, match="primary command failure") as caught:
            with mutation_integrity.guard(root, checkpoint, fixtures):
                raise error
        assert caught.value is error
        assert not getattr(error, "__notes__", [])
    else:
        with mutation_integrity.guard(root, checkpoint, fixtures):
            pass
    assert fixtures.is_dir()
