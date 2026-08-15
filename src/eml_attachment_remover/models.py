"""Define the data model and shared console utilities."""

from __future__ import annotations

import argparse
import sys
import unicodedata
from dataclasses import dataclass, field
from enum import IntEnum, StrEnum
from typing import TYPE_CHECKING, Final, Never, TextIO, override

if TYPE_CHECKING:
    from email.message import EmailMessage
    from pathlib import Path

PROGRAM_NAME: Final = "remove-eml-attachments"
SHORT_ESCAPE_BITS: Final = 8
UNICODE_ESCAPE_BITS: Final = 16
UNSAFE_DISPLAY_CATEGORIES: Final = frozenset({"Cc", "Cf", "Cs", "Zl", "Zp"})

type MimePath = tuple[int, ...]
type LeafFingerprint = tuple[
    MimePath,
    str,
    str | None,
    str | None,
    str | None,
    str,
]


class ExitCode(IntEnum):
    """Define stable process exit codes for the command-line interface."""

    SUCCESS = 0
    USAGE = 2
    INPUT_ERROR = 3
    OUTPUT_CONFLICT = 4
    PARSE_ERROR = 5
    PROTECTED_MESSAGE = 6
    WRITE_ERROR = 7
    VERIFICATION_ERROR = 8
    BATCH_FAILURE = 9
    INTERNAL_ERROR = 70
    INTERRUPTED = 130


class KeepReason(StrEnum):
    """Describe why a file-like MIME entity is retained."""

    BODY_CID_REFERENCE = "referenced by cid: from the message body"
    BODY_LOCATION_REFERENCE = "referenced by Content-Location from the message body"
    CONTENT_ID = "Content-ID"
    CONTENT_LOCATION = "Content-Location"
    INLINE_DISPOSITION = "inline disposition"
    RELATED_RESOURCE = "multipart/related resource"
    SECURITY_ENTITY = "cryptographic MIME entity"


class OutputFormat(StrEnum):
    """Define supported command-line report formats."""

    HUMAN = "human"
    JSON = "json"
    PATHS = "paths"
    PATHS0 = "paths0"


@dataclass(frozen=True, slots=True)
class ReferenceIndex:
    """Store body-resource identifiers referenced from HTML content."""

    content_ids: frozenset[str]
    locations: frozenset[str]


@dataclass(frozen=True, slots=True)
class PartContext:
    """Store the structural context of one MIME entity."""

    path: MimePath
    parent_type: str | None
    under_related: bool
    references: ReferenceIndex
    location_base: str | None = None
    is_root: bool = False


@dataclass(frozen=True, slots=True)
class RemovedPart:
    """Store metadata for a MIME entity deleted from the derived message."""

    path: MimePath
    content_type: str
    filename: str | None
    disposition: str | None


@dataclass(frozen=True, slots=True)
class PreservedFilePart:
    """Store metadata for a file-like MIME entity kept as body content."""

    path: MimePath
    content_type: str
    filename: str | None
    reason: KeepReason


@dataclass(slots=True)
class RemovalState:
    """Accumulate state while traversing a MIME tree."""

    removed: list[RemovedPart] = field(default_factory=list)
    preserved_file_parts: list[PreservedFilePart] = field(default_factory=list)
    protected_types: set[str] = field(default_factory=set)


@dataclass(frozen=True, slots=True)
class ProcessResult:
    """Describe the outcome of processing one EML file."""

    source: Path
    destination: Path | None
    source_size: int
    output_size: int | None
    removed: tuple[RemovedPart, ...]
    preserved_file_parts: tuple[PreservedFilePart, ...]
    warnings: tuple[str, ...]
    dry_run: bool


@dataclass(frozen=True, slots=True)
class BatchFailure:
    """Describe one input that could not be processed."""

    source: Path
    code: ExitCode
    message: str


@dataclass(frozen=True, slots=True)
class BatchSkip:
    """Describe one input skipped because its output already existed."""

    source: Path
    destination: Path


@dataclass(frozen=True, slots=True)
class BatchOutcome:
    """Collect all results produced while executing one batch."""

    results: tuple[ProcessResult, ...]
    skips: tuple[BatchSkip, ...]
    failures: tuple[BatchFailure, ...]


@dataclass(frozen=True, slots=True)
class OutputPlan:
    """Store all data needed to produce one output file."""

    source: Path
    destination: Path
    message: EmailMessage
    raw: bytes
    modified: bool
    force: bool


class CliError(Exception):
    """Represent an expected command failure with a stable exit code."""

    def __init__(self, code: ExitCode, message: str) -> None:
        """Initialize an expected command failure."""
        super().__init__(message)
        self.code = code
        self.message = message


class ArgumentParser(argparse.ArgumentParser):
    """Format argument errors consistently with other command failures."""

    @override
    def error(self, message: str) -> Never:
        """Raise a typed usage error for the command-line boundary.

        Raises:
            CliError: Always, with the stable usage exit code.

        """
        raise CliError(ExitCode.USAGE, message)


def _format_mime_path(path: MimePath) -> str:
    """Return a stable human-readable MIME-tree path.

    Returns:
        The one-based dotted path, or ``root`` for the message root.

    """
    return "root" if not path else ".".join(str(index + 1) for index in path)


def _display_text(value: str, stream: TextIO) -> str:
    """Make arbitrary text safe for a terminal and its active encoding.

    Returns:
        Text with controls escaped and unsupported glyphs backslash-escaped.

    """

    def escaped_character(character: str) -> str:
        code_point = ord(character)
        if unicodedata.category(character) not in UNSAFE_DISPLAY_CATEGORIES:
            return character
        bit_length = code_point.bit_length()
        if bit_length <= SHORT_ESCAPE_BITS:
            return f"\\x{code_point:02x}"
        if bit_length <= UNICODE_ESCAPE_BITS:
            return f"\\u{code_point:04x}"
        return f"\\U{code_point:08x}"

    escaped = "".join(escaped_character(character) for character in value)
    encoding = stream.encoding or sys.getdefaultencoding()
    return escaped.encode(encoding, errors="backslashreplace").decode(encoding)


def _write_line(stream: TextIO, text: str) -> None:
    """Write one encoding-safe console line."""
    stream.writelines((_display_text(text, stream), "\n"))


def _write_error(stream: TextIO, code: ExitCode, message: str) -> None:
    """Write one standardized error message."""
    _write_line(stream, f"{PROGRAM_NAME}: error[{code.name}:{int(code)}]: {message}")
