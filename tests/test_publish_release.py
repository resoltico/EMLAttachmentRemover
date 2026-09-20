"""Exercise CLI wiring while mocking repository qualification boundaries."""

from __future__ import annotations

import re
import runpy
import sys
from pathlib import Path
from typing import TYPE_CHECKING

import pytest
from tools import publish_release as cli
from tools.changelog import ReleaseError

if TYPE_CHECKING:
    from tools.release_publication import Release

METADATA = '[project]\nversion = "1.2.3"\n'
CHANGELOG = "## [Unreleased]\n\n## [1.2.3] - 2026-09-20\n\n- A change.\n"


@pytest.fixture
def checkout(tmp_path: Path, monkeypatch: pytest.MonkeyPatch) -> Path:
    (tmp_path / "pyproject.toml").write_text(METADATA, encoding="utf-8")
    (tmp_path / "CHANGELOG.md").write_text(CHANGELOG, encoding="utf-8")
    monkeypatch.setattr(cli, "ROOT", tmp_path)
    return tmp_path


@pytest.mark.parametrize(
    ("metadata", "message"),
    [
        ("", "Expected a GitHub JSON object"),
        ("[project]\n", "project.version must be a stable MAJOR.MINOR.PATCH version"),
        (
            '[project]\nversion = "01.2.3"',
            "project.version must be a stable MAJOR.MINOR.PATCH version",
        ),
        (
            "[project]\nversion = 1",
            "project.version must be a stable MAJOR.MINOR.PATCH version",
        ),
    ],
)
def test_invalid_project_metadata(metadata: str, message: str) -> None:
    with pytest.raises(ReleaseError, match="^" + re.escape(message) + "$"):
        cli.project_version(metadata)


def test_check_never_publishes(
    checkout: Path, capsys: pytest.CaptureFixture[str]
) -> None:
    assert cli.main(["--check"]) == 0
    assert capsys.readouterr().out == "- A change.\n"
    (checkout / "CHANGELOG.md").unlink()
    assert cli.main(["--check"]) == 1
    assert "Release failed" in capsys.readouterr().err


def test_cli_requires_exactly_one_publication_mode() -> None:
    with pytest.raises(SystemExit) as result:
        cli.main([])
    assert result.value.code == 2


def test_cli_help_explains_the_changelog_bound_publication_contract(
    capsys: pytest.CaptureFixture[str],
) -> None:
    with pytest.raises(SystemExit) as result:
        cli.main(["--help"])
    assert result.value.code == 0
    assert "Validate changelog prose or publish the current tag's verified assets." in (
        capsys.readouterr().out
    )


@pytest.fixture
def event(monkeypatch: pytest.MonkeyPatch) -> None:
    values = {
        "GITHUB_EVENT_NAME": "push",
        "GITHUB_REF_TYPE": "tag",
        "GITHUB_REPOSITORY": "owner/project",
        "GITHUB_REF_NAME": "v1.2.3",
        "GITHUB_REF": "refs/tags/v1.2.3",
        "GITHUB_SHA": "a" * 40,
    }
    for key, value in values.items():
        monkeypatch.setenv(key, value)


def _git_output(command: list[str], _payload: str = "") -> str:
    arguments = command[3:]
    if arguments == ["rev-parse", "HEAD"]:
        return "a" * 40 + "\n"
    if arguments[0] == "status":
        return ""
    return METADATA if arguments[-1].endswith(":pyproject.toml") else CHANGELOG


@pytest.mark.usefixtures("event")
def test_publish_uses_commit_text_and_existing_artifact_verifier(
    checkout: Path, monkeypatch: pytest.MonkeyPatch, capsys: pytest.CaptureFixture[str]
) -> None:
    candidate = checkout / "package.pyz"
    candidate.write_bytes(b"synthetic")
    verified: list[Path] = []
    plans: list[Release] = []
    commands: list[list[str]] = []

    def verify(directory: Path) -> tuple[Path, ...]:
        verified.append(directory)
        return (candidate,)

    def publish(_api: object, plan: Release) -> int:
        plans.append(plan)
        return 1

    def git(command: list[str], payload: str = "") -> str:
        commands.append(command)
        return _git_output(command, payload)

    monkeypatch.setattr(cli, "run", git)
    monkeypatch.setattr(cli, "verify_release_directory", verify)
    repositories: list[str] = []

    def github_api(repository: str) -> object:
        repositories.append(repository)
        return object()

    monkeypatch.setattr(cli, "GitHubAPI", github_api)
    monkeypatch.setattr(cli, "publish_release", publish)
    (checkout / "CHANGELOG.md").write_text("Do not publish this", encoding="utf-8")
    assert cli.main(["--assets-directory", str(checkout)]) == 0
    assert verified == [checkout]
    assert plans[0].body == "- A change.\n"
    assert plans[0].tag == "v1.2.3"
    assert repositories == ["owner/project"]
    assert plans[0].commit == "a" * 40
    assert len(plans[0].artifacts) == 1
    assert ["show", "a" * 40 + ":pyproject.toml"] in [
        command[3:] for command in commands
    ]
    assert ["show", "a" * 40 + ":CHANGELOG.md"] in [command[3:] for command in commands]
    assert capsys.readouterr().out.strip().endswith("/releases/tag/v1.2.3")


@pytest.mark.usefixtures("event")
def test_publication_requires_porcelain_tracked_file_check(
    checkout: Path, monkeypatch: pytest.MonkeyPatch
) -> None:
    commands: list[list[str]] = []

    def git(command: list[str], payload: str = "") -> str:
        commands.append(command)
        return _git_output(command, payload)

    monkeypatch.setattr(cli, "run", git)
    assert cli.main(["--assets-directory", str(checkout)]) == 1
    assert all(command[0] == "git" for command in commands)
    assert all(command[1:3] == ["-C", str(checkout)] for command in commands)
    assert ["status", "--porcelain", "--untracked-files=no"] in [
        command[3:] for command in commands
    ]


