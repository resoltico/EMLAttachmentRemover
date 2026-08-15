"""Create compact public synthetic distribution archives for verifier tests."""

from __future__ import annotations

import base64
import csv
import hashlib
import io
import tarfile
import zipfile
from dataclasses import dataclass
from typing import TYPE_CHECKING

from tools.distribution_archive_contract import ArchiveContract

if TYPE_CHECKING:
    from pathlib import Path


@dataclass(slots=True)
class SyntheticDistribution:
    """Hold one minimal public repository and its generated archives."""

    root: Path
    config: Path
    public_files: tuple[Path, ...]
    contract: ArchiveContract
    metadata: bytes
    source_archive: Path
    wheel: Path


def create_distribution(root: Path, *, scripts: bool = True) -> SyntheticDistribution:
    """Create one valid minimal public synthetic sdist and wheel.

    Returns:
        Paths and contract for the generated distribution.

    """
    config = root / "pyproject.toml"
    package = root / "src" / "public_package" / "__init__.py"
    license_file = root / "LICENSE"
    readme = root / "README.md"
    package.parent.mkdir(parents=True)
    package.write_text('VALUE = "public"\n', encoding="utf-8")
    license_file.write_text("Public synthetic license\n", encoding="utf-8")
    readme.write_text("Public synthetic README\n", encoding="utf-8")
    script_text = (
        '[project.scripts]\npublic-command = "public_package:main"\n' if scripts else ""
    )
    config.write_text(
        "[build-system]\n"
        'requires = ["hatchling==1.32.0"]\n'
        'build-backend = "hatchling.build"\n'
        "[project]\n"
        'name = "public-project"\n'
        'version = "1.2.3"\n'
        'description = "Public synthetic distribution"\n'
        'readme = "README.md"\n'
        'requires-python = ">=3.14,<3.15"\n'
        'license = "MIT"\n'
        'license-files = ["LICENSE"]\n'
        'authors = [{name = "Public Example"}]\n'
        'keywords = ["synthetic", "public"]\n'
        'classifiers = ["Topic :: Utilities"]\n'
        "dependencies = []\n"
        f"{script_text}"
        "[project.urls]\n"
        'Homepage = "https://example.test/public-project"\n'
        'Repository = "https://example.test/public-project.git"\n'
        "[tool.eml-attachment-remover.runtime]\n"
        'implementation = "CPython"\n'
        "[tool.hatch.build.targets.wheel]\n"
        'packages = ["src/public_package"]\n',
        encoding="utf-8",
    )
    public_files = (license_file, readme, config, package)
    contract = ArchiveContract.from_project(root, config, public_files)
    metadata = _metadata()
    source_archive = root / "public_project-1.2.3.tar.gz"
    wheel = root / "public_project-1.2.3-cp314-none-any.whl"
    write_source_archive(source_archive, contract, metadata)
    write_wheel(wheel, contract, metadata)
    return SyntheticDistribution(
        root,
        config,
        public_files,
        contract,
        metadata,
        source_archive,
        wheel,
    )


def _metadata() -> bytes:
    """Return valid metadata with deliberately reordered Python specifiers.

    Returns:
        Core metadata bytes matching the synthetic pyproject.

    """
    return (
        b"Metadata-Version: 2.5\n"
        b"Name: public-project\n"
        b"Version: 1.2.3\n"
        b"Summary: Public synthetic distribution\n"
        b"Author: Public Example\n"
        b"License-Expression: MIT\n"
        b"License-File: LICENSE\n"
        b"Keywords: public,synthetic\n"
        b"Classifier: Topic :: Utilities\n"
        b"Requires-Python: <3.15,>=3.14\n"
        b"Project-URL: Homepage, https://example.test/public-project\n"
        b"Project-URL: Repository, https://example.test/public-project.git\n"
        b"Description-Content-Type: text/markdown\n"
        b"\n"
        b"Public synthetic README\n"
    )


def write_source_archive(
    path: Path,
    contract: ArchiveContract,
    metadata: bytes,
    *,
    extra_members: tuple[tarfile.TarInfo, ...] = (),
) -> None:
    """Write a synthetic sdist, optionally with metadata-only extra members."""
    with tarfile.open(path, "w:gz") as archive:
        root = _tar_info(contract.source_root)
        root.type = tarfile.DIRTYPE
        archive.addfile(root)
        for name, source in contract.public_sources.items():
            content = source.read_bytes()
            member = _tar_info(f"{contract.source_root}/{name}")
            member.size = len(content)
            archive.addfile(member, io.BytesIO(content))
        package_info = _tar_info(f"{contract.source_root}/PKG-INFO")
        package_info.size = len(metadata)
        archive.addfile(package_info, io.BytesIO(metadata))
        for member in extra_members:
            archive.addfile(member)


def _tar_info(name: str) -> tarfile.TarInfo:
    """Return normalized synthetic source-archive member metadata.

    Returns:
        A member matching Hatchling's reproducible metadata contract.

    """
    member = tarfile.TarInfo(name)
    member.mtime = 1_580_601_600
    member.mode = 0o644
    return member


def _record_text(files: dict[str, bytes], record_name: str) -> bytes:
    """Return a complete valid wheel RECORD.

    Returns:
        UTF-8 CSV bytes authenticating every wheel file.

    """
    output = io.StringIO(newline="")
    writer = csv.writer(output, lineterminator="\n")
    for name, content in files.items():
        digest = base64.urlsafe_b64encode(hashlib.sha256(content).digest()).rstrip(b"=")
        writer.writerow((name, f"sha256={digest.decode()}", len(content)))
    writer.writerow((record_name, "", ""))
    return output.getvalue().encode()


def write_wheel(
    path: Path,
    contract: ArchiveContract,
    metadata: bytes,
    *,
    overrides: dict[str, bytes] | None = None,
) -> None:
    """Write a valid synthetic wheel with optional file-content overrides."""
    files = {
        name: source.read_bytes() for name, source in contract.wheel_sources().items()
    }
    files[f"{contract.dist_info}/METADATA"] = metadata
    files[f"{contract.dist_info}/WHEEL"] = contract.expected_wheel_metadata().encode()
    if contract.has_scripts:
        files[f"{contract.dist_info}/entry_points.txt"] = (
            contract.expected_entry_points().encode()
        )
    if overrides is not None:
        files.update(overrides)
    record_name = f"{contract.dist_info}/RECORD"
    files[record_name] = _record_text(files, record_name)
    with zipfile.ZipFile(path, "w") as archive:
        for name, content in files.items():
            member = zipfile.ZipInfo(name, (2020, 2, 2, 0, 0, 0))
            member.create_system = 3
            member.external_attr = (
                0o644 if name.startswith(f"{contract.dist_info}/") else 0o100644
            ) << 16
            archive.writestr(member, content)
