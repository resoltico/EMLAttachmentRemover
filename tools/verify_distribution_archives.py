"""Verify release archives against the audited public source surface."""

from __future__ import annotations

import base64
import csv
import hashlib
import importlib
import io
import tarfile
import zipfile
from email import policy
from email.parser import BytesParser
from pathlib import PurePosixPath
from typing import TYPE_CHECKING, Final, Protocol, cast

if TYPE_CHECKING:
    from collections.abc import Mapping, Sequence
    from email.message import Message
    from pathlib import Path
    from typing import IO

    class ArchiveContract(Protocol):
        """Describe release metadata consumed by both archive verifiers."""

        source_root: str
        dist_info: str
        public_sources: Mapping[str, Path]

        def wheel_sources(self) -> dict[str, Path]:
            """Return exact installed-source mappings."""

        def wheel_generated_files(self) -> frozenset[str]:
            """Return exact generated dist-info members."""

        def verify_metadata(self, metadata: Message) -> None:
            """Verify release-critical core metadata."""

        def expected_entry_points(self) -> str:
            """Return canonical console entry-point text."""

        def expected_wheel_metadata(self) -> str:
            """Return exact generated WHEEL metadata text."""

    class ArchiveContractFactory(Protocol):
        """Describe canonical archive-contract construction."""

        def from_project(
            self,
            repository_root: Path,
            project_config: Path,
            public_files: Sequence[Path],
        ) -> ArchiveContract:
            """Build an archive contract from project metadata."""

    class ContractModule(Protocol):
        """Describe shared archive-contract helpers loaded portably."""

        DistributionArchiveError: type[RuntimeError]
        ArchiveContract: ArchiveContractFactory

        def safe_parts(self, name: str, *, directory: bool) -> tuple[str, ...]:
            """Return validated POSIX path components."""

        def allowed_directories(self, file_names: set[str]) -> set[str]:
            """Return permitted explicit directory members."""

        def same_content(
            self,
            archived: IO[bytes],
            source: Path,
            chunk_size: int,
        ) -> bool:
            """Compare archive and source bytes."""

        def require_exact_set(
            self,
            label: str,
            actual: set[str],
            expected: set[str],
        ) -> None:
            """Require exact member equality."""


contract_module = cast(
    "ContractModule",
    importlib.import_module(
        "tools.distribution_archive_contract"
        if __package__
        else "distribution_archive_contract",
    ),
)
reproducibility = importlib.import_module(
    "tools.archive_reproducibility_policy"
    if __package__
    else "archive_reproducibility_policy"
)

DistributionArchiveError = contract_module.DistributionArchiveError
CHUNK_SIZE: Final = 65_536
MAX_GENERATED_FILE_BYTES: Final = 1_048_576
RECORD_FIELD_COUNT: Final = 3
REGULAR_TAR_TYPES: Final = frozenset({tarfile.REGTYPE, tarfile.AREGTYPE})
UTF8_ENCODING: Final = "utf-8"
ASCII_ENCODING: Final = "ascii"
CSV_NEWLINE: Final = ""
BASE64_PADDING: Final = b"="
SOURCE_ARCHIVE_LABEL: Final = "source archive"
SOURCE_DIRECTORY_LABEL: Final = "source-archive"
WHEEL_LABEL: Final = "wheel"
WHEEL_DIRECTORY_LABEL: Final = "wheel"
WHEEL_RECORD_LABEL: Final = "wheel RECORD"
RECORD_FIELD_ERROR: Final = "wheel RECORD rows must have exactly 3 fields"
RECORD_DUPLICATE_ERROR: Final = "wheel RECORD contains duplicate paths"
RECORD_SELF_HASH_ERROR: Final = "wheel RECORD must not hash itself"
METADATA_MISMATCH_ERROR: Final = "sdist PKG-INFO and wheel METADATA differ"


