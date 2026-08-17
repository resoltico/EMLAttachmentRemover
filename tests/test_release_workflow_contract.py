"""Bind release publication to the exact four-asset provenance contract."""

from __future__ import annotations

import re
import tomllib
from pathlib import Path

PROJECT_ROOT = Path(__file__).resolve().parents[1]
WORKFLOW = PROJECT_ROOT / ".github/workflows/release.yml"
RELEASE_NOTES_DIRECTORY = PROJECT_ROOT / ".github/release-notes"


def _project_version() -> str:
    """Return the sole release version declared in project metadata.

    Returns:
        The static project version.

    """
    with (PROJECT_ROOT / "pyproject.toml").open("rb") as project_file:
        project = tomllib.load(project_file)["project"]
    return str(project["version"])


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


def test_release_workflow_requires_exact_tag_bound_release_notes() -> None:
    """Bind publication title and body to the validated version tag."""
    workflow = WORKFLOW.read_text(encoding="utf-8")
    release_command = workflow.split("- name: Create the GitHub release", maxsplit=1)[1]
    assert "release_version=${RELEASE_TAG#v}" in release_command
    assert 'release_notes=".github/release-notes/$RELEASE_TAG.md"' in release_command
    assert '[[ ! -f "$release_notes" || -L "$release_notes" ]]' in release_command
    assert 'IFS= read -r release_heading < "$release_notes"' in release_command
    assert (
        '"$release_heading" != "# EML Attachment Remover $release_version"'
        in release_command
    )
    assert '--title "EML Attachment Remover $release_version"' in release_command
    assert '--notes-file "$release_notes"' in release_command
    assert "--generate-notes" not in release_command


def test_current_release_notes_are_exactly_version_bound_and_complete() -> None:
    """Require polished notes and exact verification names for this release."""
    version = _project_version()
    tag = f"v{version}"
    notes_path = RELEASE_NOTES_DIRECTORY / f"{tag}.md"
    assert notes_path.is_file()
    assert not notes_path.is_symlink()
    notes = notes_path.read_text(encoding="utf-8")
    lines = notes.splitlines()
    assert lines[0] == f"# EML Attachment Remover {version}"
    assert notes.endswith("\n")
    assert "intentional clean break" in notes
    assert "There is no v1 compatibility mode" in notes
    for expected in (
        "remove-eml-attachments.pyz",
        f"eml_attachment_remover-{version}-cp314-none-any.whl",
        f"eml_attachment_remover-{version}.tar.gz",
        "SHA256SUMS",
        f'gh release verify-asset {tag} "$asset"',
        (
            "--signer-workflow "
            "resoltico/EMLAttachmentRemover/.github/workflows/release.yml"
        ),
        f"--source-ref refs/tags/{tag}",
        f"/blob/{tag}/README.md",
        f"/blob/{tag}/CHANGELOG.md",
    ):
        assert expected in notes
    compare_url = (
        f"https://github.com/resoltico/EMLAttachmentRemover/compare/v1.0.0...{tag}"
    )
    assert notes.count(compare_url) == 1
    assert re.search(r"(?m)^## Verify the download$", notes) is not None


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
