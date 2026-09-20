"""Use GitHub CLI as the bounded authenticated release transport."""

from __future__ import annotations

import json
import os
import re
import shutil
import subprocess
from typing import TYPE_CHECKING, cast

from tools.changelog import ReleaseError, require
from tools.release_publication import record

if TYPE_CHECKING:
    from collections.abc import Sequence

    from tools.release_publication import Artifact, Record, Release

API_VERSION = "2026-03-10"
PAGE_SIZE = 100
MAX_TAG_DEPTH = 8
COMMAND_TIMEOUT_SECONDS = 120
REPOSITORY = re.compile(r"[A-Za-z0-9_.-]+/[A-Za-z0-9_.-]+")
COMMIT = re.compile(r"[0-9a-f]{40}")


def run(command: Sequence[str], payload: str = "") -> str:
    """Run one command without a shell or inherited stdin.

    Returns:
        Standard output decoded as UTF-8.

    Raises:
        ReleaseError: If a command fails, times out, or emits invalid UTF-8.

    """
    environment = dict(os.environ)
    environment.pop("GH_DEBUG", None)
    environment["GH_PROMPT_DISABLED"] = "1"
    try:
        result = subprocess.run(
            command,
            input=payload.encode("utf-8"),
            capture_output=True,
            check=False,
            timeout=COMMAND_TIMEOUT_SECONDS,
            env=environment,
        )
    except (OSError, subprocess.TimeoutExpired) as error:
        message = "Command failed or timed out; rerun to reconcile remote state"
        raise ReleaseError(message) from error
    require(
        "Command failed; remote writes may need reconciliation",
        condition=result.returncode == 0,
    )
    try:
        return result.stdout.decode("utf-8")
    except UnicodeDecodeError as error:
        message = "Command returned non-UTF-8 output"
        raise ReleaseError(message) from error


def _records(value: object) -> list[Record]:
    require("Expected a GitHub JSON collection", condition=isinstance(value, list))
    return [record(item) for item in cast("list[object]", value)]


class _GitHubAPI:
    """Address github.com explicitly and never overwrite release assets."""

    def __init__(self, repository: str) -> None:
        require(
            "Invalid repository name",
            condition=REPOSITORY.fullmatch(repository) is not None,
        )
        executable = shutil.which("gh")
        require(
            "GitHub CLI is required for publication", condition=executable is not None
        )
        self.repository = repository
        self.executable = cast("str", executable)
        self.base = f"repos/{repository}"

    def request(self, method: str, path: str, payload: Record | None = None) -> object:
        """Perform one JSON GitHub REST request.

        Returns:
            Decoded JSON response.

        Raises:
            ReleaseError: If GitHub returns malformed JSON.

        """
        command = [
            self.executable,
            "api",
            path,
            "--hostname",
            "github.com",
            "--method",
            method,
            "--header",
            "Accept: application/vnd.github+json",
            "--header",
            f"X-GitHub-Api-Version: {API_VERSION}",
        ]
        if payload is not None:
            command.extend(["--input", "-"])
        try:
            return json.loads(
                run(command, json.dumps(payload) if payload is not None else "")
            )
        except json.JSONDecodeError as error:
            message = "GitHub returned invalid JSON"
            raise ReleaseError(message) from error

    def collection(self, path: str) -> list[Record]:
        """Return every page of one GitHub collection.

        Returns:
            Every JSON object in the collection.

        """
        result: list[Record] = []
        page = 1
        while True:
            batch = _records(
                self.request("GET", f"{path}?per_page={PAGE_SIZE}&page={page}")
            )
            result.extend(batch)
            if len(batch) < PAGE_SIZE:
                return result
            page += 1

    def tag_commit(self, tag: str) -> str:
        """Peel a remote lightweight or annotated tag to its commit.

        Returns:
            The resolved commit SHA.

        Raises:
            ReleaseError: If the tag does not resolve within the bounded depth.

        """
        reference = record(self.request("GET", f"{self.base}/git/ref/tags/{tag}"))
        target = record(reference.get("object"))
        for _ in range(MAX_TAG_DEPTH):
            sha = target.get("sha")
            require(
                "Invalid remote tag object SHA",
                condition=isinstance(sha, str) and COMMIT.fullmatch(sha) is not None,
            )
            if target.get("type") == "commit":
                return cast("str", sha)
            require(
                "Tag does not resolve to a commit",
                condition=target.get("type") == "tag",
            )
            annotated = record(self.request("GET", f"{self.base}/git/tags/{sha}"))
            target = record(annotated.get("object"))
        message = "Annotated tag nesting exceeds the supported depth"
        raise ReleaseError(message)

    def releases(self) -> list[Record]:
        return self.collection(f"{self.base}/releases")

    def release(self, release_id: int) -> Record:
        return record(self.request("GET", f"{self.base}/releases/{release_id}"))

    def assets(self, release_id: int) -> list[Record]:
        return self.collection(f"{self.base}/releases/{release_id}/assets")

    def create(self, desired: Release) -> Record:
        return record(
            self.request(
                "POST",
                f"{self.base}/releases",
                {
                    "tag_name": desired.tag,
                    "target_commitish": desired.commit,
                    "name": desired.title,
                    "body": desired.body,
                    "draft": True,
                    "prerelease": False,
                    "generate_release_notes": False,
                },
            )
        )

    def upload(self, tag: str, artifact: Artifact) -> None:
        run([
            self.executable,
            "release",
            "upload",
            tag,
            str(artifact.path),
            "--repo",
            f"github.com/{self.repository}",
        ])

    def publish(self, release_id: int) -> None:
        self.request(
            "PATCH",
            f"{self.base}/releases/{release_id}",
            {"draft": False, "make_latest": "legacy"},
        )


GitHubAPI = _GitHubAPI
