"""Reject files and filesystem objects unsuitable for a public repository."""

from __future__ import annotations

import importlib
import stat
import sys
from pathlib import Path
from typing import TYPE_CHECKING, Final

if TYPE_CHECKING:
    from tools.repository_hygiene_types import HygieneAudit, HygieneIssue
    from tools.repository_hygiene_types import inspect_mode as _mode
    from tools.repository_hygiene_types import path_kind as _kind
else:
    hygiene_types = importlib.import_module(
        "tools.repository_hygiene_types" if __package__ else "repository_hygiene_types"
    )
    HygieneAudit = hygiene_types.HygieneAudit
    HygieneIssue = hygiene_types.HygieneIssue
    _mode = hygiene_types.inspect_mode
    _kind = hygiene_types.path_kind

__all__ = ["HygieneAudit", "HygieneIssue", "_kind"]

policy = importlib.import_module(
    "tools.repository_hygiene_policy" if __package__ else "repository_hygiene_policy"
)
mutmut_workspace = importlib.import_module(
    "tools.mutmut_workspace" if __package__ else "mutmut_workspace"
)
path_policy = importlib.import_module(
    "tools.repository_path_policy" if __package__ else "repository_path_policy"
)

PROJECT_ROOT: Final = Path(__file__).resolve().parents[1]
PUBLIC_ROOT_FILES: Final = frozenset({
    ".gitattributes",
    ".gitignore",
    ".python-version",
    "CHANGELOG.md",
    "LICENSE",
    "QA.md",
    "README.md",
    "pyproject.toml",
    "uv.lock",
})
PUBLIC_ROOT_DIRECTORIES: Final = frozenset({
    ".github",
    "integrations",
    "schema",
    "src",
    "tests",
    "tools",
})
OPAQUE_ROOT_DIRECTORIES: Final = frozenset({
    ".git",
    ".venv",
})
GENERATED_ARTIFACT_ROOT_DIRECTORIES: Final = frozenset({
    ".hypothesis",
    "build",
    "dist",
    "mutants",
    "release-dist",
})
GENERATED_ROOT_DIRECTORIES: Final = frozenset({
    *GENERATED_ARTIFACT_ROOT_DIRECTORIES,
    *policy.TOOL_CACHE_DIRECTORY_NAMES,
})
OPAQUE_ARCHIVE_SUFFIXES: Final = (".tar.gz", ".whl")
OPAQUE_ARCHIVE_NAMES: Final = frozenset({"remove-eml-attachments.pyz"})


def _append_regular_file(
    path: Path,
    files: list[Path],
    issues: list[HygieneIssue],
) -> None:
    """Add one regular public file and report a sensitive filename."""
    files.append(path)
    _append_private_issue(path, issues)
    _append_public_content_issues(path, issues)


def _append_private_issue(path: Path, issues: list[HygieneIssue]) -> None:
    """Report a sensitive filename without adding it to the public manifest."""
    reason = policy.private_file_reason(path.name)
    if reason is not None:
        issues.append(HygieneIssue(path, f"{reason}; remove this path"))


def _append_cache_issue(
    path: Path,
    issues: list[HygieneIssue],
    *,
    directory: bool,
) -> bool:
    """Append one exact cache diagnostic when the path is prohibited.

    Returns:
        Whether the path is a recognized repository-local cache.

    """
    reason = (
        policy.cache_directory_reason(path.name)
        if directory
        else policy.cache_file_reason(path.name)
    )
    if reason is None:
        return False
    issues.append(HygieneIssue(path, reason))
    return True


def _append_public_content_issues(
    path: Path,
    issues: list[HygieneIssue],
) -> None:
    """Reject private identities, home paths, and unreviewed binary artifacts."""
    try:
        text = path.read_bytes().decode()
    except UnicodeDecodeError:
        issues.append(
            HygieneIssue(
                path,
                "public file is not UTF-8 text; explicitly review and allowlist it "
                "or remove it",
            ),
        )
        return
    except OSError as error:
        issues.append(HygieneIssue(path, f"cannot read public file: {error}"))
        return
    issues.extend(
        HygieneIssue(path, message) for message in policy.public_content_messages(text)
    )


def _append_symlink_issue(path: Path, issues: list[HygieneIssue]) -> None:
    """Reject a symlink from the portable public source surface."""
    issues.append(
        HygieneIssue(
            path,
            "symbolic links are prohibited in the public source surface; replace "
            "this link with a regular file",
        ),
    )