@pytest.mark.parametrize(
    "case",
    [
        ("GITHUB_EVENT_NAME", "workflow_dispatch", "Publication requires a tag push"),
        ("GITHUB_REF_TYPE", "branch", "Publication requires a tag ref"),
        ("GITHUB_REPOSITORY", "", "Invalid repository"),
        ("GITHUB_REF_NAME", "v01.2.3", "Invalid tag"),
        ("GITHUB_REF", "refs/heads/main", "Inconsistent event ref"),
        ("GITHUB_SHA", "bad", "Invalid event commit"),
    ],
)
@pytest.mark.usefixtures("event")
def test_invalid_events_cannot_publish(
    checkout: Path,
    monkeypatch: pytest.MonkeyPatch,
    capsys: pytest.CaptureFixture[str],
    case: tuple[str, str, str],
) -> None:
    key, value, message = case
    monkeypatch.setattr(cli, "run", _git_output)
    monkeypatch.setenv(key, value)
    assert cli.main(["--assets-directory", str(checkout)]) == 1
    assert capsys.readouterr().err == "Release failed: " + message + "\n"


@pytest.mark.usefixtures("event")
@pytest.mark.parametrize(
    ("key", "message"),
    [
        ("GITHUB_REF_NAME", "Invalid tag"),
        ("GITHUB_SHA", "Invalid event commit"),
    ],
)
def test_missing_event_values_cannot_publish(
    checkout: Path,
    monkeypatch: pytest.MonkeyPatch,
    capsys: pytest.CaptureFixture[str],
    key: str,
    message: str,
) -> None:
    monkeypatch.setattr(cli, "run", _git_output)
    monkeypatch.delenv(key)
    assert cli.main(["--assets-directory", str(checkout)]) == 1
    assert capsys.readouterr().err == "Release failed: " + message + "\n"


@pytest.mark.usefixtures("event")
def test_context_rejects_an_absent_repository_before_running_git(
    monkeypatch: pytest.MonkeyPatch,
) -> None:
    monkeypatch.delenv("GITHUB_REPOSITORY")

    def unexpected_git(_command: list[str], _payload: str = "") -> str:
        pytest.fail("repository validation must precede Git execution")

    monkeypatch.setattr(cli, "run", unexpected_git)
    with pytest.raises(ReleaseError, match="^" + re.escape("Invalid repository") + "$"):
        cli._context()  # ruff: ignore[private-member-access] - validation ordering contract.


@pytest.mark.parametrize(
    "case",
    [
        ("checkout", "Checkout/event commit mismatch"),
        ("dirty", "Tracked files changed"),
        ("version", "Tag/project version mismatch"),
        ("qualification", "Archive qualification rejected"),
    ],
)
@pytest.mark.usefixtures("event")
def test_source_and_qualification_failures_cannot_publish(
    checkout: Path,
    monkeypatch: pytest.MonkeyPatch,
    capsys: pytest.CaptureFixture[str],
    case: tuple[str, str],
) -> None:
    fault, message = case

    def output(command: list[str], payload: str = "") -> str:
        if fault == "checkout" and command[3] == "rev-parse":
            return "b" * 40
        if fault == "dirty" and command[3] == "status":
            return " M tools/publish_release.py\n"
        if fault == "version" and command[-1].endswith(":pyproject.toml"):
            return METADATA.replace("1.2.3", "1.2.4")
        return _git_output(command, payload)

    def rejected(_directory: Path) -> tuple[Path, ...]:
        message = "Archive qualification rejected"
        raise RuntimeError(message)

    monkeypatch.setattr(cli, "run", output)
    monkeypatch.setattr(cli, "verify_release_directory", rejected)
    assert cli.main(["--assets-directory", str(checkout)]) == 1
    assert capsys.readouterr().err == "Release failed: " + message + "\n"


def test_script_entrypoint(monkeypatch: pytest.MonkeyPatch) -> None:
    script = Path(cli.__file__)
    root = script.resolve().parents[1]
    read_bytes = Path.read_bytes

    def content(path: Path) -> bytes:
        if path == root / "pyproject.toml":
            return METADATA.encode()
        if path == root / "CHANGELOG.md":
            return CHANGELOG.encode()
        return read_bytes(path)

    monkeypatch.setattr(Path, "read_bytes", content)
    monkeypatch.setattr(sys, "argv", [str(script), "--check"])
    with pytest.raises(SystemExit) as result:
        runpy.run_path(str(script), run_name="__main__")
    assert result.value.code == 0


def test_check_rejects_bare_cr_without_implicit_normalization(checkout: Path) -> None:
    (checkout / "CHANGELOG.md").write_bytes(CHANGELOG.replace("\n", "\r").encode())
    assert cli.main(["--check"]) == 1


def test_check_uses_the_canonical_changelog_case(
    checkout: Path, monkeypatch: pytest.MonkeyPatch
) -> None:
    read_bytes = Path.read_bytes

    def content(path: Path) -> bytes:
        if path == checkout / "pyproject.toml":
            return METADATA.encode()
        if path == checkout / "CHANGELOG.md":
            return CHANGELOG.encode()
        if path.parent == checkout:
            raise FileNotFoundError(path)
        return read_bytes(path)

    monkeypatch.setattr(Path, "read_bytes", content)
    assert cli.main(["--check"]) == 0