def _tar_file_bytes(archive: tarfile.TarFile, member: tarfile.TarInfo) -> bytes:
    """Read one bounded generated tar member.

    Returns:
        The complete generated member bytes.

    """
    if member.size > MAX_GENERATED_FILE_BYTES:
        message = f"generated archive member is too large: {member.name}"
        raise DistributionArchiveError(message)
    extracted = archive.extractfile(member)
    if extracted is None:
        message = f"cannot read regular source-archive member: {member.name}"
        raise DistributionArchiveError(message)
    with extracted:
        return extracted.read()


def _add_tar_member(
    member: tarfile.TarInfo,
    contract: ArchiveContract,
    files: dict[str, tarfile.TarInfo],
    directories: set[str],
) -> None:
    """Validate and classify one source-archive member."""
    is_directory = member.isdir()
    parts = contract_module.safe_parts(member.name, directory=is_directory)
    if parts[0] != contract.source_root:
        message = f"wrong source-archive root: {member.name!r}"
        raise DistributionArchiveError(message)
    relative_name = "/".join(parts[1:])
    if relative_name in files or relative_name in directories:
        message = f"duplicate source-archive member: {member.name!r}"
        raise DistributionArchiveError(message)
    if not is_directory:
        if not relative_name:
            message = f"root source-archive member must be a directory: {member.name!r}"
            raise DistributionArchiveError(message)
        if member.type not in REGULAR_TAR_TYPES:
            message = f"non-regular source-archive member: {member.name!r}"
            raise DistributionArchiveError(message)
    if not reproducibility.tar_member_normalized(member):
        message = f"non-reproducible source-archive metadata: {member.name!r}"
        raise DistributionArchiveError(message)
    if is_directory:
        directories.add(relative_name)
    else:
        files[relative_name] = member


def _check_directories(label: str, directories: set[str], files: set[str]) -> None:
    """Reject archive directory entries unrelated to exact expected files."""
    unexpected = directories - {""} - contract_module.allowed_directories(files)
    if unexpected:
        message = f"unexpected {label} directories: {sorted(unexpected)}"
        raise DistributionArchiveError(message)


def _compare_tar_sources(
    archive: tarfile.TarFile,
    members: Mapping[str, tarfile.TarInfo],
    sources: Mapping[str, Path],
) -> None:
    """Require every sdist source member to match repository bytes."""
    for name, source in sources.items():
        extracted = archive.extractfile(members[name])
        if extracted is None:
            message = f"cannot read regular source-archive member: {name}"
            raise DistributionArchiveError(message)
        with extracted:
            matches = contract_module.same_content(extracted, source, CHUNK_SIZE)
        if not matches:
            message = f"source-archive content differs from repository: {name}"
            raise DistributionArchiveError(message)


def _inspect_source_archive(
    archive: tarfile.TarFile,
    contract: ArchiveContract,
) -> bytes:
    """Inspect one already-open source archive.

    Returns:
        The verified generated ``PKG-INFO`` bytes.

    """
    sources = dict(contract.public_sources)
    expected_files = set(sources) | {"PKG-INFO"}
    members: dict[str, tarfile.TarInfo] = {}
    directories: set[str] = set()
    for member in archive.getmembers():
        _add_tar_member(member, contract, members, directories)
    contract_module.require_exact_set(
        SOURCE_ARCHIVE_LABEL,
        set(members),
        expected_files,
    )
    _check_directories(SOURCE_DIRECTORY_LABEL, directories, expected_files)
    _compare_tar_sources(archive, members, sources)
    return _tar_file_bytes(archive, members["PKG-INFO"])


def _verify_source_archive(
    archive_path: Path,
    contract: ArchiveContract,
) -> bytes:
    """Verify the exact safe sdist member set and source bytes.

    Returns:
        The verified generated ``PKG-INFO`` bytes.

    """
    try:
        with tarfile.open(archive_path, mode="r:gz") as archive:
            return _inspect_source_archive(archive, contract)
    except (OSError, tarfile.TarError) as error:
        message = f"cannot inspect source archive {archive_path}: {error}"
        raise DistributionArchiveError(message) from error


