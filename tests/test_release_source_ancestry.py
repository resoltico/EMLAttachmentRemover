"""Release policy crosses real Git history before any publication boundary."""

from __future__ import annotations

import importlib
import shutil
import subprocess
from pathlib import Path
from unittest.mock import Mock

import pytest
from tools import check_release_tag, publish_release


def git(directory: Path, *arguments: str) -> str:
    """Run real Git to construct or inspect an independent history oracle.

    Returns:
        Git's stripped standard output.

    """
    executable = shutil.which("git")
    assert executable is not None
    return subprocess.run(
        [executable, "-C", str(directory), *arguments],
        check=True,
        capture_output=True,
        text=True,
        timeout=30,
    ).stdout.strip()


@pytest.fixture
def history(tmp_path: Path, monkeypatch: pytest.MonkeyPatch) -> tuple[Path, str]:
    metadata = tmp_path / "pyproject.toml"
    metadata.write_text('[project]\nversion = "1.2.3"\n')
    monkeypatch.setattr(check_release_tag, "PROJECT_CONFIG", metadata)

    git(tmp_path, "init", "-b", "main")
    git(tmp_path, "config", "user.name", "Synthetic release test")
    git(tmp_path, "config", "user.email", "release@example.test")
    git(tmp_path, "add", "pyproject.toml")
    git(tmp_path, "commit", "-m", "Initial public metadata")
    first = git(tmp_path, "rev-parse", "HEAD")
    (tmp_path / "content").write_text("integrated")
    git(tmp_path, "add", "content")
    git(tmp_path, "commit", "-m", "Integrated content")
    git(tmp_path, "update-ref", "refs/remotes/origin/main", "HEAD")
    return tmp_path, first


@pytest.mark.parametrize("historical", [False, True])
def test_release_accepts_tip_and_already_integrated_history(
    history: tuple[Path, str], capsys: pytest.CaptureFixture[str], *, historical: bool
) -> None:
    directory, first = history
    if historical:
        git(directory, "checkout", "--detach", first)
    assert check_release_tag.main(["v1.2.3", "--require-main"]) == 0
    assert capsys.readouterr().out == "v1.2.3\n"


def _unmerged_branch(history: tuple[Path, str]) -> str:
    directory, first = history
    git(history[0], "checkout", "-b", "topic", first)
    (directory / "unmerged").write_text("not integrated")
    git(history[0], "add", "unmerged")
    git(history[0], "commit", "-m", "Unmerged content")
    return git(history[0], "rev-parse", "HEAD").strip()


def test_release_rejects_unmerged_branch(
    history: tuple[Path, str], capsys: pytest.CaptureFixture[str]
) -> None:
    _unmerged_branch(history)
    with pytest.raises(SystemExit) as captured:
        check_release_tag.main(["v1.2.3", "--require-main"])
    assert captured.value.code == 2
    assert capsys.readouterr().err.splitlines()[-1].partition(": error: ")[2] == (
        "Release commit must be reachable from origin/main; fetch full main history"
    )


def test_release_requires_existing_main_history(
    history: tuple[Path, str], capsys: pytest.CaptureFixture[str]
) -> None:
    git(history[0], "update-ref", "-d", "refs/remotes/origin/main")
    with pytest.raises(SystemExit) as captured:
        check_release_tag.main(["v1.2.3", "--require-main"])
    assert captured.value.code == 2
    assert capsys.readouterr().err.splitlines()[-1].partition(": error: ")[2] == (
        "Release commit must be reachable from origin/main; fetch full main history"
    )


def test_release_rejects_shallow_history(
    history: tuple[Path, str],
    monkeypatch: pytest.MonkeyPatch,
    capsys: pytest.CaptureFixture[str],
) -> None:
    directory, _first = history
    shallow = directory / "shallow"
    git(history[0], "clone", "--depth", "1", directory.as_uri(), str(shallow))
    monkeypatch.setattr(check_release_tag, "PROJECT_CONFIG", shallow / "pyproject.toml")
    assert (shallow / ".git/shallow").is_file()
    with pytest.raises(SystemExit) as captured:
        check_release_tag.main(["v1.2.3", "--require-main"])
    assert captured.value.code == 2
    assert capsys.readouterr().err.splitlines()[-1].partition(": error: ")[2] == (
        "Release ancestry requires full history; "
        "fetch origin/main without a depth limit"
    )


