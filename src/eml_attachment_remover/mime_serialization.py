"""Parse, serialize, and verify safely rewritable MIME messages."""

from __future__ import annotations

import hashlib
from collections import Counter
from email import errors, policy
from email.generator import BytesGenerator
from email.parser import BytesParser
from typing import TYPE_CHECKING, BinaryIO, Final

from .mime_locations import CONTENT_LOCATION_HEADER
from .mime_references import CONTENT_ID_HEADER
from .mime_text_only import _plan_text_only
from .models import (
    CliError,
    ExitCode,
    LeafFingerprint,
    MimePath,
    _format_mime_path,
)

if TYPE_CHECKING:
    from collections.abc import Iterator
    from email.message import EmailMessage
    from pathlib import Path

type HeaderFingerprint = tuple[tuple[str, str], ...]
type PartStructureFingerprint = tuple[
    MimePath,
    str,
    tuple[tuple[str, str], ...],
    HeaderFingerprint,
    str | None,
    str | None,
    str | None,
]

PARSE_POLICY: Final = policy.default.clone(refold_source="none")
OUTPUT_POLICY: Final = policy.SMTP.clone(refold_source="none")
UNSAFE_DEFECT_TYPES: Final = (
    errors.CloseBoundaryNotFoundDefect,
    errors.InvalidBase64CharactersDefect,
    errors.InvalidBase64LengthDefect,
    errors.InvalidBase64PaddingDefect,
    errors.InvalidMultipartContentTransferEncodingDefect,
    errors.MissingHeaderBodySeparatorDefect,
    errors.MultipartInvariantViolationDefect,
    errors.NoBoundaryInMultipartDefect,
    errors.StartBoundaryNotFoundDefect,
)
MAX_MIME_DEPTH: Final = 100


def _iter_parts(message: EmailMessage) -> Iterator[tuple[MimePath, EmailMessage]]:
    """Iterate the MIME tree in depth-first path order without recursion.

    Yields:
        Each stable zero-based path and its MIME entity.

    """
    pending: list[tuple[MimePath, EmailMessage]] = [((), message)]
    while pending:
        path, part = pending.pop()
        yield path, part
        payload = part.get_payload()
        if isinstance(payload, list):
            pending.extend(
                ((*path, index), raw_child)
                for index, raw_child in reversed(tuple(enumerate(payload)))
            )


def _validate_mime_depth(message: EmailMessage) -> None:
    """Reject MIME trees deep enough to exhaust recursive policy operations.

    Raises:
        CliError: If an entity exceeds the supported nesting depth.

    """
    for path, _part in _iter_parts(message):
        if len(path) > MAX_MIME_DEPTH:
            raise CliError(
                ExitCode.PARSE_ERROR,
                f"MIME tree exceeds maximum supported depth {MAX_MIME_DEPTH} "
                f"at MIME path {_format_mime_path(path)}",
            )


def _payload_bytes(part: EmailMessage) -> bytes:
    """Return deterministic bytes for one non-multipart payload.

    Returns:
        Decoded bytes, with a safe fallback for unencoded text.

    """
    decoded = part.get_payload(decode=True)
    if isinstance(decoded, bytes):
        return decoded
    payload = part.get_payload()
    if isinstance(payload, bytes):
        return payload
    if isinstance(payload, str):
        return payload.encode(errors="surrogateescape")
    return b""


def _leaf_fingerprints(message: EmailMessage) -> Counter[LeafFingerprint]:
    """Fingerprint every leaf's path, metadata, and decoded payload.

    Returns:
        A multiset of leaf metadata and SHA-256 payload digests.

    """
    fingerprints: Counter[LeafFingerprint] = Counter()

    def visit(part: EmailMessage, path: MimePath) -> None:
        if part.is_multipart():
            payload = part.get_payload()
            if isinstance(payload, list):
                for index, raw_child in enumerate(payload):
                    visit(raw_child, (*path, index))
            return
        payload = _payload_bytes(part)
        if part.get_content_maintype() == "text":
            payload = payload.replace(b"\r\n", b"\n").replace(b"\r", b"\n")
        digest = hashlib.sha256(payload).hexdigest()
        fingerprints[
            path,
            part.get_content_type(),
            part.get_filename(),
            part.get(CONTENT_ID_HEADER),
            part.get(CONTENT_LOCATION_HEADER),
            digest,
        ] = 1

    visit(message, ())
    return fingerprints


