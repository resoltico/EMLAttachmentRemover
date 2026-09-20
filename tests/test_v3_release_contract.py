"""Schema-3 release artifact, workflow, and immutable-tag contracts."""

from __future__ import annotations

import re
import runpy
import subprocess
import sys
import tomllib
from pathlib import Path

import pytest
from tools import check_release_tag, qualify_release
from tools.changelog import extract_release

PROJECT_ROOT = Path(__file__).resolve().parents[1]
RELEASE_TAG_TOOL = PROJECT_ROOT / "tools" / "check_release_tag.py"


def test_release_artifact_names_come_from_the_actual_producer() -> None:
    with (PROJECT_ROOT / "pyproject.toml").open("rb") as project_file:
        project = tomllib.load(project_file)["project"]
    name, version = str(project["name"]), str(project["version"])
    normalized = re.sub(r"[-_.]+", "_", name)
    assert set(qualify_release._artifact_names(name, version)) == {
        f"{normalized}-{version}-cp314-none-any.whl",
        f"{normalized}-{version}.tar.gz",
        "remove-eml-attachments.pyz",
    }


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
    with (PROJECT_ROOT / "pyproject.toml").open("rb") as project_file:
        version = str(tomllib.load(project_file)["project"]["version"])
    monkeypatch.setattr(sys, "argv", [str(RELEASE_TAG_TOOL), f"v{version}"])
    with pytest.raises(SystemExit) as result:
        runpy.run_path(str(RELEASE_TAG_TOOL), run_name="__main__")
    assert result.value.code == 0


def test_current_changelog_is_the_release_prose_source() -> None:
    with (PROJECT_ROOT / "pyproject.toml").open("rb") as project_file:
        version = str(tomllib.load(project_file)["project"]["version"])
    body = extract_release(
        (PROJECT_ROOT / "CHANGELOG.md").read_text(encoding="utf-8"), version
    )
    assert not body.startswith(f"## [{version}] - ")
    assert body.startswith("### Changed\n")
    assert body.endswith("\n")


def test_release_workflow_requires_all_qualification_jobs_before_publication() -> None:
    workflow = (PROJECT_ROOT / ".github" / "workflows" / "release.yml").read_text(
        encoding="utf-8"
    )
    required_needs = (
        "needs:\n      - qualify\n      - property\n      - mutation\n      - build"
    )
    assert required_needs in workflow
    assert "tools/tasks.py thorough" in workflow
    assert "--observable --timeout-seconds 3300" in workflow
    assert "tools/qualify_release.py" in workflow
    assert "--output-directory release-dist" in workflow
    assert "--verify-directory release-dist" in workflow
    assert "subject-checksums: release-dist/SHA256SUMS" in workflow
    assert "-m tools.publish_release --assets-directory release-dist" in workflow
    assert "release-notes" not in workflow
