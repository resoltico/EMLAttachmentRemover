# ruff: file-ignore[private-member-access]
"""Exact mutation-sensitive contracts for release qualification."""

from __future__ import annotations

import os
import sys
import tempfile
from pathlib import Path
from unittest.mock import call, patch

import pytest
from tools import qualify_release


def _release_names() -> tuple[str, str, str]:
    """Return one complete public synthetic artifact-name set.

    Returns:
        The wheel, source archive, and zipapp names in lexical order.

    """
    return qualify_release._artifact_names("public-project", "9.8.7")


def test_release_parser_preserves_every_public_option_contract() -> None:
    """Keep the description, defaults, conversions, and help text exact."""
    parser = qualify_release._build_parser()
    output_action = parser._option_string_actions["--output-directory"]
    verify_action = parser._option_string_actions["--verify-directory"]

    assert parser.description == qualify_release.__doc__
    assert output_action.option_strings == ["--output-directory"]
    assert output_action.type is Path
    assert output_action.default == qualify_release.DEFAULT_OUTPUT_DIRECTORY
    assert output_action.help == (
        "candidate output directory "
        f"(default: {qualify_release.DEFAULT_OUTPUT_DIRECTORY})"
    )
    assert verify_action.option_strings == ["--verify-directory"]
    assert verify_action.type is Path
    assert verify_action.default is None
    assert verify_action.help == (
        "verify an existing downloaded candidate directory without building"
    )
    assert vars(parser.parse_args([])) == {
        "output_directory": qualify_release.DEFAULT_OUTPUT_DIRECTORY,
        "verify_directory": None,
    }
    with pytest.raises(SystemExit) as raised:
        parser.parse_args([
            "--output-directory",
            "public-output",
            "--verify-directory",
            "public-candidate",
        ])
    assert raised.value.code == 2


def test_strict_environment_is_exact_with_or_without_removed_inputs() -> None:
    """Remove optional overrides and add only the three strict Python controls."""
    source_environments = (
        {"PUBLIC_SETTING": "kept"},
        {
            "PUBLIC_SETTING": "kept",
            "PYTHONPATH": "untrusted",
            "SOURCE_DATE_EPOCH": "untrusted",
        },
    )
    expected = {
        "PUBLIC_SETTING": "kept",
        "PYTHONDEVMODE": "1",
        "PYTHONNOUSERSITE": "1",
        "PYTHONWARNINGS": "error",
    }

    for environment in source_environments:
        with patch.object(os.environ, "items", return_value=environment.items()):
            assert qualify_release._strict_environment() == expected


def test_build_and_test_forwards_the_exact_verification_and_command_contracts() -> None:
    """Require isolated smoke tests and the canonical zipapp builder invocation."""
    names = _release_names()
    wheel = next(name for name in names if name.endswith(".whl"))
    source = next(name for name in names if name.endswith(".tar.gz"))
    staging = Path("/public/release-staging")
    with (
        patch.object(qualify_release, "_build_reproducible_archives") as build,
        patch.object(qualify_release, "_verify_distribution_files") as verify,
        patch.object(qualify_release, "_run") as run,
    ):
        qualify_release._build_and_test(staging, names)

    build.assert_called_once_with(staging, (wheel, source))
    verify.assert_called_once_with(staging, names, allow_staging=True)
    assert run.call_args_list == [
        call(
            (
                "uv",
                "run",
                "--isolated",
                "--no-project",
                "--python",
                sys.executable,
                "--with",
                str(staging / wheel),
                str(qualify_release.SMOKE_TOOL),
            ),
            timeout_seconds=qualify_release.COMMAND_TIMEOUT_SECONDS,
        ),
        call(
            (
                "uv",
                "run",
                "--isolated",
                "--no-project",
                "--python",
                sys.executable,
                "--with",
                str(staging / source),
                str(qualify_release.SMOKE_TOOL),
            ),
            timeout_seconds=qualify_release.COMMAND_TIMEOUT_SECONDS,
        ),
        call(
            (
                sys.executable,
                str(qualify_release.BUILD_TOOL),
                "--target",
                str(staging / qualify_release.ZIPAPP_FILE_NAME),
            ),
            timeout_seconds=qualify_release.COMMAND_TIMEOUT_SECONDS,
        ),
    ]


def test_reproducibility_builds_are_named_and_confined_to_the_staging_parent() -> None:
    """Keep both independent build directories beside the candidate staging area."""
    archive_names = ("public-9.8.7.whl", "public-9.8.7.tar.gz")
    expected_names = frozenset(archive_names)
    observed_directories: list[Path] = []

    def build(directory: Path, names: frozenset[str]) -> None:
        assert names == expected_names
        observed_directories.append(directory)
        for name in names:
            (directory / name).write_bytes(f"public {name}".encode())

    with tempfile.TemporaryDirectory() as directory:
        base = Path(directory)
        staging = base / ".release-dist.staging"
        staging.mkdir()
        with patch.object(qualify_release, "_build_archive_set", side_effect=build):
            qualify_release._build_reproducible_archives(staging, archive_names)

        assert {path.name for path in staging.iterdir()} == set(archive_names)
        assert [path.parent for path in observed_directories] == [base, base]
        assert observed_directories[0].name.startswith(".release-build-a.")
        assert observed_directories[1].name.startswith(".release-build-b.")
        assert observed_directories[0] != observed_directories[1]
        assert all(not path.exists() for path in observed_directories)


def test_lexical_absolute_allows_a_missing_parent_without_following_the_leaf() -> None:
    """Normalize output paths even before any destination parent exists."""
    with tempfile.TemporaryDirectory() as directory:
        root = Path(directory).resolve()
        with patch.object(Path, "cwd", return_value=root):
            actual = qualify_release._lexical_absolute(
                Path("missing-parent") / "release-dist"
            )

    assert actual == root / "missing-parent" / "release-dist"


def test_publication_rejects_a_broken_symbolic_destination_race() -> None:
    """Never replace a symbolic entry created after staging was reserved."""
    with tempfile.TemporaryDirectory() as directory:
        base = Path(directory)
        staging = base / ".release-dist.staging"
        staging.mkdir()
        output = base / "release-dist"
        output.symlink_to(base / "missing-target", target_is_directory=True)

        with pytest.raises(
            qualify_release.ReleaseQualificationError,
            match="must remain absent or empty",
        ):
            qualify_release._publish_staging(staging, output)

        assert output.is_symlink()
        assert staging.is_dir()
