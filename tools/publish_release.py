"""Validate changelog prose or publish the current tag's verified assets."""

from __future__ import annotations

import argparse
import os
import sys
import tomllib
from pathlib import Path
from typing import cast

from tools.changelog import VERSION, extract_release, require
from tools.github_release_api import COMMIT, GitHubAPI, run
from tools.qualify_release import verify_release_directory
from tools.release_publication import Artifact, Release, publish_release, record

ROOT = Path(__file__).resolve().parents[1]


def project_version(metadata: str) -> str:
    """Read the stable release version from project metadata.

    Returns:
        The validated stable version.

    """
    project = record(tomllib.loads(metadata).get("project"))
    value = project.get("version")
    require(
        "project.version must be a stable MAJOR.MINOR.PATCH version",
        condition=isinstance(value, str) and VERSION.fullmatch(value) is not None,
    )
    return cast("str", value)


def _git(*arguments: str) -> str:
    return run(["git", "-C", str(ROOT), *arguments])


def _context() -> tuple[str, str, str]:
    """Validate the exact clean tag-push event context.

    Returns:
        Repository, tag, and tagged commit SHA.

    """
    require(
        "Publication requires a tag push",
        condition=os.environ.get("GITHUB_EVENT_NAME") == "push",
    )
    require(
        "Publication requires a tag ref",
        condition=os.environ.get("GITHUB_REF_TYPE") == "tag",
    )
    repository = os.environ.get("GITHUB_REPOSITORY", "")
    tag = os.environ.get("GITHUB_REF_NAME")
    commit = os.environ.get("GITHUB_SHA")
    require(
        "Invalid tag",
        condition=isinstance(tag, str)
        and tag.startswith("v")
        and VERSION.fullmatch(tag[1:]) is not None,
    )
    require("Invalid repository", condition=bool(repository))
    require(
        "Inconsistent event ref",
        condition=os.environ.get("GITHUB_REF") == f"refs/tags/{tag}",
    )
    require(
        "Invalid event commit",
        condition=isinstance(commit, str) and COMMIT.fullmatch(commit) is not None,
    )
    require(
        "Checkout/event commit mismatch",
        condition=_git("rev-parse", "HEAD").strip() == commit,
    )
    require(
        "Tracked files changed",
        condition=not _git("status", "--porcelain", "--untracked-files=no"),
    )
    return repository, cast("str", tag), cast("str", commit)


def _publish(directory: Path) -> str:
    repository, tag, commit = _context()
    version = project_version(_git("show", f"{commit}:pyproject.toml"))
    require("Tag/project version mismatch", condition=tag == "v" + version)
    body = extract_release(_git("show", f"{commit}:CHANGELOG.md"), version)
    artifacts = tuple(
        Artifact.inspect(path) for path in verify_release_directory(directory)
    )
    publish_release(GitHubAPI(repository), Release(tag, commit, body, artifacts))
    return f"https://github.com/{repository}/releases/tag/{tag}"


def main(argv: list[str] | None = None) -> int:
    """Validate current release prose or publish the exact tagged artifact set.

    Returns:
        Zero on success and one on a reported failure.

    """
    parser = argparse.ArgumentParser(description=__doc__)
    mode = parser.add_mutually_exclusive_group(required=True)
    mode.add_argument("--check", action="store_true")
    mode.add_argument("--assets-directory", type=Path)
    arguments = parser.parse_args(argv)
    try:
        if arguments.check:
            version = project_version(
                (ROOT / "pyproject.toml").read_bytes().decode("utf-8")
            )
            sys.stdout.write(
                extract_release(
                    (ROOT / "CHANGELOG.md").read_bytes().decode("utf-8"), version
                )
            )
        else:
            print(_publish(arguments.assets_directory))
    except (RuntimeError, OSError, ValueError) as error:
        print(f"Release failed: {error}", file=sys.stderr)
        return 1
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
