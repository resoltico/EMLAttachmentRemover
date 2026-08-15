"""Build a deterministic, dependency-free command-line zipapp."""

from __future__ import annotations

import argparse
import hashlib
import os
import re
import stat
import subprocess
import sys
import tempfile
import tomllib
import zipfile
from dataclasses import dataclass
from pathlib import Path
from typing import Final

if __package__:
    from tools.mutmut_workspace import generated_sidecar
else:
    from mutmut_workspace import generated_sidecar  # type: ignore[import-not-found,no-redef]  # ruff: ignore[unsorted-imports]

PROJECT_ROOT: Final = Path(__file__).resolve().parents[1]
PACKAGE_SOURCE: Final = PROJECT_ROOT / "src" / "eml_attachment_remover"
PROJECT_CONFIG: Final = PROJECT_ROOT / "pyproject.toml"
LICENSE_FILE: Final = PROJECT_ROOT / "LICENSE"
DEFAULT_TARGET: Final = PROJECT_ROOT / "build" / "remove-eml-attachments.pyz"
INTERPRETER: Final = "/usr/bin/env python3.14"
ARCHIVE_MODE: Final = 0o100644 << 16
CHUNK_SIZE: Final = 65_536
VERIFY_TIMEOUT_SECONDS: Final = 30
METADATA_TEMPLATE: Final = """Metadata-Version: 2.4
Name: {name}
Version: {version}
Summary: {summary}
Requires-Python: {requires_python}
License-Expression: {license_expression}
"""
REQUIRES_PATTERN: Final = re.compile(r">=(\d+)\.(\d+),<(\d+)\.(\d+)")


@dataclass(frozen=True, slots=True)
class ProjectMetadata:
    """Contain archive identity fields derived from project metadata."""

    name: str
    version: str
    summary: str
    requires_python: str
    license_expression: str
    implementation: str


def _project_metadata() -> ProjectMetadata:
    """Read authoritative archive metadata from ``pyproject.toml``.

    Returns:
        The fields that embedded packaging metadata must report.

    """
    with PROJECT_CONFIG.open("rb") as project_file:
        configuration = tomllib.load(project_file)
    project = configuration["project"]
    runtime = configuration["tool"]["eml-attachment-remover"]["runtime"]
    return ProjectMetadata(
        name=str(project["name"]),
        version=str(project["version"]),
        summary=str(project["description"]),
        requires_python=str(project["requires-python"]),
        license_expression=str(project["license"]),
        implementation=str(runtime["implementation"]),
    )


def _runtime_bounds(requirement: str) -> tuple[tuple[int, int], tuple[int, int]]:
    """Parse the supported lower-inclusive and upper-exclusive Python bounds.

    Returns:
        Two major/minor version tuples.

    Raises:
        ValueError: If the canonical constraint cannot generate a safe launcher.

    """
    match = REQUIRES_PATTERN.fullmatch(requirement)
    if match is None:
        message = f"unsupported requires-python constraint for zipapp: {requirement}"
        raise ValueError(message)
    lower_major, lower_minor, upper_major, upper_minor = map(int, match.groups())
    return (lower_major, lower_minor), (upper_major, upper_minor)


def _launcher_source(metadata: ProjectMetadata) -> bytes:
    """Generate a runtime-guarded portable zipapp launcher.

    Returns:
        UTF-8 Python source enforcing the declared implementation and bounds.

    """
    lower, upper = _runtime_bounds(metadata.requires_python)
    source = f"""import platform
import sys

if platform.python_implementation() != {metadata.implementation!r} or not (
    {lower!r} <= sys.version_info[:2] < {upper!r}
):
    print(
        "EML Attachment Remover requires {metadata.implementation} "
        "{metadata.requires_python}",
        file=sys.stderr,
    )
    raise SystemExit(1)

from eml_attachment_remover.app import main

raise SystemExit(main())
"""
    return source.encode()


def _build_parser() -> argparse.ArgumentParser:
    """Create the command-line parser for this build tool.

    Returns:
        The configured argument parser.

    """
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument(
        "--target",
        type=Path,
        default=DEFAULT_TARGET,
        help=f"archive path (default: {DEFAULT_TARGET})",
    )
    parser.add_argument(
        "--checksum-file",
        type=Path,
        help="optional SHA-256 manifest to write for this archive",
    )
    parser.add_argument(
        "--no-verify",
        action="store_true",
        help="skip executing the completed archive with --version",
    )
    return parser


