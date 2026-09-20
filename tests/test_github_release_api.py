"""Verify the GitHub adapter without authenticated requests or release writes."""

from __future__ import annotations

import json
import re
import subprocess
from typing import TYPE_CHECKING

import pytest
from tools import github_release_api as transport
from tools.changelog import ReleaseError
from tools.release_publication import Artifact, Release

if TYPE_CHECKING:
    from pathlib import Path


@pytest.fixture
def api(monkeypatch: pytest.MonkeyPatch) -> transport.GitHubAPI:
    def locate(name: str) -> str:
        assert name == "gh"
        return "/usr/bin/gh"

    monkeypatch.setattr("tools.github_release_api.shutil.which", locate)
    return transport.GitHubAPI("owner/project")


def test_explicit_host_json_stdin_and_no_generated_prose(
    api: transport.GitHubAPI,
    monkeypatch: pytest.MonkeyPatch,
) -> None:
    calls: list[tuple[list[str], str]] = []

    def capture(command: list[str], payload: str = "") -> str:
        calls.append((command, payload))
        return '{"id":1}'

    monkeypatch.setattr(transport, "run", capture)
    desired = Release("v1.2.3", "a" * 40, 'Exact `text` with "quotes"\n', ())
    assert api.create(desired) == {"id": 1}
    command, text = calls.pop()
    assert command[:7] == [
        "/usr/bin/gh",
        "api",
        "repos/owner/project/releases",
        "--hostname",
        "github.com",
        "--method",
        "POST",
    ]
    assert command[command.index("--hostname") + 1] == "github.com"
    assert command[-2:] == ["--input", "-"]
    payload = json.loads(text)
    assert payload == {
        "tag_name": desired.tag,
        "target_commitish": desired.commit,
        "name": desired.title,
        "body": desired.body,
        "draft": True,
        "prerelease": False,
        "generate_release_notes": False,
    }
    api.publish(1)
    publish_command, publish_payload = calls[-1]
    assert publish_command == [
        "/usr/bin/gh",
        "api",
        "repos/owner/project/releases/1",
        "--hostname",
        "github.com",
        "--method",
        "PATCH",
        "--header",
        "Accept: application/vnd.github+json",
        "--header",
        f"X-GitHub-Api-Version: {transport.API_VERSION}",
        "--input",
        "-",
    ]
    assert json.loads(publish_payload) == {"draft": False, "make_latest": "legacy"}


def test_collections_are_paginated(
    api: transport.GitHubAPI,
    monkeypatch: pytest.MonkeyPatch,
) -> None:
    replies = iter([[{"id": index} for index in range(100)], [{"id": 100}]])
    paths: list[str] = []

    def request(method: str, path: str) -> object:
        assert method == "GET"
        paths.append(path)
        return next(replies)

    monkeypatch.setattr(api, "request", request)
    assert len(api.releases()) == 101
    assert paths == [
        "repos/owner/project/releases?per_page=100&page=1",
        "repos/owner/project/releases?per_page=100&page=2",
    ]


def test_get_request_uses_no_json_stdin_payload(
    api: transport.GitHubAPI, monkeypatch: pytest.MonkeyPatch
) -> None:
    calls: list[tuple[list[str], str]] = []

    def capture(command: list[str], payload: str = "") -> str:
        calls.append((command, payload))
        return "{}"

    monkeypatch.setattr(transport, "run", capture)
    assert api.request("GET", "repos/owner/project/releases/1") == {}
    command, payload = calls[0]
    assert "--input" not in command
    assert not payload


@pytest.mark.parametrize("nested", [False, True])
def test_lightweight_and_annotated_tags(
    api: transport.GitHubAPI,
    monkeypatch: pytest.MonkeyPatch,
    *,
    nested: bool,
) -> None:
    commit = {"object": {"type": "commit", "sha": "a" * 40}}
    replies = iter(
        [{"object": {"type": "tag", "sha": "b" * 40}}, commit] if nested else [commit]
    )
    requests: list[tuple[object, ...]] = []

    def request(*arguments: object) -> object:
        requests.append(arguments)
        return next(replies)

    monkeypatch.setattr(api, "request", request)
    assert api.tag_commit("v1.2.3") == "a" * 40
    assert requests[0] == ("GET", "repos/owner/project/git/ref/tags/v1.2.3")
    if nested:
        assert requests[1] == ("GET", "repos/owner/project/git/tags/" + "b" * 40)


@pytest.mark.parametrize(
    "target",
    [
        None,
        {"type": "tree", "sha": "a" * 40},
        {"type": "commit", "sha": "bad"},
        {"type": "commit", "sha": None},
        {"type": "tag", "sha": "a" * 40},
    ],
)
def test_invalid_or_cyclic_tags_fail(
    api: transport.GitHubAPI,
    monkeypatch: pytest.MonkeyPatch,
    target: object,
) -> None:
    monkeypatch.setattr(api, "request", lambda *_args: {"object": target})
    with pytest.raises(ReleaseError):
        api.tag_commit("v1.2.3")


def test_annotated_tag_depth_has_an_exact_public_error(
    api: transport.GitHubAPI, monkeypatch: pytest.MonkeyPatch
) -> None:
    annotated = {"object": {"type": "tag", "sha": "a" * 40}}
    monkeypatch.setattr(api, "request", lambda *_args: annotated)
    with pytest.raises(
        ReleaseError,
        match="^"
        + re.escape("Annotated tag nesting exceeds the supported depth")
        + "$",
    ):
        api.tag_commit("v1.2.3")