def _zip_file_bytes(archive: zipfile.ZipFile, member: zipfile.ZipInfo) -> bytes:
    """Read one bounded generated wheel member.

    Returns:
        The complete generated member bytes after ZIP integrity verification.

    """
    if member.flag_bits & 1:
        message = f"encrypted wheel member is prohibited: {member.filename}"
        raise DistributionArchiveError(message)
    if member.file_size > MAX_GENERATED_FILE_BYTES:
        message = f"generated archive member is too large: {member.filename}"
        raise DistributionArchiveError(message)
    return archive.read(member)


def _add_zip_member(
    member: zipfile.ZipInfo,
    expected_roots: set[str],
    files: dict[str, zipfile.ZipInfo],
    directories: set[str],
) -> None:
    """Validate and classify one wheel member."""
    if member.flag_bits & 1:
        message = f"encrypted wheel member is prohibited: {member.filename}"
        raise DistributionArchiveError(message)
    parts = contract_module.safe_parts(member.filename, directory=member.is_dir())
    name = "/".join(parts)
    if parts[0] not in expected_roots:
        message = f"wrong wheel member root: {member.filename!r}"
        raise DistributionArchiveError(message)
    if name in files or name in directories:
        message = f"duplicate wheel member: {member.filename!r}"
        raise DistributionArchiveError(message)
    if not reproducibility.regular_wheel_member(member):
        message = f"non-regular wheel member: {member.filename!r}"
        raise DistributionArchiveError(message)
    directory_member = member.is_dir()
    generated = parts[0].endswith(".dist-info")
    if not reproducibility.wheel_member_normalized(member, generated=generated):
        message = f"non-reproducible wheel member metadata: {member.filename!r}"
        raise DistributionArchiveError(message)
    if directory_member:
        directories.add(name)
    else:
        files[name] = member


def _verify_record(
    archive: zipfile.ZipFile,
    members: Mapping[str, zipfile.ZipInfo],
    record_name: str,
) -> None:
    """Verify that wheel RECORD authenticates the exact archive member set."""
    try:
        record_text = _zip_file_bytes(archive, members[record_name]).decode(
            UTF8_ENCODING,
        )
        rows = tuple(csv.reader(io.StringIO(record_text, newline=CSV_NEWLINE)))
    except (UnicodeDecodeError, csv.Error) as error:
        message = f"wheel RECORD is not valid UTF-8 CSV: {error}"
        raise DistributionArchiveError(message) from error
    if any(len(row) != RECORD_FIELD_COUNT for row in rows):
        raise DistributionArchiveError(RECORD_FIELD_ERROR)
    records = {row[0]: (row[1], row[2]) for row in rows}
    if len(records) != len(rows):
        raise DistributionArchiveError(RECORD_DUPLICATE_ERROR)
    contract_module.require_exact_set(WHEEL_RECORD_LABEL, set(records), set(members))
    for name, values in records.items():
        _verify_record_entry(archive, members, record_name, name, values)


def _verify_record_entry(
    archive: zipfile.ZipFile,
    members: Mapping[str, zipfile.ZipInfo],
    record_name: str,
    name: str,
    values: tuple[str, str],
) -> None:
    """Verify one RECORD row against its wheel member."""
    recorded_hash, recorded_size = values
    if name == record_name:
        if recorded_hash or recorded_size:
            raise DistributionArchiveError(RECORD_SELF_HASH_ERROR)
        return
    content = _zip_file_bytes(archive, members[name])
    digest = base64.urlsafe_b64encode(hashlib.sha256(content).digest()).removesuffix(
        BASE64_PADDING,
    )
    expected_hash = f"sha256={digest.decode(ASCII_ENCODING)}"
    if recorded_hash != expected_hash or recorded_size != str(len(content)):
        message = f"wheel RECORD digest or size mismatch: {name}"
        raise DistributionArchiveError(message)


