"""Verify that repository-local caches cannot inherit artifact-root authority."""

from __future__ import annotations

from typing import TYPE_CHECKING

from tools import check_repository_hygiene as hygiene

from tests.repository_hygiene_support import make_public_root

if TYPE_CHECKING:
    from pathlib import Path

    import pytest


def test_generated_scanner_preserves_root_provenance(
    tmp_path: Path, monkeypatch: pytest.MonkeyPatch
) -> None:
    """Pass artifact provenance only for declared generated-artifact roots."""
    root = make_public_root(str(tmp_path))
    artifact_roots = hygiene.GENERATED_ARTIFACT_ROOT_DIRECTORIES
    cache_roots = hygiene.policy.TOOL_CACHE_DIRECTORY_NAMES
    for name in artifact_roots | cache_roots:
        (root / name).mkdir()

    observed: dict[str, Path | None] = {}
    original_scan = hygiene._scan_generated_directory  # ruff: ignore[private-member-access]

    def observe(
        directory: Path,
        issues: list[hygiene.HygieneIssue],
        *,
        artifact_root: Path | None,
    ) -> None:
        """Record provenance while executing the real scanner."""
        if directory.parent == root:
            observed[directory.name] = artifact_root
        original_scan(directory, issues, artifact_root=artifact_root)

    monkeypatch.setattr(hygiene, "_scan_generated_directory", observe)
    audit = hygiene.audit_repository(root)

    assert set(observed) == artifact_roots | cache_roots
    assert all(observed[name] == root / name for name in artifact_roots)
    assert all(observed[name] is None for name in cache_roots)
    assert {issue.path for issue in audit.issues} == {
        root / name for name in cache_roots
    }
