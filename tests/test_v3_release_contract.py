"""Schema-3 release artifact, workflow, and immutable-tag contracts."""

from __future__ import annotations

import runpy
import subprocess
import sys
import tomllib
from pathlib import Path

import pytest
from tools import check_release_tag

PROJECT_ROOT = Path(__file__).resolve().parents[1]
RELEASE_TAG_TOOL = PROJECT_ROOT / "tools" / "check_release_tag.py"


def test_release_artifact_names_are_v3_and_include_all_public_distribution_forms() -> (
    None
):
    names = (
        "eml_attachment_remover-3.0.0-cp314-none-any.whl",
        "eml_attachment_remover-3.0.0.tar.gz",
        "remove-eml-attachments.pyz",
    )
    assert names == tuple(sorted(names))


def test_release_tag_is_derived_only_from_current_project_metadata(
    capsys: pytest.CaptureFixture[str],
) -> None:
    with (PROJECT_ROOT / "pyproject.toml").open("rb") as project_file:
        version = str(tomllib.load(project_file)["project"]["version"])
    result = subprocess.run(
        [sys.executable, "-B", str(RELEASE_TAG_TOOL), f"v{version}"],
        check=False,
        cwd=PROJECT_ROOT,
        text=True,
        capture_output=True,
    )
    assert result.returncode == 0, result.stderr
    assert result.stdout.strip() == f"v{version}"
    assert check_release_tag.main([f"v{version}"]) == 0
    assert capsys.readouterr().out.strip() == f"v{version}"
    with pytest.raises(SystemExit):
        check_release_tag.main(["v0.0.0"])


def test_release_tag_script_entrypoint_uses_the_same_version_contract(
    monkeypatch: pytest.MonkeyPatch,
) -> None:
    monkeypatch.setattr(sys, "argv", [str(RELEASE_TAG_TOOL), "v3.0.0"])
    with pytest.raises(SystemExit) as result:
        runpy.run_path(str(RELEASE_TAG_TOOL), run_name="__main__")
    assert result.value.code == 0


def test_v3_release_notes_describe_the_corrected_mime_pruned_boundary() -> None:
    notes = PROJECT_ROOT / ".github" / "release-notes" / "v3.0.0.md"
    content = notes.read_text(encoding="utf-8")
    assert content.startswith("# EML Attachment Remover 3.0.0\n")
    assert "MIME-pruned" in content
    assert "regenerate any v2-derived files" in content
    assert "attestation" in content


def test_release_workflow_requires_all_qualification_jobs_before_publication() -> None:
    workflow = (PROJECT_ROOT / ".github" / "workflows" / "release.yml").read_text(
        encoding="utf-8"
    )
    required_needs = "needs:\n      - qualify\n      - mutation\n      - build"
    assert required_needs in workflow
    assert "tools/tasks.py release" in workflow
    assert "--verify-directory release-dist" in workflow
    assert "subject-checksums: release-dist/SHA256SUMS" in workflow
    assert "--verify-tag" in workflow
