"""Build and qualify a portable local release-candidate artifact set."""

from __future__ import annotations

import argparse
import importlib
import os
import re
import shutil
import subprocess
import sys
import tempfile
import tomllib
from pathlib import Path
from typing import TYPE_CHECKING, Final, Protocol, cast

if TYPE_CHECKING:
    from collections.abc import Sequence

    from tools.check_repository_hygiene import HygieneIssue


class ReleaseToolModule(Protocol):
    """Describe repository release-support modules loaded portably."""

    DistributionArchiveError: type[RuntimeError]
    public_files: tuple[Path, ...]
    issues: tuple[HygieneIssue, ...]

    def audit_repository(
        self,
        root: Path,
    ) -> ReleaseToolModule:
        """Return a fresh repository audit."""

    def diagnostics(self, root: Path) -> tuple[str, ...]:
        """Return deterministic issue diagnostics."""

    def verify_distribution_archives(
        self,
        source_archive: Path,
        wheel: Path,
        repository_root: Path,
        project_config: Path,
        public_files: Sequence[Path],
    ) -> None:
        """Verify the exact source and wheel archive contracts."""


class ReleaseFilesModule(Protocol):
    """Describe release filesystem helpers loaded portably."""

    ReleaseQualificationError: type[RuntimeError]
    CHECKSUM_FILE_NAME: str

    def prepare_output_directory(self, output: Path) -> Path:
        """Validate an output and create its staging directory."""

    def lexical_absolute(self, path: Path) -> Path:
        """Normalize a path without following its last component."""

    def assert_exact_entries(
        self,
        directory: Path,
        expected_names: frozenset[str],
    ) -> None:
        """Require an exact regular-file set."""

    def sha256(self, path: Path) -> str:
        """Hash one complete artifact."""

    def manifest_text(self, directory: Path, artifact_names: Sequence[str]) -> str:
        """Render a checksum manifest."""

    def manifest_bytes(
        self,
        directory: Path,
        artifact_names: Sequence[str],
    ) -> bytes:
        """Render a checksum manifest in its canonical wire encoding."""

    def write_and_verify_manifest(
        self,
        directory: Path,
        artifact_names: Sequence[str],
    ) -> Path:
        """Write and verify a checksum manifest."""

    def publish_staging(self, staging: Path, output: Path) -> None:
        """Atomically publish a staging directory."""


check_repository_hygiene = cast(
    "ReleaseToolModule",
    importlib.import_module(
        "tools.check_repository_hygiene" if __package__ else "check_repository_hygiene",
    ),
)
verify_distribution_archives = cast(
    "ReleaseToolModule",
    importlib.import_module(
        "tools.verify_distribution_archives"
        if __package__
        else "verify_distribution_archives",
    ),
)
release_files = cast(
    "ReleaseFilesModule",
    importlib.import_module(
        "tools.release_files" if __package__ else "release_files",
    ),
)

PROJECT_ROOT: Final = Path(__file__).resolve().parents[1]
PROJECT_CONFIG: Final = PROJECT_ROOT / "pyproject.toml"
BUILD_TOOL: Final = PROJECT_ROOT / "tools" / "build_zipapp.py"
SMOKE_TOOL: Final = PROJECT_ROOT / "tools" / "smoke_distribution.py"
DEFAULT_OUTPUT_DIRECTORY: Final = PROJECT_ROOT / "release-dist"
CHECKSUM_FILE_NAME = release_files.CHECKSUM_FILE_NAME
ZIPAPP_FILE_NAME: Final = "remove-eml-attachments.pyz"
UV_MARKER_NAME: Final = ".gitignore"
COMMAND_TIMEOUT_SECONDS: Final = 600
ReleaseQualificationError = release_files.ReleaseQualificationError
_prepare_output_directory = release_files.prepare_output_directory
_lexical_absolute = release_files.lexical_absolute
_assert_exact_entries = release_files.assert_exact_entries
_sha256 = release_files.sha256
_manifest_text = release_files.manifest_text
_manifest_bytes = release_files.manifest_bytes
_write_and_verify_manifest = release_files.write_and_verify_manifest
_publish_staging = release_files.publish_staging


