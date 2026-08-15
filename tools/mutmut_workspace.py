"""Authenticate Mutmut workspaces and their exact generated artifacts."""

from __future__ import annotations

import os
import re
import stat
from typing import TYPE_CHECKING, Final

if TYPE_CHECKING:
    from pathlib import Path

MARKER: Final = "MUTANT_UNDER_TEST"
DIRECTORY: Final = "mutants"
GENERATED_ROOT_FILES: Final = frozenset({
    "mutmut-cicd-stats.json",
    "mutmut-stats.json",
})
FIXED_MARKERS: Final = frozenset({
    "",
    "fail",
    "list_all_tests",
    "mutant_generation",
    "stats",
})
SOURCE_ROOTS: Final = frozenset({"src", "tools"})
MUTANT_PATTERN: Final = re.compile(r".+__mutmut_[1-9][0-9]*\Z")


def active(root: Path, *, marker: str | None = None) -> bool:
    """Return whether Mutmut is active in its canonical copied workspace.

    Returns:
        Whether the root and marker form Mutmut's exact local execution context.

    """
    selected_marker = os.environ.get(MARKER) if marker is None else marker
    project_config = root.parent / "pyproject.toml"
    try:
        project_mode = project_config.lstat().st_mode
    except OSError:
        return False
    return (
        selected_marker is not None
        and (
            selected_marker in FIXED_MARKERS
            or MUTANT_PATTERN.fullmatch(selected_marker) is not None
        )
        and root.name == DIRECTORY
        and stat.S_ISREG(project_mode)
    )


def generated_root_file(
    path: Path,
    root: Path,
    *,
    marker: str | None = None,
) -> bool:
    """Return whether a path is a canonical Mutmut root statistic.

    Returns:
        Whether Mutmut owns the exact root file in its authenticated workspace.

    """
    return (
        active(root, marker=marker)
        and path.parent == root
        and path.name in GENERATED_ROOT_FILES
    )


def generated_sidecar(
    path: Path,
    root: Path,
    *,
    marker: str | None = None,
) -> bool:
    """Return whether a file is an exact Mutmut source sidecar.

    A sidecar is trusted only beside its regular Python source inside one of the
    configured mutation source roots in an authenticated copied workspace.

    Returns:
        Whether the file is Mutmut control-plane data rather than public source.

    """
    if not active(root, marker=marker) or path.suffix not in {".meta", ".spans"}:
        return False
    try:
        relative = path.relative_to(root)
        source = path.with_suffix("")
        source_mode = source.lstat().st_mode
        sidecar_mode = path.lstat().st_mode
    except OSError, ValueError:
        return False
    return (
        relative.parts[0] in SOURCE_ROOTS
        and source.suffix == ".py"
        and stat.S_ISREG(source_mode)
        and stat.S_ISREG(sidecar_mode)
    )
