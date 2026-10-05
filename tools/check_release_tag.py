"""Validate that a release tag exactly matches the project version."""

from __future__ import annotations

import argparse
import shutil
import subprocess
import tomllib
from pathlib import Path
from typing import TYPE_CHECKING, Final

if TYPE_CHECKING:
    from collections.abc import Callable

PROJECT_CONFIG: Final = Path(__file__).resolve().parents[1] / "pyproject.toml"
RELEASE_BRANCH: Final = "refs/remotes/origin/main"


def require_release_source(commit: str, git: Callable[..., str]) -> None:
    """Require full history and a commit reachable from the release branch.

    Raises:
        RuntimeError: If history or release-branch ancestry cannot be established.

    """
    if git("rev-parse", "--is-shallow-repository").strip() != "false":
        message = (
            "Release ancestry requires full history; "
            "fetch origin/main without a depth limit"
        )
        raise RuntimeError(message)
    try:
        git("merge-base", "--is-ancestor", commit, RELEASE_BRANCH)
    except RuntimeError as error:
        message = (
            "Release commit must be reachable from origin/main; fetch full main history"
        )
        raise RuntimeError(message) from error


def _git(*options: str) -> str:
    """Read local release history without a shell or inherited input.

    Returns:
        Git's standard output.

    Raises:
        RuntimeError: If Git fails or the local inspection times out.

    """
    executable = shutil.which("git")
    if executable is None:
        message = "Git is required for release ancestry; install Git"
        raise RuntimeError(message)
    try:
        return subprocess.run(
            [executable, "-C", str(PROJECT_CONFIG.parent), *options],
            input="",
            capture_output=True,
            text=True,
            check=True,
            timeout=30,
        ).stdout
    except (OSError, subprocess.SubprocessError) as error:
        message = (
            "Could not inspect release history; check the checkout and Git installation"
        )
        raise RuntimeError(message) from error


def _project_version() -> str:
    """Read the static project version from the canonical project metadata.

    Returns:
        The release version.

    """
    with PROJECT_CONFIG.open("rb") as project_file:
        metadata = tomllib.load(project_file)
    return str(metadata["project"]["version"])


def _expected_tag() -> str:
    """Return the only release tag accepted for the current project version.

    Returns:
        The version tag prefixed with ``v``.

    """
    return f"v{_project_version()}"


def main(argv: list[str] | None = None) -> int:
    """Validate one proposed release tag.

    Returns:
        Zero when the tag matches the project version.

    """
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("tag", help="release tag to validate")
    parser.add_argument(
        "--require-main",
        action="store_true",
        help="require full history and main ancestry",
    )
    arguments = parser.parse_args(argv)
    expected = _expected_tag()
    if arguments.tag != expected:
        parser.error(f"tag must be {expected}, not {arguments.tag}")
    if arguments.require_main:
        try:
            require_release_source(_git("rev-parse", "HEAD").strip(), _git)
        except RuntimeError as error:
            parser.error(str(error))
    print(expected)
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
