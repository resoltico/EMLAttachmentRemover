"""Validate portable archive paths, member sets, and source byte streams."""

from __future__ import annotations

import importlib
import re
from pathlib import PurePosixPath
from typing import TYPE_CHECKING

if TYPE_CHECKING:
    from pathlib import Path
    from typing import IO

path_policy = importlib.import_module(
    "tools.repository_path_policy" if __package__ else "repository_path_policy"
)


class DistributionArchiveError(RuntimeError):
    """Report an unsafe or unexpected source or wheel archive."""


def safe_parts(name: str, *, directory: bool) -> tuple[str, ...]:
    """Return unambiguous portable archive-name components.

    Returns:
        The validated non-empty POSIX path components.

    Raises:
        DistributionArchiveError: If a name is absolute, traversing, or ambiguous.

    """
    candidate = name[:-1] if directory and name.endswith("/") else name
    parts = tuple(candidate.split("/"))
    invalid = (
        "\\" in candidate
        or "\x00" in candidate
        or any(part in {"", ".", ".."} for part in parts)
        or bool(re.fullmatch(r"[A-Za-z]:.*", parts[0]))
    )
    if invalid:
        message = f"unsafe or ambiguous archive member name: {name!r}"
        raise DistributionArchiveError(message)
    portability_issues = path_policy.audit_paths((candidate,))
    if portability_issues:
        message = f"non-portable archive member name: {name!r}"
        raise DistributionArchiveError(message)
    return parts


def allowed_directories(file_names: set[str]) -> set[str]:
    """Return optional directory entries needed by the exact file set.

    Returns:
        Every proper parent directory of an expected archive file.

    """
    directories: set[str] = set()
    for name in file_names:
        parent = PurePosixPath(name).parent
        while parent != PurePosixPath():
            directories.add(parent.as_posix())
            parent = parent.parent
    return directories


def same_content(archived: IO[bytes], source: Path, chunk_size: int) -> bool:
    """Return whether an archive stream exactly matches one source file.

    Returns:
        ``True`` only when both byte streams end together and match completely.

    """
    with source.open("rb") as source_file:
        while True:
            archived_chunk = archived.read(chunk_size)
            source_chunk = source_file.read(chunk_size)
            if archived_chunk != source_chunk:
                return False
            if not archived_chunk:
                return True


def require_exact_set(label: str, actual: set[str], expected: set[str]) -> None:
    """Require an exact archive member set.

    Raises:
        DistributionArchiveError: If members are missing or unexpected.

    """
    if actual != expected:
        missing = sorted(expected - actual)
        unknown = sorted(actual - expected)
        message = f"{label} member mismatch: missing={missing}; unknown={unknown}"
        raise DistributionArchiveError(message)