def _scan_public_directory(
    directory: Path,
    root: Path,
    files: list[Path],
    issues: list[HygieneIssue],
) -> None:
    """Recursively inspect a public directory without consulting Git."""
    try:
        children = tuple(directory.iterdir())
    except OSError as error:
        issues.append(HygieneIssue(directory, f"cannot list directory: {error}"))
        return
    for path in children:
        mode = _mode(path, issues)
        if mode is None:
            continue
        if stat.S_ISDIR(mode) and policy.generated_directory(path.name):
            _append_cache_issue(path, issues, directory=True)
            _scan_generated_directory(path, issues, artifact_root=None)
            continue
        if stat.S_ISREG(mode) and policy.generated_file(path.name):
            _append_private_issue(path, issues)
            _append_cache_issue(path, issues, directory=False)
            continue
        if stat.S_ISREG(mode) and mutmut_workspace.generated_sidecar(path, root):
            _append_private_issue(path, issues)
            issues.extend(
                HygieneIssue(path, message)
                for message in policy.generated_content_messages(path)
            )
            continue
        _inspect_public_node(path, mode, root, files, issues)


def _inspect_public_node(
    path: Path,
    mode: int,
    root: Path,
    files: list[Path],
    issues: list[HygieneIssue],
) -> None:
    """Inspect one non-generated node within a public directory."""
    if stat.S_ISDIR(mode):
        _scan_public_directory(path, root, files, issues)
    elif stat.S_ISREG(mode):
        _append_regular_file(path, files, issues)
    elif stat.S_ISLNK(mode):
        _append_symlink_issue(path, issues)
    else:
        message = (
            f"unsupported public artifact type {_kind(mode)!r}; use a regular file"
        )
        issues.append(HygieneIssue(path, message))


def _scan_generated_directory(
    directory: Path,
    issues: list[HygieneIssue],
    *,
    artifact_root: Path | None,
) -> None:
    """Name-scan repository-owned generated output without following links."""
    try:
        children = tuple(directory.iterdir())
    except OSError as error:
        issues.append(
            HygieneIssue(directory, f"cannot list generated directory: {error}")
        )
        return
    for path in children:
        mode = _mode(path, issues)
        if mode is None:
            continue
        _append_private_issue(path, issues)
        if stat.S_ISLNK(mode):
            _append_symlink_issue(path, issues)
        elif stat.S_ISDIR(mode):
            _append_cache_issue(path, issues, directory=True)
            _scan_generated_directory(path, issues, artifact_root=artifact_root)
        elif stat.S_ISREG(mode):
            is_cache = _append_cache_issue(path, issues, directory=False)
            if not is_cache:
                issues.extend(
                    HygieneIssue(path, message)
                    for message in policy.generated_content_messages(
                        path,
                        allow_opaque=_expected_opaque_archive(path, artifact_root),
                    )
                )
        else:
            message = (
                f"unsupported generated artifact type {_kind(mode)!r}; remove this path"
            )
            issues.append(HygieneIssue(path, message))


def _expected_opaque_archive(path: Path, artifact_root: Path | None) -> bool:
    """Return whether a direct generated archive has a validated artifact shape.

    Returns:
        Whether opacity is expected for this build/release artifact path.

    """
    if (
        artifact_root is None
        or artifact_root.name not in {"build", "dist", "release-dist"}
        or path.parent != artifact_root
    ):
        return False
    return path.name in OPAQUE_ARCHIVE_NAMES or path.name.endswith(
        OPAQUE_ARCHIVE_SUFFIXES
    )


def _inspect_root_entry(
    path: Path,
    root: Path,
    files: list[Path],
    issues: list[HygieneIssue],
) -> None:
    """Inspect one direct child of the workspace root."""
    mode = _mode(path, issues)
    if mode is None:
        return
    if _inspect_excluded_root(path, mode, issues):
        return
    if policy.generated_root_file(
        path.name,
        mutmut_statistic=mutmut_workspace.generated_root_file(path, root),
    ):
        is_cache = policy.cache_file_reason(path.name) is not None
        _inspect_reserved_root(
            path,
            mode,
            expected="regular file",
            issues=issues,
            inspect_content=not is_cache,
        )
        _append_private_issue(path, issues)
        _append_cache_issue(path, issues, directory=False)
        return
    if path.name in PUBLIC_ROOT_FILES:
        _inspect_public_root_file(path, mode, files, issues)
        return
    if path.name in PUBLIC_ROOT_DIRECTORIES:
        _inspect_public_root_directory(path, mode, root, files, issues)
        return
    issues.append(
        HygieneIssue(
            path,
            f"unexpected top-level {_kind(mode)}; document or remove this path",
        ),
    )
    if stat.S_ISREG(mode):
        _append_private_issue(path, issues)
    if stat.S_ISLNK(mode):
        _append_symlink_issue(path, issues)