def test_tag_object_sha_must_be_a_full_commit_hash(
    api: transport.GitHubAPI, monkeypatch: pytest.MonkeyPatch
) -> None:
    monkeypatch.setattr(
        api,
        "request",
        lambda *_args: {"object": {"type": "commit", "sha": "bad"}},
    )
    with pytest.raises(
        ReleaseError,
        match="^" + re.escape("Invalid remote tag object SHA") + "$",
    ):
        api.tag_commit("v1.2.3")


def test_tag_object_must_resolve_through_a_tag_or_commit(
    api: transport.GitHubAPI, monkeypatch: pytest.MonkeyPatch
) -> None:
    monkeypatch.setattr(
        api,
        "request",
        lambda *_args: {"object": {"type": "tree", "sha": "a" * 40}},
    )
    with pytest.raises(
        ReleaseError,
        match="^" + re.escape("Tag does not resolve to a commit") + "$",
    ):
        api.tag_commit("v1.2.3")


def test_asset_reads_and_upload_have_no_overwrite_flag(
    api: transport.GitHubAPI,
    monkeypatch: pytest.MonkeyPatch,
    tmp_path: Path,
) -> None:
    commands: list[list[str]] = []

    def capture(command: list[str], _payload: str = "") -> str:
        commands.append(command)
        return '[{"id":1}]' if "/assets?" in command[2] else '{"id":1}'

    monkeypatch.setattr(transport, "run", capture)
    assert api.release(1) == {"id": 1}
    assert commands[0][:7] == [
        "/usr/bin/gh",
        "api",
        "repos/owner/project/releases/1",
        "--hostname",
        "github.com",
        "--method",
        "GET",
    ]
    assert api.assets(1) == [{"id": 1}]
    path = tmp_path / "package.pyz"
    path.write_bytes(b"synthetic")
    api.upload("v1.2.3", Artifact.inspect(path))
    assert "--clobber" not in commands[-1]
    assert commands[-1] == [
        "/usr/bin/gh",
        "release",
        "upload",
        "v1.2.3",
        str(path),
        "--repo",
        "github.com/owner/project",
    ]


def test_bad_transport_responses_and_missing_cli_fail(
    api: transport.GitHubAPI,
    monkeypatch: pytest.MonkeyPatch,
) -> None:
    monkeypatch.setattr(transport, "run", lambda *_args: "not JSON")
    with pytest.raises(
        ReleaseError, match="^" + re.escape("GitHub returned invalid JSON") + "$"
    ):
        api.release(1)
    monkeypatch.setattr(api, "request", lambda *_args: {"unexpected": "collection"})
    with pytest.raises(
        ReleaseError, match="^" + re.escape("Expected a GitHub JSON collection") + "$"
    ):
        api.releases()
    monkeypatch.setattr("tools.github_release_api.shutil.which", lambda _: None)
    with pytest.raises(
        ReleaseError,
        match="^" + re.escape("GitHub CLI is required for publication") + "$",
    ):
        transport.GitHubAPI("owner/project")
    with pytest.raises(
        ReleaseError, match="^" + re.escape("Invalid repository name") + "$"
    ):
        transport.GitHubAPI("owner/project/other")


def test_subprocess_environment_and_stdin_are_controlled(
    monkeypatch: pytest.MonkeyPatch,
) -> None:
    monkeypatch.setenv("GH_DEBUG", "api")

    def execute(
        command: list[str],
        **options: object,
    ) -> subprocess.CompletedProcess[bytes]:
        assert command == ["/usr/bin/gh"]
        assert options["input"] == b"payload"
        assert options["capture_output"] is True
        assert options["check"] is False
        assert options["timeout"] == transport.COMMAND_TIMEOUT_SECONDS
        environment = options["env"]
        assert isinstance(environment, dict)
        assert "GH_DEBUG" not in environment
        assert environment["GH_PROMPT_DISABLED"] == "1"
        assert "shell" not in options
        return subprocess.CompletedProcess(command, 0, b"result\r\nraw\rbytes", b"")

    monkeypatch.setattr("tools.github_release_api.subprocess.run", execute)
    assert transport.run(["/usr/bin/gh"], "payload") == "result\r\nraw\rbytes"


@pytest.mark.parametrize(
    ("failure", "message"),
    [
        (
            OSError("missing"),
            "Command failed or timed out; rerun to reconcile remote state",
        ),
        (
            subprocess.TimeoutExpired("gh", 120),
            "Command failed or timed out; rerun to reconcile remote state",
        ),
        (None, "Command failed; remote writes may need reconciliation"),
    ],
)
def test_uncertain_commands_are_not_retried(
    monkeypatch: pytest.MonkeyPatch,
    failure: Exception | None,
    message: str,
) -> None:
    calls: list[str] = []

    def execute(
        *_args: object, **options: object
    ) -> subprocess.CompletedProcess[bytes]:
        calls.append("run")
        assert options["input"] == b""
        if failure is not None:
            raise failure
        return subprocess.CompletedProcess(["gh"], 1, b"", b"private diagnostic")

    monkeypatch.setattr("tools.github_release_api.subprocess.run", execute)
    with pytest.raises(ReleaseError, match="^" + message + "$"):
        transport.run(["/usr/bin/gh"])
    assert calls == ["run"]


def test_invalid_utf8_command_output_fails(monkeypatch: pytest.MonkeyPatch) -> None:
    monkeypatch.setattr(
        "tools.github_release_api.subprocess.run",
        lambda *_args, **_kwargs: subprocess.CompletedProcess(["gh"], 0, b"\xff", b""),
    )
    with pytest.raises(
        ReleaseError,
        match="^" + re.escape("Command returned non-UTF-8 output") + "$",
    ):
        transport.run(["/usr/bin/gh"])
