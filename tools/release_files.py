"""Provide safe filesystem and checksum operations for release qualification."""

from __future__ import annotations

import hashlib
import tempfile
from pathlib import Path
from typing import TYPE_CHECKING, Final

if TYPE_CHECKING:
    from collections.abc import Sequence

CHECKSUM_FILE_NAME: Final = "SHA256SUMS"


class ReleaseQualificationError(RuntimeError):
    """Report an unsafe output directory or invalid release artifact set."""


def prepare_output_directory(output: Path) -> Path:
    """Validate the destination and reserve a same-parent staging directory.

    Returns:
        A unique staging directory for atomic publication.

    Raises:
        ReleaseQualificationError: If the requested destination is not empty.

    """
    output.parent.mkdir(parents=True, exist_ok=True)
    if (output.exists() or output.is_symlink()) and (
        not output.is_dir() or output.is_symlink() or any(output.iterdir())
    ):
        message = f"release output must be an absent or empty directory: {output}"
        raise ReleaseQualificationError(message)
    return Path(tempfile.mkdtemp(dir=output.parent, prefix=f".{output.name}."))


def lexical_absolute(path: Path) -> Path:
    """Return an absolute path without following its final component.

    Returns:
        The normalized path with only its parent resolved.

    """
    expanded = path.expanduser()
    absolute = expanded if expanded.is_absolute() else Path.cwd() / expanded
    return absolute.parent.resolve() / absolute.name


def assert_exact_entries(directory: Path, expected_names: frozenset[str]) -> None:
    """Require an exact set of regular, non-symbolic artifact files.

    Raises:
        ReleaseQualificationError: If entries are missing, unexpected, or unsafe.

    """
    entries = {path.name: path for path in directory.iterdir()}
    actual_names = frozenset(entries)
    if actual_names != expected_names:
        missing = sorted(expected_names - actual_names)
        unknown = sorted(actual_names - expected_names)
        message = f"release artifact set mismatch: missing={missing}; unknown={unknown}"
        raise ReleaseQualificationError(message)
    for name in sorted(expected_names):
        path = entries[name]
        if path.is_symlink() or not path.is_file():
            message = f"release artifact must be a regular non-symbolic file: {path}"
            raise ReleaseQualificationError(message)


def sha256(path: Path) -> str:
    """Return the complete SHA-256 digest of one artifact.

    Returns:
        A lower-case hexadecimal digest.

    """
    with path.open("rb") as artifact:
        return hashlib.file_digest(artifact, hashlib.sha256).hexdigest()


def manifest_text(directory: Path, artifact_names: Sequence[str]) -> str:
    """Return a portable checksum manifest containing basenames only.

    Returns:
        Deterministically ordered SHA-256 records.

    """
    return "".join(
        f"{sha256(directory / name)}  {name}\n" for name in sorted(artifact_names)
    )


def manifest_bytes(directory: Path, artifact_names: Sequence[str]) -> bytes:
    """Return the checksum manifest in its canonical UTF-8 wire encoding.

    Returns:
        The deterministic manifest encoded as UTF-8 bytes.

    """
    return manifest_text(directory, artifact_names).encode()


def write_and_verify_manifest(
    directory: Path,
    artifact_names: Sequence[str],
) -> Path:
    """Write the checksum manifest and independently recompute expected bytes.

    Returns:
        The verified manifest path.

    Raises:
        ReleaseQualificationError: If the completed manifest does not verify.

    """
    manifest = directory / CHECKSUM_FILE_NAME
    manifest.write_bytes(manifest_bytes(directory, artifact_names))
    expected = manifest_bytes(directory, artifact_names)
    if manifest.read_bytes() != expected:
        message = f"release checksum manifest failed verification: {manifest}"
        raise ReleaseQualificationError(message)
    return manifest


def publish_staging(staging: Path, output: Path) -> None:
    """Replace an absent or empty destination while preserving it on failure.

    Raises:
        ReleaseQualificationError: If the destination became unsafe during building.

    """
    destination_was_empty = output.exists() or output.is_symlink()
    if destination_was_empty:
        if output.is_symlink() or not output.is_dir() or any(output.iterdir()):
            message = f"release output must remain absent or empty: {output}"
            raise ReleaseQualificationError(message)
        output.rmdir()
    try:
        staging.replace(output)
    except BaseException:
        if destination_was_empty and not output.exists() and not output.is_symlink():
            output.mkdir()
        raise
