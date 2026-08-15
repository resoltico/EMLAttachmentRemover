"""Verify authentication of Mutmut-owned generated workspace artifacts."""

from __future__ import annotations

import os
from typing import TYPE_CHECKING

import pytest
from tools import mutmut_workspace

if TYPE_CHECKING:
    from pathlib import Path


@pytest.fixture
def active_workspace(
    tmp_path: Path,
) -> tuple[Path, Path]:
    """Create one canonical workspace for explicit marker authentication.

    Returns:
        The project and its canonical Mutmut workspace root.

    """
    project = tmp_path / "project"
    project.mkdir()
    (project / "pyproject.toml").write_text("[project]\n", encoding="utf-8")
    root = project / "mutants"
    root.mkdir()
    return project, root


@pytest.mark.parametrize("marker", ["", "stats", "package.x_value__mutmut_1"])
def test_active_accepts_exact_mutmut_markers(
    active_workspace: tuple[Path, Path],
    marker: str,
) -> None:
    """Authenticate fixed control markers and positive numbered mutants."""
    _project, root = active_workspace
    assert mutmut_workspace.active(root, marker=marker)


@pytest.mark.parametrize(
    "marker",
    [
        "package.x_value__mutmut_0",
        "package.x_value__mutmut_1.extra",
        "not-a-mutmut-marker",
    ],
)
def test_active_rejects_lookalike_mutmut_markers(
    active_workspace: tuple[Path, Path],
    marker: str,
) -> None:
    """Reject marker lookalikes outside the exact control-plane grammar."""
    _project, root = active_workspace
    assert not mutmut_workspace.active(root, marker=marker)


def test_active_uses_the_ambient_marker_by_default(
    active_workspace: tuple[Path, Path],
) -> None:
    """Read Mutmut's real control marker when no explicit marker is supplied."""
    _project, root = active_workspace
    ambient_marker = os.environ.get(mutmut_workspace.MARKER)
    assert mutmut_workspace.active(root) == mutmut_workspace.active(
        root,
        marker=ambient_marker,
    )


def test_active_requires_marker_canonical_name_and_regular_project_file(
    active_workspace: tuple[Path, Path],
) -> None:
    """Fail closed when any independent workspace credential is absent."""
    project, root = active_workspace
    assert not mutmut_workspace.active(root, marker="not-a-mutmut-marker")
    wrong_root = project / "other"
    wrong_root.mkdir()
    assert not mutmut_workspace.active(wrong_root, marker="stats")

    project_file = project / "pyproject.toml"
    project_file.unlink()
    assert not mutmut_workspace.active(root, marker="stats")
    project_file.mkdir()
    assert not mutmut_workspace.active(root, marker="stats")
    project_file.rmdir()
    target = project / "project-config.txt"
    target.write_text("[project]\n", encoding="utf-8")
    project_file.symlink_to(target)
    assert not mutmut_workspace.active(root, marker="stats")


def test_generated_root_statistics_require_exact_location_and_name(
    active_workspace: tuple[Path, Path],
) -> None:
    """Recognize only the two documented statistics at the workspace root."""
    _project, root = active_workspace
    assert mutmut_workspace.generated_root_file(
        root / "mutmut-stats.json",
        root,
        marker="stats",
    )
    assert mutmut_workspace.generated_root_file(
        root / "mutmut-cicd-stats.json",
        root,
        marker="stats",
    )
    assert not mutmut_workspace.generated_root_file(
        root / "mutmut-stats.json.backup",
        root,
        marker="stats",
    )
    assert not mutmut_workspace.generated_root_file(
        root / "src" / "mutmut-stats.json",
        root,
        marker="stats",
    )


def test_generated_sidecar_requires_active_workspace_and_exact_extension(
    active_workspace: tuple[Path, Path],
) -> None:
    """Reject plausible sidecars when the marker or extension is wrong."""
    _project, root = active_workspace
    source_root = root / "src"
    source_root.mkdir()
    source = source_root / "public.py"
    source.write_text("PUBLIC = True\n", encoding="utf-8")
    metadata = source_root / "public.py.meta"
    metadata.write_text("{}\n", encoding="utf-8")
    spans = source_root / "public.py.spans"
    spans.write_text("{}\n", encoding="utf-8")
    lookalike = source_root / "public.py.txt"
    lookalike.write_text("{}\n", encoding="utf-8")

    assert mutmut_workspace.generated_sidecar(metadata, root, marker="stats")
    assert mutmut_workspace.generated_sidecar(spans, root, marker="stats")
    assert not mutmut_workspace.generated_sidecar(lookalike, root, marker="stats")
    assert not mutmut_workspace.generated_sidecar(
        metadata,
        root,
        marker="not-a-mutmut-marker",
    )


def test_generated_sidecar_requires_python_source_in_a_mutated_root(
    active_workspace: tuple[Path, Path],
) -> None:
    """Reject missing, misplaced, and non-Python sidecar pairs."""
    _project, root = active_workspace
    source_root = root / "src"
    source_root.mkdir()

    missing = source_root / "missing.py.meta"
    missing.write_text("{}\n", encoding="utf-8")
    assert not mutmut_workspace.generated_sidecar(missing, root, marker="stats")

    tests_root = root / "tests"
    tests_root.mkdir()
    misplaced_source = tests_root / "public.py"
    misplaced_source.write_text("PUBLIC = True\n", encoding="utf-8")
    misplaced = tests_root / "public.py.meta"
    misplaced.write_text("{}\n", encoding="utf-8")
    assert not mutmut_workspace.generated_sidecar(misplaced, root, marker="stats")

    text_source = source_root / "public.txt"
    text_source.write_text("PUBLIC\n", encoding="utf-8")
    text_sidecar = source_root / "public.txt.meta"
    text_sidecar.write_text("{}\n", encoding="utf-8")
    assert not mutmut_workspace.generated_sidecar(text_sidecar, root, marker="stats")


def test_generated_sidecar_pair_must_contain_two_regular_files(
    active_workspace: tuple[Path, Path],
) -> None:
    """Reject a source directory or sidecar directory masquerading as a pair."""
    _project, root = active_workspace
    source_root = root / "src"
    source_root.mkdir()
    directory_source = source_root / "directory.py"
    directory_source.mkdir()
    directory_source_sidecar = source_root / "directory.py.meta"
    directory_source_sidecar.write_text("{}\n", encoding="utf-8")
    assert not mutmut_workspace.generated_sidecar(
        directory_source_sidecar,
        root,
        marker="stats",
    )

    source = source_root / "directory-sidecar.py"
    source.write_text("PUBLIC = True\n", encoding="utf-8")
    sidecar_directory = source_root / "directory-sidecar.py.meta"
    sidecar_directory.mkdir()
    assert not mutmut_workspace.generated_sidecar(
        sidecar_directory,
        root,
        marker="stats",
    )


def test_generated_sidecar_rejects_a_regular_pair_outside_workspace(
    active_workspace: tuple[Path, Path],
) -> None:
    """Fail closed when a sidecar cannot be made relative to the workspace."""
    project, root = active_workspace
    outside_source = project / "outside.py"
    outside_source.write_text("PUBLIC = True\n", encoding="utf-8")
    outside = project / "outside.py.meta"
    outside.write_text("{}\n", encoding="utf-8")
    assert not mutmut_workspace.generated_sidecar(outside, root, marker="stats")
