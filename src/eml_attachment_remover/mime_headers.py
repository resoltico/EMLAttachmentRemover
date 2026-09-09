"""Physical MIME header indexing and singleton validation."""

from __future__ import annotations

import re
from dataclasses import dataclass
from typing import Final

from .domain import AppError, ExitCode

MAX_HEADERS: Final = 512
MAX_HEADER_BYTES: Final = 256 * 1024
SINGLETONS: Final = frozenset({
    b"content-type",
    b"content-transfer-encoding",
    b"content-disposition",
    b"content-id",
    b"content-location",
    b"content-base",
    b"mime-version",
})
TOKEN_RE: Final = re.compile(rb"^[!#$%&'*+\-.^_`|~0-9A-Za-z]+$")


@dataclass(frozen=True, slots=True)
class Header:
    """One complete physical header field, including its wire span."""

    name: bytes
    value: bytes
    start: int
    end: int


def is_header_name(value: bytes) -> bool:
    """Return whether bytes are an RFC token suitable for a physical header name.

    Returns:
        Whether the nonempty byte sequence uses only RFC token characters.

    """
    return bool(value) and TOKEN_RE.fullmatch(value) is not None


def line_end(raw: bytes, position: int, end: int) -> int:
    """Return the first position after one wire line.

    Returns:
        The exclusive wire offset after a CRLF, LF, CR, or final partial line.

    """
    lf = raw.find(b"\n", position, end)
    cr = raw.find(b"\r", position, end)
    positions = [line_end for line_end in (lf, cr) if line_end >= 0]
    if not positions:
        return end
    newline = min(positions)
    if raw[newline : newline + 2] == b"\r\n":
        return newline + 2
    return newline + 1


def _advanced_cursor(position: int, next_position: int) -> int:
    """Return a strictly advanced header cursor or fail closed.

    Returns:
        The validated next header offset.

    Raises:
        AppError: If the proposed offset does not strictly advance.

    """
    if next_position <= position:
        raise AppError(ExitCode.PARSE_ERROR, "MIME header cursor did not advance")
    return next_position


def parse_headers(raw: bytes, start: int, separator: int) -> tuple[Header, ...]:
    """Parse physical fields without unfolding or silently repairing them.

    Returns:
        Complete source-indexed physical header fields.

    Raises:
        AppError: If a field is malformed or exceeds a public header budget.

    """
    if separator - start > MAX_HEADER_BYTES:
        raise AppError(ExitCode.PARSE_ERROR, "MIME entity exceeds header-byte limit")
    headers: list[Header] = []
    position = _first_header_offset(raw, start, separator)
    for _line in range(separator - position + 1):
        if position >= separator:
            break
        end = _advanced_cursor(position, line_end(raw, position, separator))
        line = raw[position:end].rstrip(b"\r\n")
        _append_header(headers, line, position, end)
        position = end
    else:
        raise AppError(ExitCode.PARSE_ERROR, "MIME header cursor did not advance")
    _validate_header_multiplicity(headers)
    return tuple(headers)


def _first_header_offset(raw: bytes, start: int, separator: int) -> int:
    """Return the first field offset after an optional mbox envelope line.

    Returns:
        The offset at the first MIME field, after a leading mbox envelope if present.

    """
    return line_end(raw, start, separator) if raw.startswith(b"From ", start) else start


def _append_header(headers: list[Header], line: bytes, start: int, end: int) -> None:
    """Append one physical field or continuation while enforcing its entity limit.

    Raises:
        AppError: If field syntax, continuation ownership, or count is invalid.

    """
    if not line:
        return
    if line[:1] in {b" ", b"\t"}:
        _append_continuation(headers, line, end)
    else:
        headers.append(_new_header(line, start, end))
    if len(headers) > MAX_HEADERS:
        raise AppError(ExitCode.PARSE_ERROR, "MIME entity exceeds header-count limit")


def _append_continuation(headers: list[Header], line: bytes, end: int) -> None:
    """Add one physical continuation to the immediately preceding field.

    Raises:
        AppError: If no prior field owns the continuation.

    """
    if not headers:
        raise AppError(ExitCode.PARSE_ERROR, "orphaned MIME header continuation")
    previous = headers[-1]
    headers[-1] = Header(
        previous.name, previous.value + b"\r\n" + line, previous.start, end
    )


def _new_header(line: bytes, start: int, end: int) -> Header:
    """Parse one noncontinuation physical header field.

    Returns:
        The source-indexed normalized-name physical field.

    Raises:
        AppError: If the field name is not an RFC token.

    """
    name, colon, value = line.partition(b":")
    if not colon or not is_header_name(name):
        raise AppError(ExitCode.PARSE_ERROR, "malformed MIME header field")
    return Header(name.lower(), value.strip(b" \t"), start, end)


def _validate_header_multiplicity(headers: list[Header]) -> None:
    """Reject duplicate controls before structured parsing can hide them.

    Raises:
        AppError: If a singleton MIME control appears more than once.

    """
    counts: dict[bytes, int] = {}
    for header in headers:
        counts[header.name] = counts.get(header.name, 0) + 1
    duplicate = next((name for name in SINGLETONS if counts.get(name, 0) > 1), None)
    if duplicate is not None:
        raise AppError(
            ExitCode.PARSE_ERROR,
            f"duplicate singleton MIME header {duplicate.decode('ascii')}",
        )