def _compare_zip_sources(
    archive: zipfile.ZipFile,
    members: Mapping[str, zipfile.ZipInfo],
    sources: Mapping[str, Path],
) -> None:
    """Require every installed wheel source to match repository bytes."""
    for name, source in sources.items():
        with archive.open(members[name]) as extracted:
            matches = contract_module.same_content(extracted, source, CHUNK_SIZE)
        if not matches:
            message = f"wheel content differs from repository: {name}"
            raise DistributionArchiveError(message)


def _inspect_wheel(archive: zipfile.ZipFile, contract: ArchiveContract) -> bytes:
    """Inspect one already-open wheel.

    Returns:
        The verified generated ``METADATA`` bytes.

    """
    sources = contract.wheel_sources()
    expected_files = set(sources) | set(contract.wheel_generated_files())
    expected_roots = {PurePosixPath(name).parts[0] for name in expected_files}
    members: dict[str, zipfile.ZipInfo] = {}
    directories: set[str] = set()
    for member in archive.infolist():
        _add_zip_member(member, expected_roots, members, directories)
    contract_module.require_exact_set(WHEEL_LABEL, set(members), expected_files)
    _check_directories(WHEEL_DIRECTORY_LABEL, directories, expected_files)
    _compare_zip_sources(archive, members, sources)
    metadata_name = f"{contract.dist_info}/METADATA"
    metadata = _zip_file_bytes(archive, members[metadata_name])
    _verify_record(archive, members, f"{contract.dist_info}/RECORD")
    _verify_generated_wheel_files(archive, members, contract)
    return metadata


def _verify_generated_wheel_files(
    archive: zipfile.ZipFile,
    members: Mapping[str, zipfile.ZipInfo],
    contract: ArchiveContract,
) -> None:
    """Require WHEEL and console-entry-point content to match the contract."""
    wheel_name = f"{contract.dist_info}/WHEEL"
    wheel_text = _zip_file_bytes(archive, members[wheel_name]).decode(UTF8_ENCODING)
    if wheel_text != contract.expected_wheel_metadata():
        message = f"wheel compatibility contract mismatch: {wheel_name}"
        raise DistributionArchiveError(message)
    entry_points_name = f"{contract.dist_info}/entry_points.txt"
    if entry_points_name in members:
        actual = _zip_file_bytes(archive, members[entry_points_name]).decode(
            UTF8_ENCODING,
        )
        if actual != contract.expected_entry_points():
            message = f"wheel console entry points mismatch: {entry_points_name}"
            raise DistributionArchiveError(message)


def _verify_wheel(archive_path: Path, contract: ArchiveContract) -> bytes:
    """Verify the exact safe wheel member set, source bytes, and RECORD.

    Returns:
        The verified generated ``METADATA`` bytes.

    """
    try:
        with zipfile.ZipFile(archive_path) as archive:
            return _inspect_wheel(archive, contract)
    except (OSError, zipfile.BadZipFile) as error:
        message = f"cannot inspect wheel {archive_path}: {error}"
        raise DistributionArchiveError(message) from error


def _parse_metadata(metadata: bytes) -> Message:
    """Return parsed core metadata.

    Returns:
        The parsed generated metadata message.

    """
    try:
        message: Message = BytesParser(policy=policy.compat32).parsebytes(metadata)
    except (UnicodeError, ValueError) as error:
        message_text = f"cannot parse generated core metadata: {error}"
        raise DistributionArchiveError(message_text) from error
    return message


def verify_distribution_archives(
    source_archive: Path,
    wheel: Path,
    repository_root: Path,
    project_config: Path,
    public_files: Sequence[Path],
) -> None:
    """Verify sdist and wheel safety, exact surfaces, bytes, and identity."""
    contract = contract_module.ArchiveContract.from_project(
        repository_root,
        project_config,
        public_files,
    )
    source_metadata = _verify_source_archive(source_archive, contract)
    wheel_metadata = _verify_wheel(wheel, contract)
    if source_metadata != wheel_metadata:
        raise DistributionArchiveError(METADATA_MISMATCH_ERROR)
    contract.verify_metadata(_parse_metadata(source_metadata))
