"""Bind release publication to the exact four-asset provenance contract."""

from __future__ import annotations

from pathlib import Path

WORKFLOW = Path(__file__).resolve().parents[1] / ".github/workflows/release.yml"


def test_release_workflow_attests_artifacts_and_checksum_manifest() -> None:
    """Require provenance subjects for all four published release files."""
    workflow = WORKFLOW.read_text(encoding="utf-8")
    assert workflow.count("uses: actions/attest@") == 2
    assert "subject-checksums: release-dist/SHA256SUMS" in workflow
    assert "subject-name: SHA256SUMS" in workflow
    assert "subject-digest: ${{ steps.checksum-manifest.outputs.digest }}" in workflow
    assert "manifest_digest=$(sha256sum release-dist/SHA256SUMS)" in workflow


def test_release_workflow_publishes_only_the_exact_verified_patterns() -> None:
    """Keep publication tag and asset selection explicit and reviewable."""
    workflow = WORKFLOW.read_text(encoding="utf-8")
    release_command = workflow.split("gh release create", maxsplit=1)[1]
    for expected in (
        '"$RELEASE_TAG"',
        "release-dist/*.pyz",
        "release-dist/*.whl",
        "release-dist/*.tar.gz",
        "release-dist/SHA256SUMS",
        ' --repo "$GITHUB_REPOSITORY"',
        " --verify-tag",
    ):
        assert expected in release_command


def test_release_workflow_peels_and_matches_the_live_tag_target() -> None:
    """Prevent tag replacement between qualification and publication."""
    workflow = WORKFLOW.read_text(encoding="utf-8")
    release_command = workflow.split("- name: Create the GitHub release", maxsplit=1)[1]
    assert "git/ref/tags/$RELEASE_TAG" in workflow
    assert "git/tags/$object_sha" in workflow
    assert '"$object_sha" != "$GITHUB_SHA"' in workflow
    assert release_command.index("git/ref/tags/$RELEASE_TAG") < release_command.index(
        "gh release create",
    )