def _source_files() -> tuple[Path, ...]:
    """Return all package files that belong in the archive.

    Returns:
        Source files sorted by their portable archive name.

    Raises:
        FileNotFoundError: If the package source tree is unavailable.
        ValueError: If the package contains an unsafe or unexpected entry.

    """
    if not PACKAGE_SOURCE.is_dir():
        message = f"package source not found: {PACKAGE_SOURCE}"
        raise FileNotFoundError(message)
    paths = sorted(
        PACKAGE_SOURCE.rglob("*"),
        key=lambda path: path.relative_to(PROJECT_ROOT / "src").as_posix(),
    )
    sources: list[Path] = []
    for path in paths:
        if "__pycache__" in path.relative_to(PACKAGE_SOURCE).parts or generated_sidecar(
            path, PROJECT_ROOT
        ):
            continue
        if path.is_symlink():
            message = f"package source must not be a symbolic link: {path}"
            raise ValueError(message)
        if path.is_dir():
            continue
        if not path.is_file():
            message = f"package source must be a regular file: {path}"
            raise ValueError(message)
        if path.suffix != ".py" and path.name != "py.typed":
            message = f"unexpected package source file: {path}"
            raise ValueError(message)
        sources.append(path)
    return tuple(sources)


def _archive_members(metadata: ProjectMetadata) -> tuple[tuple[str, bytes], ...]:
    """Return every archive member with a stable name and byte payload.

    Returns:
        Archive names and contents in lexical order.

    Raises:
        FileNotFoundError: If the license file is unavailable.

    """
    if not LICENSE_FILE.is_file():
        message = f"license file not found: {LICENSE_FILE}"
        raise FileNotFoundError(message)
    members = [
        ("LICENSE", LICENSE_FILE.read_bytes()),
        ("__main__.py", _launcher_source(metadata)),
        (
            (
                f"{re.sub(r'[-_.]+', '_', metadata.name)}-"
                f"{metadata.version}.dist-info/METADATA"
            ),
            METADATA_TEMPLATE.format(
                name=metadata.name,
                version=metadata.version,
                summary=metadata.summary,
                requires_python=metadata.requires_python,
                license_expression=metadata.license_expression,
            ).encode(),
        ),
    ]
    members.extend(
        (
            source.relative_to(PROJECT_ROOT / "src").as_posix(),
            source.read_bytes(),
        )
        for source in _source_files()
    )
    return tuple(sorted(members))


def _zip_info(name: str) -> zipfile.ZipInfo:
    """Return deterministic metadata for one regular archive member.

    Returns:
        The normalized ZIP member metadata.

    """
    info = zipfile.ZipInfo(name)
    info.compress_type = zipfile.ZIP_STORED
    info.create_system = 3
    info.external_attr = ARCHIVE_MODE
    return info


def _temporary_path(target: Path) -> Path:
    """Reserve a same-directory temporary path for atomic replacement.

    Returns:
        An empty temporary path in the target directory.

    """
    descriptor, name = tempfile.mkstemp(
        dir=target.parent,
        prefix=f".{target.name}.",
        suffix=".tmp",
    )
    os.close(descriptor)
    return Path(name)


def _write_archive(target: Path, metadata: ProjectMetadata) -> None:
    """Write a stable ZIP archive with a portable Python launcher."""
    with target.open("wb") as archive_file:
        archive_file.write(f"#!{INTERPRETER}\n".encode())
        with zipfile.ZipFile(
            archive_file,
            mode="w",
        ) as archive:
            for name, content in _archive_members(metadata):
                archive.writestr(_zip_info(name), content)


