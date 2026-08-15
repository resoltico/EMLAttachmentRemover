"""Define shared repository-hygiene diagnostics and filesystem inspection."""

from __future__ import annotations

import stat
from dataclasses import dataclass
from typing import TYPE_CHECKING

if TYPE_CHECKING:
    from pathlib import Path


@dataclass(frozen=True, slots=True)
class HygieneIssue:
    """Describe one public-workspace policy violation."""

    path: Path
    message: str


@dataclass(frozen=True, slots=True)
class HygieneAudit:
    """Contain the public manifest and repository-hygiene issues."""

    public_files: tuple[Path, ...]
    issues: tuple[HygieneIssue, ...]

    def diagnostics(self, root: Path) -> tuple[str, ...]:
        """Return deterministic path-qualified diagnostics for this audit.

        Returns:
            Every issue formatted for a command-line or task-runner failure.

        """
        diagnostics: list[str] = []
        for issue in self.issues:
            try:
                displayed_path = issue.path.relative_to(root)
            except ValueError:
                displayed_path = issue.path
            diagnostics.append(f"{displayed_path}: {issue.message}")
        return tuple(diagnostics)


def path_kind(mode: int) -> str:
    """Return an actionable label for a filesystem mode.

    Returns:
        A stable human-readable filesystem-object kind.

    """
    if stat.S_ISLNK(mode):
        kind = "symbolic link"
    elif stat.S_ISFIFO(mode):
        kind = "FIFO"
    elif stat.S_ISSOCK(mode):
        kind = "socket"
    elif stat.S_ISCHR(mode):
        kind = "character device"
    elif stat.S_ISBLK(mode):
        kind = "block device"
    elif stat.S_ISDIR(mode):
        kind = "directory"
    elif stat.S_ISREG(mode):
        kind = "regular file"
    else:
        kind = "unknown filesystem object"
    return kind


def inspect_mode(path: Path, issues: list[HygieneIssue]) -> int | None:
    """Read a path mode and convert access failures into audit issues.

    Returns:
        The filesystem mode, or ``None`` when inspection failed.

    """
    try:
        return path.lstat().st_mode
    except OSError as error:
        issues.append(HygieneIssue(path, f"cannot inspect path: {error}"))
        return None