def _inspect_excluded_root(
    path: Path,
    mode: int,
    issues: list[HygieneIssue],
) -> bool:
    """Inspect an opaque or repository-generated root when one applies.

    Returns:
        Whether the path belongs to an excluded root category.

    """
    if path.name in OPAQUE_ROOT_DIRECTORIES:
        _inspect_reserved_root(path, mode, expected="directory", issues=issues)
        return True
    if path.name not in GENERATED_ROOT_DIRECTORIES:
        return False
    _inspect_reserved_root(path, mode, expected="directory", issues=issues)
    _append_cache_issue(path, issues, directory=True)
    if stat.S_ISDIR(mode):
        _scan_generated_directory(
            path,
            issues,
            artifact_root=(
                path if path.name in GENERATED_ARTIFACT_ROOT_DIRECTORIES else None
            ),
        )
    return True


def _inspect_reserved_root(
    path: Path,
    mode: int,
    *,
    expected: str,
    issues: list[HygieneIssue],
    inspect_content: bool = True,
) -> None:
    """Require a reserved generated root path to have its documented type."""
    correct_type = stat.S_ISDIR(mode) if expected == "directory" else stat.S_ISREG(mode)
    if not correct_type:
        issues.append(
            HygieneIssue(
                path,
                f"reserved generated path must be a {expected}, found {_kind(mode)}",
            ),
        )
        if stat.S_ISLNK(mode):
            _append_symlink_issue(path, issues)
    elif expected == "regular file" and inspect_content:
        issues.extend(
            HygieneIssue(path, message)
            for message in policy.generated_content_messages(path)
        )


def _inspect_public_root_file(
    path: Path,
    mode: int,
    files: list[Path],
    issues: list[HygieneIssue],
) -> None:
    """Require a documented root file to resolve to a regular public file."""
    if stat.S_ISREG(mode):
        _append_regular_file(path, files, issues)
    elif stat.S_ISLNK(mode):
        _append_symlink_issue(path, issues)
    else:
        issues.append(HygieneIssue(path, f"documented root file is a {_kind(mode)}"))


def _inspect_public_root_directory(
    path: Path,
    mode: int,
    root: Path,
    files: list[Path],
    issues: list[HygieneIssue],
) -> None:
    """Require a documented root directory to be a real directory."""
    if stat.S_ISDIR(mode):
        _scan_public_directory(path, root, files, issues)
    else:
        issues.append(
            HygieneIssue(path, f"documented root directory is a {_kind(mode)}"),
        )


def audit_repository(root: Path = PROJECT_ROOT) -> HygieneAudit:
    """Audit the workspace and return its deterministic public manifest.

    Returns:
        Every public file and every policy issue in stable path order.

    """
    root = root.resolve()
    files: list[Path] = []
    issues: list[HygieneIssue] = []
    try:
        entries = tuple(root.iterdir())
    except OSError as error:
        return HygieneAudit((), (HygieneIssue(root, f"cannot list root: {error}"),))
    for path in entries:
        _inspect_root_entry(path, root, files, issues)
    file_pairs = sorted((path.relative_to(root).as_posix(), path) for path in files)
    relative_files = tuple(relative_path for relative_path, _path in file_pairs)
    ordered_files = tuple(path for _relative_path, path in file_pairs)
    issues.extend(
        HygieneIssue(root / issue.relative_path, issue.message)
        for issue in path_policy.audit_paths(relative_files)
    )
    ordered_issues = tuple(
        sorted(
            issues,
            key=lambda issue: (issue.path.relative_to(root).as_posix(), issue.message),
        ),
    )
    return HygieneAudit(ordered_files, ordered_issues)


def main() -> int:
    """Report all hygiene issues and return a command-line status code.

    Returns:
        Zero for a clean workspace, otherwise one.

    """
    audit = audit_repository(PROJECT_ROOT)
    for diagnostic in audit.diagnostics(PROJECT_ROOT):
        print(diagnostic, file=sys.stderr)
    return int(bool(audit.issues))


if __name__ == "__main__":
    raise SystemExit(main())