def _verify_archive(path: Path, metadata: ProjectMetadata, *, execute: bool) -> None:
    """Check archive integrity, required metadata, and optional execution.

    Raises:
        RuntimeError: If the archive is malformed or lacks required metadata.

    """
    with zipfile.ZipFile(path) as archive:
        expected_members = dict(_archive_members(metadata))
        if (
            archive.testzip() is not None
            or set(archive.namelist()) != set(expected_members)
            or any(
                archive.read(name) != content
                for name, content in expected_members.items()
            )
        ):
            message = f"invalid zipapp archive: {path}"
            raise RuntimeError(message)
    if execute:
        subprocess.run(
            [
                sys.executable,
                "-I",
                "-X",
                "dev",
                "-W",
                "error",
                str(path),
                "--version",
            ],
            check=True,
            text=True,
            timeout=VERIFY_TIMEOUT_SECONDS,
        )


def _sha256(path: Path) -> str:
    """Return the SHA-256 digest of one completed archive.

    Returns:
        The lower-case hexadecimal digest.

    """
    digest = hashlib.sha256()
    with path.open("rb") as archive_file:
        while chunk := archive_file.read(CHUNK_SIZE):
            digest.update(chunk)
    return digest.hexdigest()


def _write_checksum(path: Path, archive: Path) -> None:
    """Atomically write a portable checksum manifest for one archive."""
    path = _lexical_absolute(path)
    _reject_symbolic_output(path, "checksum")
    path.parent.mkdir(parents=True, exist_ok=True)
    temporary = _temporary_path(path)
    try:
        temporary.write_bytes(f"{_sha256(archive)}  {archive.name}\n".encode())
        temporary.replace(path)
    except BaseException:
        temporary.unlink(missing_ok=True)
        raise


def _lexical_absolute(path: Path) -> Path:
    """Return an absolute normalized path without following the final component.

    Returns:
        The lexical absolute path.

    """
    expanded = path.expanduser()
    absolute = expanded if expanded.is_absolute() else Path.cwd() / expanded
    return absolute.parent.resolve() / absolute.name


def _reject_symbolic_output(path: Path, label: str) -> None:
    """Reject a final-component symbolic link before publication.

    Raises:
        ValueError: If the named output is a symbolic link.

    """
    if path.is_symlink():
        message = f"{label} output must not be a symbolic link: {path}"
        raise ValueError(message)


def _same_output(first: Path, second: Path) -> bool:
    """Return whether two outputs lexically or physically alias.

    Returns:
        Whether publishing one could overwrite the other.

    """
    if first == second:
        return True
    try:
        return first.samefile(second)
    except OSError:
        return False


def _validate_outputs(target: Path, checksum: Path | None) -> tuple[Path, Path | None]:
    """Normalize and reject unsafe or aliasing output names.

    Returns:
        Safe lexical archive and optional checksum paths.

    Raises:
        ValueError: If either output is symbolic or both outputs alias.

    """
    normalized_target = _lexical_absolute(target)
    _reject_symbolic_output(normalized_target, "zipapp")
    if checksum is None:
        return normalized_target, None
    normalized_checksum = _lexical_absolute(checksum)
    _reject_symbolic_output(normalized_checksum, "checksum")
    if _same_output(normalized_target, normalized_checksum):
        message = "zipapp and checksum outputs must be distinct files"
        raise ValueError(message)
    return normalized_target, normalized_checksum


def build_zipapp(target: Path, *, verify: bool) -> Path:
    """Build and optionally execute a verified, deterministic zipapp.

    Returns:
        The absolute path of the completed archive.

    """
    target, _checksum = _validate_outputs(target, None)
    target.parent.mkdir(parents=True, exist_ok=True)
    temporary = _temporary_path(target)
    try:
        metadata = _project_metadata()
        _write_archive(temporary, metadata)
        _verify_archive(temporary, metadata, execute=verify)
        temporary.chmod(
            temporary.stat().st_mode | stat.S_IXUSR | stat.S_IXGRP | stat.S_IXOTH
        )
        temporary.replace(target)
    except BaseException:
        temporary.unlink(missing_ok=True)
        raise
    return target


def main(argv: list[str] | None = None) -> int:
    """Run the deterministic zipapp builder.

    Returns:
        Zero after a successful build.

    """
    arguments = _build_parser().parse_args(argv)
    target_path, checksum_path = _validate_outputs(
        arguments.target,
        arguments.checksum_file,
    )
    target = build_zipapp(target_path, verify=not arguments.no_verify)
    if checksum_path is not None:
        _write_checksum(checksum_path, target)
    print(target)
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