def _structure_fingerprint(
    message: EmailMessage,
) -> tuple[PartStructureFingerprint, ...]:
    """Capture MIME-tree shape, semantic headers, and the envelope marker.

    Returns:
        An ordered, path-bound description of every MIME node.

    """
    fingerprints: list[PartStructureFingerprint] = []

    def normalized_auxiliary(value: str | None) -> str | None:
        if value is None:
            return None
        normalized = value.replace("\r\n", "\n").replace("\r", "\n")
        return normalized or None

    def visit(part: EmailMessage, path: MimePath) -> None:
        parameter_items = part.get_params()
        if parameter_items:
            _media_type, *semantic_parameters = parameter_items
        else:
            semantic_parameters = []
        parameters = tuple(
            (str(name).casefold(), str(value))
            for name, value in semantic_parameters
            if str(name).casefold() != "boundary"
        )
        headers = tuple(
            (name.casefold(), str(value))
            for name, value in part.raw_items()
            if name.casefold() != "content-type"
        )
        fingerprints.append(
            (
                path,
                part.get_content_type(),
                parameters,
                headers,
                part.get_unixfrom(),
                normalized_auxiliary(part.preamble),
                normalized_auxiliary(part.epilogue),
            ),
        )
        payload = part.get_payload()
        if isinstance(payload, list):
            for index, raw_child in enumerate(payload):
                visit(raw_child, (*path, index))

    visit(message, ())
    return tuple(fingerprints)


def _iter_defects(message: EmailMessage) -> list[tuple[MimePath, errors.MessageDefect]]:
    """Return every parser defect together with its MIME-tree path.

    Returns:
        Parser defects paired with their zero-based MIME-tree paths.

    """
    return [
        (path, defect) for path, part in _iter_parts(message) for defect in part.defects
    ]


def _validate_defects(message: EmailMessage) -> tuple[str, ...]:
    """Reject unsafe parser defects and retain lesser defects as warnings.

    Returns:
        Human-readable warnings for non-structural parser defects.

    Raises:
        CliError: If the MIME structure or transfer encoding is unsafe to rewrite.

    """
    warnings: list[str] = []
    for path, defect in _iter_defects(message):
        description = f"{type(defect).__name__} at MIME path {_format_mime_path(path)}"
        if isinstance(defect, UNSAFE_DEFECT_TYPES):
            raise CliError(
                ExitCode.PARSE_ERROR,
                f"unsafe MIME structure or transfer encoding: {description}",
            )
        warnings.append(f"parser reported {description}")
    return tuple(warnings)


def _parse_message(
    raw: bytes,
    source_label: str,
) -> tuple[EmailMessage, tuple[str, ...]]:
    """Parse EML bytes and validate that rewriting is safe.

    Returns:
        The parsed message and any non-fatal parser warnings.

    Raises:
        CliError: If parsing fails or an unsafe MIME defect is found.

    """
    try:
        parsed = BytesParser(policy=PARSE_POLICY).parsebytes(raw)
    except RecursionError as exc:
        raise CliError(
            ExitCode.PARSE_ERROR,
            f"could not parse {source_label}: MIME nesting exceeded parser capacity",
        ) from exc
    except (errors.MessageParseError, UnicodeError, ValueError) as exc:
        raise CliError(
            ExitCode.PARSE_ERROR,
            f"could not parse {source_label}: {exc}",
        ) from exc
    _validate_mime_depth(parsed)
    for _path, part in _iter_parts(parsed):
        if not part.is_multipart():
            _payload_bytes(part)
    return parsed, _validate_defects(parsed)


def _serialize_to_stream(message: EmailMessage, output: BinaryIO) -> None:
    """Serialize a parsed message to an open binary stream."""
    generator = BytesGenerator(
        output,
        policy=OUTPUT_POLICY,
        maxheaderlen=0,
    )
    generator.flatten(
        message,
        unixfrom=message.get_unixfrom() is not None,
    )


def _verify_serialized_message(
    temporary: Path,
    expected_fingerprints: Counter[LeafFingerprint],
    expected_structure: tuple[PartStructureFingerprint, ...] | None = None,
) -> None:
    """Reparse generated output and verify every retained payload.

    Raises:
        CliError: If generated output cannot be read, parsed, or verified.

    """
    try:
        raw = temporary.read_bytes()
    except OSError as exc:
        raise CliError(
            ExitCode.VERIFICATION_ERROR,
            f"could not read generated output for verification: {exc}",
        ) from exc
    try:
        message, _warnings = _parse_message(raw, str(temporary))
    except CliError as exc:
        raise CliError(
            ExitCode.VERIFICATION_ERROR,
            f"generated EML failed MIME verification: {exc.message}",
        ) from exc
    if _leaf_fingerprints(message) != expected_fingerprints:
        raise CliError(
            ExitCode.VERIFICATION_ERROR,
            "generated EML did not preserve every retained MIME payload",
        )
    if (
        expected_structure is not None
        and _structure_fingerprint(message) != expected_structure
    ):
        raise CliError(
            ExitCode.VERIFICATION_ERROR,
            "generated EML did not preserve the retained MIME structure and headers",
        )
    try:
        text_only_plan = _plan_text_only(message)
    except CliError as exc:
        raise CliError(
            ExitCode.VERIFICATION_ERROR,
            f"generated EML is not a safe text-only message: {exc.message}",
        ) from exc
    if text_only_plan.modified:
        raise CliError(
            ExitCode.VERIFICATION_ERROR,
            "generated EML is not a canonical root text/plain message",
        )
