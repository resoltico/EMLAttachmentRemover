"""Keep opaque artifact exemptions separate from repository-local tool caches."""

from __future__ import annotations

from pathlib import Path

from tools import check_repository_hygiene as hygiene

from tests.repository_hygiene_support import make_public_root


def test_only_declared_artifact_roots_accept_opaque_archives(tmp_path: Path) -> None:
    """Reject matching archive bytes in tool caches, not actual release roots."""
    root = make_public_root(str(tmp_path))
    artifact_roots = ("build", "dist", "release-dist")
    cache_roots = hygiene.policy.TOOL_CACHE_DIRECTORY_NAMES
    archive_name = "candidate-macos-arm64.zip"
    for directory_name in (*artifact_roots, *cache_roots):
        directory = root / directory_name
        directory.mkdir()
        (directory / archive_name).write_bytes(b"\xff")

    audit = hygiene.audit_repository(root)
    rejected_archives = {
        issue.path for issue in audit.issues if "opaque generated file" in issue.message
    }
    assert {root / name / archive_name for name in cache_roots} <= rejected_archives
    assert {root / name / archive_name for name in artifact_roots}.isdisjoint(
        rejected_archives
    )
