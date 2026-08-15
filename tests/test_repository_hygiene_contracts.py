"""Verify exact fail-closed contracts of the repository hygiene traversal."""

from __future__ import annotations

from pathlib import Path
from typing import TYPE_CHECKING, cast
from unittest.mock import patch

from tools import check_repository_hygiene as hygiene

from tests.repository_hygiene_support import make_public_root

if TYPE_CHECKING:
    from collections.abc import Callable, Iterator
    from os import stat_result

scan_public_directory = cast(
    "Callable[[Path, Path, list[Path], list[hygiene.HygieneIssue]], None]",
    vars(hygiene)["_scan_public_directory"],
)


def messages_for(audit: hygiene.HygieneAudit, path: Path) -> tuple[str, ...]:
    """Return ordered messages attached to one exact path.

    Returns:
        Messages for the requested path in audit order.

    """
    return tuple(issue.message for issue in audit.issues if issue.path == path)


def test_public_binary_and_read_error_diagnostics_are_exact(tmp_path: Path) -> None:
    """Reject undecodable or unreadable public files with stable diagnostics."""
    root = make_public_root(str(tmp_path))
    binary = root / "src" / "binary.dat"
    binary.write_bytes(b"\xff\xfe")
    unreadable = root / "src" / "unreadable.txt"
    unreadable.write_text("PUBLIC\n", encoding="utf-8")
    original_read_bytes = Path.read_bytes

    def reject_one(path: Path) -> bytes:
        if path == unreadable:
            message = "denied"
            raise OSError(message)
        return original_read_bytes(path)

    with patch.object(Path, "read_bytes", reject_one):
        audit = hygiene.audit_repository(root)

    assert messages_for(audit, binary) == (
        (
            "public file is not UTF-8 text; explicitly review and allowlist it or "
            "remove it"
        ),
    )
    assert messages_for(audit, unreadable) == ("cannot read public file: denied",)


def test_opaque_roots_require_directories_without_becoming_public(
    tmp_path: Path,
) -> None:
    """Validate opaque root types and consume them as excluded entries."""
    root = make_public_root(str(tmp_path))
    opaque_file = root / ".git"
    opaque_file.write_text("PUBLIC\n", encoding="utf-8")
    opaque_link = root / ".venv"
    opaque_link.symlink_to(root / "tests")

    audit = hygiene.audit_repository(root)

    assert messages_for(audit, opaque_file) == (
        "reserved generated path must be a directory, found regular file",
    )
    assert messages_for(audit, opaque_link) == (
        "reserved generated path must be a directory, found symbolic link",
        (
            "symbolic links are prohibited in the public source surface; replace "
            "this link with a regular file"
        ),
    )


def test_generated_root_types_and_regular_content_are_checked_exactly(
    tmp_path: Path,
) -> None:
    """Enforce documented generated types and scan regular statistics content."""
    root = make_public_root(str(tmp_path))
    wrong_directory = root / "build"
    wrong_directory.write_text("PUBLIC\n", encoding="utf-8")
    wrong_file = root / ".coverage"
    wrong_file.mkdir()

    audit = hygiene.audit_repository(root)

    assert messages_for(audit, wrong_directory) == (
        "reserved generated path must be a directory, found regular file",
    )
    assert messages_for(audit, wrong_file) == (
        (
            "repository-local coverage data is prohibited because it can embed "
            "machine-specific paths; use ephemeral coverage storage and remove "
            "this path"
        ),
        "reserved generated path must be a regular file, found directory",
    )

    wrong_file.rmdir()
    private_identity = "private@" + "example.com"
    wrong_file.write_text(f"{private_identity}\n", encoding="utf-8")
    content_audit = hygiene.audit_repository(root)
    assert messages_for(content_audit, wrong_file) == (
        (
            "repository-local coverage data is prohibited because it can embed "
            "machine-specific paths; use ephemeral coverage storage and remove "
            "this path"
        ),
    )


def test_root_listing_failure_retains_exact_audit_shape(tmp_path: Path) -> None:
    """Return an empty manifest and root-qualified diagnostic on listing failure."""
    root = make_public_root(str(tmp_path))
    error = OSError("denied")
    with patch.object(Path, "iterdir", side_effect=error):
        audit = hygiene.audit_repository(root)

    assert audit == hygiene.HygieneAudit(
        (),
        (hygiene.HygieneIssue(root, "cannot list root: denied"),),
    )


