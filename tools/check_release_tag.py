"""Validate that a release tag exactly matches the project version."""

from __future__ import annotations

import argparse
import tomllib
from pathlib import Path
from typing import Final

PROJECT_CONFIG: Final = Path(__file__).resolve().parents[1] / "pyproject.toml"


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
    arguments = parser.parse_args(argv)
    expected = _expected_tag()
    if arguments.tag != expected:
        parser.error(f"tag must be {expected}, not {arguments.tag}")
    print(expected)
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
