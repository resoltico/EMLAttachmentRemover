"""Coordinate MIME processing for one source EML file."""

from __future__ import annotations

import os
import stat
from email.message import Message
from typing import TYPE_CHECKING, Final

from .mime_policy import _remove_attachments
from .mime_references import _collect_references
from .mime_serialization import _parse_message
from .models import (
    CliError,
    ExitCode,
    MimePath,
    OutputPlan,
    ProcessResult,
    RemovedPart,
    _format_mime_path,
)
from .paths import _default_destination, _validate_paths, _validate_source
from .storage import _produce_output

if TYPE_CHECKING:
    from pathlib import Path
    from typing import BinaryIO

STALE_ROOT_HEADERS: Final = (
    "Content-Length",
    "Lines",
    "X-MS-Has-Attach",
)
TRANSPORT_SIGNATURE_HEADERS: Final = (
    "ARC-Message-Signature",
    "ARC-Seal",
    "DKIM-Signature",
    "DomainKey-Signature",
)
type LocatedPart = tuple[MimePath, str | None, Message[str, str]]


def _located_parts(
    part: Message[str, str],
    path: MimePath = (),
    parent_type: str | None = None,
) -> tuple[LocatedPart, ...]:
    """Snapshot MIME entities with their stable pre-mutation paths.

    Returns:
        The complete depth-first part inventory.

    Raises:
        TypeError: If a multipart payload contains a non-message entity.

    """
    located: list[LocatedPart] = [(path, parent_type, part)]
    payload = part.get_payload()
    if not isinstance(payload, list):
        return tuple(located)
    for index, raw_child in enumerate(payload):
        if not isinstance(raw_child, Message):
            raise TypeError
        located.extend(
            _located_parts(
                raw_child,
                (*path, index),
                part.get_content_type(),
            ),
        )
    return tuple(located)


def _remove_stale_root_headers(message: Message[str, str]) -> None:
    """Delete root headers whose values become false after removal."""
    for header in STALE_ROOT_HEADERS:
        if header in message:
            del message[header]


def _transport_signature_warning(
    message: Message[str, str],
    path: MimePath = (),
) -> str | None:
    """Return a warning for transport signatures invalidated by rewriting.

    Returns:
        A warning listing affected headers, or ``None`` when none are present.

    """
    present = [header for header in TRANSPORT_SIGNATURE_HEADERS if header in message]
    if not present:
        return None
    subject = (
        "the message"
        if not path
        else f"nested message at MIME path {_format_mime_path(path)}"
    )
    return (
        f"rewriting {subject} invalidates existing transport signatures "
        f"({', '.join(present)}); the original EML remains unchanged"
    )


def _path_changed(path: MimePath, removed: list[RemovedPart]) -> bool:
    """Return whether removal changed this entity or one of its descendants.

    Returns:
        ``True`` when at least one removed path begins with this part path.

    """
    return any(part.path[: len(path)] == path for part in removed)


def _require_regular_source(input_file: BinaryIO, source: Path) -> None:
    """Require the opened source descriptor itself to be a regular file.

    Raises:
        CliError: If a path race selected a special file for reading.

    """
    if not stat.S_ISREG(os.fstat(input_file.fileno()).st_mode):
        raise CliError(
            ExitCode.INPUT_ERROR,
            f"input path is not a regular file at read time: {source}",
        )


def _read_source(source: Path) -> bytes:
    """Read a regular source through one inspected file descriptor.

    Returns:
        The source EML bytes.

    Raises:
        CliError: If the source cannot be read.

    """
    flags = os.O_RDONLY | getattr(os, "O_BINARY", 0) | getattr(os, "O_NONBLOCK", 0)
    try:
        descriptor = os.open(source, flags)
        with os.fdopen(descriptor, "rb") as input_file:
            _require_regular_source(input_file, source)
            return input_file.read()
    except CliError:
        raise
    except OSError as exc:
        raise CliError(
            ExitCode.INPUT_ERROR,
            f"could not read input file {source}: {exc}",
        ) from exc


def _removal_warnings(
    located_parts: tuple[LocatedPart, ...],
    protected_types: set[str],
    parse_warnings: tuple[str, ...],
    removed: list[RemovedPart],
) -> list[str]:
    """Collect parser, protected-content, and signature warnings.

    Returns:
        The warnings that apply to this processing result.

    """
    warnings = list(parse_warnings)
    if protected_types:
        warnings.append(
            f"protected MIME entity left intact: {', '.join(sorted(protected_types))}",
        )
    for path, parent_type, part in located_parts:
        if not _path_changed(path, removed):
            continue
        _remove_stale_root_headers(part)
        is_logical_message = not path or (
            parent_type is not None and parent_type.startswith("message/")
        )
        if not is_logical_message:
            continue
        signature_warning = _transport_signature_warning(part, path)
        if signature_warning is not None:
            warnings.append(signature_warning)
    return warnings


def process_file(
    source: Path,
    destination: Path | None,
    *,
    force: bool,
    dry_run: bool,
) -> ProcessResult:
    """Remove attachments from one EML and return a verified result.

    Returns:
        The complete processing report.

    """
    requested_destination = destination or _default_destination(source)
    if dry_run:
        source = _validate_source(source)
    else:
        source, requested_destination = _validate_paths(
            source,
            requested_destination,
            force=force,
        )
    raw = _read_source(source)
    message, parse_warnings = _parse_message(raw, str(source))
    located_parts = _located_parts(message)
    state = _remove_attachments(message, _collect_references(message))
    warnings = _removal_warnings(
        located_parts,
        state.protected_types,
        parse_warnings,
        state.removed,
    )
    removed = tuple(state.removed)
    preserved = tuple(state.preserved_file_parts)
    if dry_run:
        return ProcessResult(
            source=source,
            destination=None,
            source_size=len(raw),
            output_size=None,
            dry_run=True,
            removed=removed,
            preserved_file_parts=preserved,
            warnings=tuple(warnings),
        )
    output_size, mode_warning = _produce_output(
        OutputPlan(
            source=source,
            destination=requested_destination,
            message=message,
            raw=raw,
            modified=bool(state.removed),
            force=force,
        ),
    )
    if mode_warning is not None:
        warnings.append(mode_warning)
    return ProcessResult(
        source=source,
        destination=requested_destination,
        source_size=len(raw),
        output_size=output_size,
        dry_run=False,
        removed=removed,
        preserved_file_parts=preserved,
        warnings=tuple(warnings),
    )