def test_nested_generated_files_do_not_hide_later_or_root_only_files(
    tmp_path: Path,
) -> None:
    """Continue traversal and apply root-only generated rules only at the root."""
    root = make_public_root(str(tmp_path))
    source = root / "src"
    sensitive_cache = source / "credentials.pyc"
    sensitive_cache.write_bytes(b"cache")
    nested_coverage = source / ".coverage"
    nested_coverage.write_text("PUBLIC\n", encoding="utf-8")
    retained = source / "retained.py"
    retained.write_text("PUBLIC = True\n", encoding="utf-8")
    ordered_children = (sensitive_cache, nested_coverage, retained)
    original_iterdir = Path.iterdir

    def ordered_source(path: Path) -> Iterator[Path]:
        if path == source:
            return iter(ordered_children)
        return original_iterdir(path)

    with patch.object(Path, "iterdir", ordered_source):
        audit = hygiene.audit_repository(root)

    assert retained in audit.public_files
    assert nested_coverage in audit.public_files
    assert sensitive_cache not in audit.public_files
    assert messages_for(audit, sensitive_cache) == (
        (
            "Python bytecode cache is prohibited because it can embed "
            "machine-specific paths; run Python with bytecode disabled and remove "
            "this path"
        ),
        "credential-bearing filename is prohibited; remove this path",
    )


def test_public_scan_continues_after_failed_nodes_and_generated_directories(
    tmp_path: Path,
) -> None:
    """Do not let an uninspectable node or generated directory end traversal."""
    root = make_public_root(str(tmp_path))
    source = root / "src"
    blocked = source / "blocked.py"
    blocked.write_text("PUBLIC = True\n", encoding="utf-8")
    generated = source / "__pycache__"
    generated.mkdir()
    retained = source / "retained.py"
    retained.write_text("PUBLIC = True\n", encoding="utf-8")
    original_iterdir = Path.iterdir
    original_lstat = Path.lstat

    def ordered_source(path: Path) -> Iterator[Path]:
        if path == source:
            return iter((blocked, generated, retained))
        return original_iterdir(path)

    def reject_blocked(path: Path) -> stat_result:
        if path == blocked:
            message = "denied"
            raise OSError(message)
        return original_lstat(path)

    files: list[Path] = []
    issues: list[hygiene.HygieneIssue] = []
    with (
        patch.object(Path, "iterdir", ordered_source),
        patch.object(Path, "lstat", reject_blocked),
    ):
        scan_public_directory(source, root, files, issues)

    assert files == [retained]
    assert issues == [
        hygiene.HygieneIssue(blocked, "cannot inspect path: denied"),
        hygiene.HygieneIssue(
            generated,
            "Python bytecode cache directory is prohibited because bytecode can "
            "embed machine-specific paths; run Python with bytecode disabled and "
            "remove this path",
        ),
    ]


def test_sensitive_mutmut_sidecar_is_scanned_and_does_not_end_traversal(
    tmp_path: Path,
) -> None:
    """Apply exact sidecar rules without changing Mutmut's selector marker."""
    root = make_public_root(str(tmp_path))
    source_root = root / "src"
    sidecar = source_root / "credentials.py.meta"
    private_identity = "private@" + "example.com"
    sidecar.write_text(f"{private_identity}\n", encoding="utf-8")
    retained = source_root / "z-retained.py"
    retained.write_text("PUBLIC = True\n", encoding="utf-8")
    ordered_children = (sidecar, retained)
    original_iterdir = Path.iterdir

    def ordered_source(path: Path) -> Iterator[Path]:
        if path == source_root:
            return iter(ordered_children)
        return original_iterdir(path)

    files: list[Path] = []
    issues: list[hygiene.HygieneIssue] = []
    with (
        patch.object(Path, "iterdir", ordered_source),
        patch.object(
            hygiene.mutmut_workspace,
            "generated_sidecar",
            side_effect=lambda path, _root: path == sidecar,
        ),
    ):
        scan_public_directory(source_root, root, files, issues)

    assert files == [retained]
    assert issues == [
        hygiene.HygieneIssue(
            sidecar,
            "credential-bearing filename is prohibited; remove this path",
        ),
        hygiene.HygieneIssue(
            sidecar,
            "non-reserved email identity on line 1; replace it with an example.test "
            "identity",
        ),
    ]