def test_publisher_rejects_unmerged_source_before_verifying_or_writing(
    history: tuple[Path, str],
    monkeypatch: pytest.MonkeyPatch,
    capsys: pytest.CaptureFixture[str],
) -> None:
    directory, _first = history
    candidate = _unmerged_branch(history)
    monkeypatch.setattr(publish_release, "ROOT", directory)
    for key, value in {
        "GITHUB_EVENT_NAME": "push",
        "GITHUB_REF_TYPE": "tag",
        "GITHUB_REPOSITORY": "owner/project",
        "GITHUB_REF_NAME": "v1.2.3",
        "GITHUB_REF": "refs/tags/v1.2.3",
        "GITHUB_SHA": candidate,
    }.items():
        monkeypatch.setenv(key, value)
    verifier = Mock()
    remote = Mock()
    monkeypatch.setattr(publish_release, "verify_release_directory", verifier)
    monkeypatch.setattr(publish_release, "GitHubAPI", remote)
    assert publish_release.main(["--assets-directory", str(directory)]) == 1
    assert capsys.readouterr().err == (
        "Release failed: Release commit must be reachable from origin/main; "
        "fetch full main history\n"
    )
    verifier.assert_not_called()
    remote.assert_not_called()


@pytest.mark.parametrize("fault", ["missing", "removed", "timeout"])
def test_release_history_failures_are_actionable_cli_errors(
    history: tuple[Path, str],
    fault: str,
    monkeypatch: pytest.MonkeyPatch,
    capsys: pytest.CaptureFixture[str],
) -> None:
    directory, _first = history
    if fault == "missing":
        monkeypatch.setattr(shutil, "which", lambda _name: None)
    elif fault == "removed":
        monkeypatch.setattr(
            shutil,
            "which",
            lambda _name: str(directory / "absent-git"),
        )
    else:
        monkeypatch.setattr(
            subprocess,
            "run",
            Mock(side_effect=subprocess.TimeoutExpired("git", 30)),
        )
    with pytest.raises(SystemExit) as captured:
        check_release_tag.main(["v1.2.3", "--require-main"])
    assert captured.value.code == 2
    message = capsys.readouterr().err
    expected = (
        "Git is required for release ancestry; install Git"
        if fault == "missing"
        else (
            "Could not inspect release history; check the checkout and Git installation"
        )
    )
    assert message.splitlines()[-1].partition(": error: ")[2] == expected


def test_release_workflow_fetches_full_history_and_checks_ancestry_early() -> None:
    workflow = Path(__file__).resolve().parents[1] / ".github/workflows/release.yml"
    jobs = importlib.import_module("yaml").safe_load(workflow.read_text())["jobs"]
    for name, job in jobs.items():
        for step in job.get("steps", []):
            if step.get("uses", "").startswith("actions/checkout@"):
                assert step["with"]["fetch-depth"] == 0
        if name not in {"qualify", "property", "mutation", "build", "publish"}:
            continue
        validations = [
            s for s in job["steps"] if "check_release_tag" in s.get("run", "")
        ]
        assert len(validations) == 1
        assert "--require-main" in validations[0]["run"]
        assert "--no-project" in validations[0]["run"]
        validation = job["steps"].index(validations[0])
        for index, step in enumerate(job["steps"]):
            if (
                "uv sync" in step.get("run", "")
                or "release_delivery" in step.get("run", "")
                or step.get("uses", "").startswith("actions/attest@")
            ):
                assert validation < index


def test_release_git_reads_are_bounded_noninteractive_real_processes(
    history: tuple[Path, str],
    monkeypatch: pytest.MonkeyPatch,
) -> None:
    assert history[0].is_dir()
    observer = Mock(wraps=subprocess.run)
    monkeypatch.setattr(subprocess, "run", observer)
    assert check_release_tag.main(["v1.2.3", "--require-main"]) == 0
    assert len(observer.call_args_list) == 3
    for call in observer.call_args_list:
        assert isinstance(call.kwargs["input"], str)
        assert not call.kwargs["input"]
        assert call.kwargs["timeout"] == 30
        assert call.kwargs["check"] is True
        assert call.kwargs["capture_output"] is True
        assert call.kwargs["text"] is True


def test_release_help_explains_authoritative_history_check(
    capsys: pytest.CaptureFixture[str],
) -> None:
    with pytest.raises(SystemExit) as captured:
        check_release_tag.main(["--help"])
    assert captured.value.code == 0
    lines = [" ".join(line.split()) for line in capsys.readouterr().out.splitlines()]
    assert [line for line in lines if line.startswith("--require-main")] == [
        "--require-main require full history and main ancestry"
    ]