def _build_parser() -> argparse.ArgumentParser:
    """Create the release-qualification command-line parser.

    Returns:
        The configured argument parser.

    """
    parser = argparse.ArgumentParser(description=__doc__)
    mode = parser.add_mutually_exclusive_group()
    mode.add_argument(
        "--output-directory",
        type=Path,
        default=DEFAULT_OUTPUT_DIRECTORY,
        help=f"candidate output directory (default: {DEFAULT_OUTPUT_DIRECTORY})",
    )
    mode.add_argument(
        "--verify-directory",
        type=Path,
        help="verify an existing downloaded candidate directory without building",
    )
    return parser


def _project_identity() -> tuple[str, str]:
    """Read the canonical project name and version.

    Returns:
        The distribution name and static version from ``pyproject.toml``.

    """
    with PROJECT_CONFIG.open("rb") as project_file:
        metadata = tomllib.load(project_file)["project"]
    return str(metadata["name"]), str(metadata["version"])


def _artifact_names(name: str, version: str) -> tuple[str, str, str]:
    """Return the exact expected wheel, source archive, and zipapp names.

    Returns:
        Version-derived artifact basenames in lexical order.

    """
    normalized_name = re.sub(r"[-_.]+", "_", name)
    names = (
        f"{normalized_name}-{version}-cp314-none-any.whl",
        f"{normalized_name}-{version}.tar.gz",
        ZIPAPP_FILE_NAME,
    )
    first, second, third = sorted(names)
    return first, second, third


def _strict_environment() -> dict[str, str]:
    """Return a strict build environment without source import overrides.

    Returns:
        A fresh child-process environment.

    """
    removed_names = {"PYTHONPATH", "SOURCE_DATE_EPOCH"}
    environment = {
        name: value for name, value in os.environ.items() if name not in removed_names
    }
    environment["PYTHONDEVMODE"] = "1"
    environment["PYTHONNOUSERSITE"] = "1"
    environment["PYTHONWARNINGS"] = "error"
    return environment


def _run(command: Sequence[str], *, timeout_seconds: float) -> None:
    """Run one bounded release-qualification subprocess."""
    subprocess.run(
        command,
        check=True,
        cwd=PROJECT_ROOT,
        env=_strict_environment(),
        timeout=timeout_seconds,
    )


def _build_and_test(staging: Path, artifact_names: tuple[str, str, str]) -> None:
    """Build and smoke-test the three expected release candidates."""
    wheel_name = next(name for name in artifact_names if name.endswith(".whl"))
    source_name = next(name for name in artifact_names if name.endswith(".tar.gz"))
    _build_reproducible_archives(staging, (wheel_name, source_name))
    _verify_distribution_files(staging, artifact_names, allow_staging=True)
    for artifact_name in (wheel_name, source_name):
        _run(
            (
                "uv",
                "run",
                "--isolated",
                "--no-project",
                "--python",
                sys.executable,
                "--with",
                str(staging / artifact_name),
                str(SMOKE_TOOL),
            ),
            timeout_seconds=COMMAND_TIMEOUT_SECONDS,
        )
    _run(
        (
            sys.executable,
            str(BUILD_TOOL),
            "--target",
            str(staging / ZIPAPP_FILE_NAME),
        ),
        timeout_seconds=COMMAND_TIMEOUT_SECONDS,
    )


def _build_archive_set(directory: Path, expected_names: frozenset[str]) -> None:
    """Build one isolated wheel/source pair and validate its exact surface."""
    _run(
        (
            "uv",
            "build",
            "--no-build-isolation",
            "--no-sources",
            "--out-dir",
            str(directory),
        ),
        timeout_seconds=COMMAND_TIMEOUT_SECONDS,
    )
    (directory / UV_MARKER_NAME).unlink(missing_ok=True)
    _assert_exact_entries(directory, expected_names)


def _build_reproducible_archives(
    staging: Path,
    archive_names: tuple[str, str],
) -> None:
    """Require byte-identical artifacts from two independent build directories."""
    expected_names = frozenset(archive_names)
    with (
        tempfile.TemporaryDirectory(
            dir=staging.parent,
            prefix=".release-build-a.",
        ) as first_directory,
        tempfile.TemporaryDirectory(
            dir=staging.parent,
            prefix=".release-build-b.",
        ) as second_directory,
    ):
        first = Path(first_directory)
        second = Path(second_directory)
        _build_archive_set(first, expected_names)
        _build_archive_set(second, expected_names)
        mismatches = [
            name
            for name in sorted(expected_names)
            if _sha256(first / name) != _sha256(second / name)
        ]
        if mismatches:
            message = f"release builds are not byte reproducible: {mismatches}"
            raise ReleaseQualificationError(message)
        for name in sorted(expected_names):
            (first / name).replace(staging / name)


