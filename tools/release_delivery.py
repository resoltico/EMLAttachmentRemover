"""Produce and reverify the complete macOS-plus-portable release delivery."""

from __future__ import annotations

import argparse
import os
import shutil
import subprocess
import sys
import tempfile
import tomllib
from pathlib import Path

from tools import macos_archive, qualify_release, release_files
from tools.build_timestamp import environment, stamp_tree

ROOT = Path(__file__).resolve().parents[1]
ARCHITECTURES = ("arm64", "x86_64")


def _version() -> str:
    """Read the single source of release version metadata.

    Returns:
        The project version from pyproject.toml.

    """
    with (ROOT / "pyproject.toml").open("rb") as source:
        return str(tomllib.load(source)["project"]["version"])


def _macos() -> None:
    """Refuse incomplete native qualification on a non-macOS publisher.

    Raises:
        ReleaseQualificationError: If the release contract is violated.

    """
    if sys.platform != "darwin":
        message = (
            "Complete native release production/verification requires macOS; "
            "use qualify_release.py for portable-only artifacts"
        )
        raise release_files.ReleaseQualificationError(message)


def verify(directory: Path) -> tuple[Path, ...]:
    """Require all eight delivery files and verify every extracted native edition.

    Returns:
        The complete verified artifact and checksum paths.

    """
    _macos()
    version = _version()
    paths = qualify_release.verify_release_directory(
        directory,
        additional_artifacts=tuple(
            macos_archive.archive_name(version, architecture, mode)
            for architecture in ARCHITECTURES
            for mode in macos_archive.RUNTIME_MODES
        ),
    )
    for architecture in ARCHITECTURES:
        for mode in macos_archive.RUNTIME_MODES:
            macos_archive.verify(
                directory / macos_archive.archive_name(version, architecture, mode),
                directory / "remove-eml-attachments.pyz",
                version,
                architecture,
                mode,
            )
    return paths


def _native(
    directory: Path,
    architecture: str,
    icon_resources: Path,
    runtime_mode: str = "bundled",
) -> Path:
    """Build and package one independently staged ad-hoc signed application.

    Returns:
        The independently packaged native archive path.

    """
    directory.mkdir()
    app = directory / macos_archive.APP
    subprocess.run(
        [
            "/bin/sh",
            str(ROOT / "integrations/macos-ui/build.sh"),
            str(app),
            architecture,
            runtime_mode,
        ],
        env={
            **os.environ,
            **environment(),
            "EML_REMOVER_PYTHON": sys.executable,
            "EML_ICON_RESOURCES": str(icon_resources),
        },
        check=True,
        timeout=600,
    )
    archive = directory / "native.zip"
    macos_archive.package(app, archive)
    return archive


def _copy_verified(paths: tuple[Path, ...], staging: Path) -> None:
    """Copy already verified bytes into the atomic publication directory.

    Raises:
        ReleaseQualificationError: If any staged bytes change.

    """
    for source in paths:
        copied = staging / source.name
        shutil.copyfile(source, copied)
        if release_files.sha256(source) != release_files.sha256(copied):
            message = "Staged delivery copy changed"
            raise release_files.ReleaseQualificationError(message)


def _complete(staging: Path) -> tuple[str, ...]:
    """Qualify and copy the complete delivery into its reserved staging directory.

    Returns:
        The eight verified basenames.

    """
    with tempfile.TemporaryDirectory(prefix="eml-release-delivery-") as temporary:
        root = Path(temporary)
        portable = root / "artifacts"
        qualify_release.qualify_release(portable)
        icon_resources = root / "icon-resources"
        subprocess.run(
            [
                "/bin/sh",
                str(ROOT / "integrations/macos-ui/compile-icon.sh"),
                str(icon_resources),
            ],
            check=True,
            timeout=180,
        )
        for architecture in ARCHITECTURES:
            for mode in macos_archive.RUNTIME_MODES:
                _native_pair(root, portable, architecture, mode, icon_resources)
        artifacts = tuple(
            sorted(
                file.name for file in portable.iterdir() if file.name != "SHA256SUMS"
            )
        )
        release_files.write_and_verify_manifest(portable, artifacts)
        verified = verify(portable)
        _copy_verified(verified, staging)
        return tuple(file.name for file in verified)


def _publish(output: Path, source: Path, names: tuple[str, ...]) -> tuple[Path, ...]:
    """Copy verified artifacts and atomically publish their complete set.

    Returns:
        The eight final paths.

    Raises:
        BaseExceptionGroup: If publication and staging cleanup both fail.

    """
    staging = release_files.prepare_output_directory(output)
    try:
        _copy_verified(tuple(source / name for name in names), staging)
        stamp_tree(staging)
        release_files.publish_staging(staging, output)
    except BaseException as error:
        try:
            shutil.rmtree(staging)
        except OSError as cleanup_error:
            message = "Release delivery failed and staging cleanup also failed"
            raise BaseExceptionGroup(message, [error, cleanup_error]) from None
        raise
    return tuple(output / name for name in names)


def _native_pair(
    root: Path,
    portable: Path,
    architecture: str,
    mode: str,
    icons: Path,
) -> None:
    """Compare independent same-time builds before adding one approved edition.

    Raises:
        ReleaseQualificationError: If independent archive bytes differ.

    """
    first = _native(root / f"{architecture}-{mode}-a", architecture, icons, mode)
    second = _native(root / f"{architecture}-{mode}-b", architecture, icons, mode)
    if release_files.sha256(first) != release_files.sha256(second):
        message = "Native archives differ between independent builds"
        raise release_files.ReleaseQualificationError(message)
    shutil.copyfile(
        first, portable / macos_archive.archive_name(_version(), architecture, mode)
    )


def build(output: Path) -> tuple[Path, ...]:
    """Qualify outside the checkout, then atomically publish every artifact.

    Returns:
        The verified release artifacts and checksum manifest.

    """
    _macos()
    output = release_files.lexical_absolute(output)
    release_files.validate_output_directory(output)
    with tempfile.TemporaryDirectory(prefix="eml-qualified-delivery-") as temporary:
        source = Path(temporary)
        names = _complete(source)
        return _publish(output, source, names)


def main(argv: list[str] | None = None) -> int:
    """Build a complete delivery or reverify a downloaded one without rebuilding.

    Returns:
        Zero after printing every accepted artifact path.

    """
    parser = argparse.ArgumentParser(description=__doc__)
    modes = parser.add_mutually_exclusive_group(required=True)
    modes.add_argument("--output-directory", type=Path)
    modes.add_argument("--verify-directory", type=Path)
    parser.add_argument(
        "--portable-only",
        action="store_true",
        help="host-local CLI qualification only; never accepted by the publisher",
    )
    options = parser.parse_args(argv)
    if options.portable_only:
        paths = (
            qualify_release.verify_release_directory(options.verify_directory)
            if options.verify_directory
            else qualify_release.qualify_release(options.output_directory)
        )
    else:
        paths = (
            verify(options.verify_directory)
            if options.verify_directory
            else build(options.output_directory)
        )
    for path in paths:
        print(path)
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