def _complete_staging(
    staging: Path,
    artifact_names: tuple[str, str, str],
) -> frozenset[str]:
    """Complete and verify the staged artifact set.

    Returns:
        The exact final basenames approved for publication.

    """
    _build_and_test(staging, artifact_names)
    _assert_exact_entries(staging, frozenset(artifact_names))
    _write_and_verify_manifest(staging, artifact_names)
    final_names = frozenset((*artifact_names, CHECKSUM_FILE_NAME))
    _assert_exact_entries(staging, final_names)
    return final_names


def verify_release_directory(directory: Path) -> tuple[Path, ...]:
    """Verify the exact artifact set and checksum bytes in one directory.

    Returns:
        The four verified paths in lexical basename order.

    """
    candidate = _lexical_absolute(directory)
    if candidate.is_symlink() or not candidate.is_dir():
        message = f"release candidate must be a non-symbolic directory: {candidate}"
        raise ReleaseQualificationError(message)
    artifact_names = _artifact_names(*_project_identity())
    final_names = frozenset((*artifact_names, CHECKSUM_FILE_NAME))
    _assert_exact_entries(candidate, final_names)
    manifest = candidate / CHECKSUM_FILE_NAME
    expected = _manifest_bytes(candidate, artifact_names)
    if manifest.read_bytes() != expected:
        message = f"release checksum manifest failed verification: {manifest}"
        raise ReleaseQualificationError(message)
    _verify_distribution_files(candidate, artifact_names, allow_staging=False)
    return tuple(candidate / name for name in sorted(final_names))


def _verify_distribution_files(
    directory: Path,
    artifact_names: tuple[str, str, str],
    *,
    allow_staging: bool,
) -> None:
    """Verify build archives against a fresh audited repository snapshot."""
    audit = check_repository_hygiene.audit_repository(PROJECT_ROOT)
    is_staging = allow_staging and directory.parent == PROJECT_ROOT
    remaining_issues = tuple(
        issue
        for issue in audit.issues
        if not is_staging or not issue.path.is_relative_to(directory)
    )
    if remaining_issues:
        diagnostics = "\n".join(
            f"{issue.path.relative_to(PROJECT_ROOT)}: {issue.message}"
            for issue in remaining_issues
        )
        message = f"repository changed during release build:\n{diagnostics}"
        raise ReleaseQualificationError(message)
    wheel_name = next(name for name in artifact_names if name.endswith(".whl"))
    source_name = next(name for name in artifact_names if name.endswith(".tar.gz"))
    try:
        verify_distribution_archives.verify_distribution_archives(
            directory / source_name,
            directory / wheel_name,
            PROJECT_ROOT,
            PROJECT_CONFIG,
            audit.public_files,
        )
    except verify_distribution_archives.DistributionArchiveError as error:
        raise ReleaseQualificationError(str(error)) from error


def qualify_release(output_directory: Path) -> tuple[Path, ...]:
    """Build, qualify, and atomically publish one local candidate set.

    Returns:
        The four validated artifact and manifest paths.

    Raises:
        BaseExceptionGroup: If qualification and cleanup both fail.

    """
    output = _lexical_absolute(output_directory)
    artifact_names = _artifact_names(*_project_identity())
    staging = _prepare_output_directory(output)
    try:
        final_names = _complete_staging(staging, artifact_names)
        _publish_staging(staging, output)
    except BaseException as qualification_error:
        cleanup_error: Exception | None = None
        try:
            shutil.rmtree(staging)
        except OSError as error:
            cleanup_error = error
        if cleanup_error is None and (staging.exists() or staging.is_symlink()):
            message = f"release staging cleanup was incomplete: {staging}"
            cleanup_error = ReleaseQualificationError(message)
        if cleanup_error is not None:
            message = "release qualification failed and staging cleanup also failed"
            raise BaseExceptionGroup(
                message,
                [qualification_error, cleanup_error],
            ) from None
        raise
    return tuple(output / name for name in sorted(final_names))


def main(argv: list[str] | None = None) -> int:
    """Build and qualify the requested local release-candidate directory.

    Returns:
        Zero after printing the complete validated artifact set.

    """
    arguments = _build_parser().parse_args(argv)
    paths = (
        verify_release_directory(arguments.verify_directory)
        if arguments.verify_directory is not None
        else qualify_release(arguments.output_directory)
    )
    for path in paths:
        print(path)
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
