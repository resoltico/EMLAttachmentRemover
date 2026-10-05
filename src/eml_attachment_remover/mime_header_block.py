"""Locate one MIME entity's header block, its separator, and its body start."""

from __future__ import annotations

from typing import Final

from .domain import AppError, ExitCode
from .mime_headers import (
    Header,
    is_header_name,
    line_end,
    parse_headers,
    root_header_start,
)

_SEPARATOR_MISSING: Final = "header-like MIME entity lacks a body separator"


def _find_separator(raw: bytes, start: int, end: int) -> tuple[int, int]:
    """Find the exact header/body separator in one entity.

    Returns:
        The separator start offset and its byte length.

    Raises:
        AppError: If the entity has no physical separator.

    """
    matches = [
        (raw.find(marker, start, end), len(marker))
        for marker in (b"\r\n\r\n", b"\n\n", b"\r\r")
    ]
    positions = [(position, length) for position, length in matches if position >= 0]
    if not positions:
        raise AppError(ExitCode.PARSE_ERROR, "MIME entity has no header/body separator")
    position, length = min(positions)
    # The first newline ends the last field; only the blank line is the separator.
    return position + length // 2, length // 2


def entity_headers(
    raw: bytes, start: int, end: int, *, root: bool = False
) -> tuple[tuple[Header, ...], int, int]:
    """Return physical headers and exact body start, accepting a headerless entity.

    Returns:
        Parsed fields, body start offset, and physical header byte count.

    Raises:
        AppError: If a header-like entity lacks a valid physical separator.

    """
    header_start = root_header_start(raw, start, end) if root else start
    if not _first_line_is_header_like(raw, header_start, end):
        # An empty first line is an empty header block's separator (RFC 2046), not
        # payload; any other non-header first line starts a headerless body.
        return (), start + _line_ending_length(raw, start, end), 0
    try:
        separator, separator_length = _find_separator(raw, header_start, end)
    except AppError as exc:
        if root:
            return _fields_only_message(raw, header_start, end, start)
        raise AppError(ExitCode.PARSE_ERROR, _SEPARATOR_MISSING) from exc
    headers = parse_headers(raw, header_start, separator)
    return headers, separator + separator_length, separator - start


def _fields_only_message(
    raw: bytes, header_start: int, end: int, start: int
) -> tuple[tuple[Header, ...], int, int]:
    """Accept a root message made only of complete fields, with an empty body.

    RFC 5322 makes the body and its separator optional; nested MIME parts keep
    failing closed without one.

    Returns:
        The fields, an empty body at ``end``, and the header byte count.

    Raises:
        AppError: If any line up to the end is not a complete header field.

    """
    try:
        headers = parse_headers(raw, header_start, end)
    except AppError as exc:
        raise AppError(ExitCode.PARSE_ERROR, _SEPARATOR_MISSING) from exc
    return headers, end, end - start


def _line_ending_length(raw: bytes, start: int, end: int) -> int:
    """Return the length of a line ending at ``start``, or 0 for any other byte.

    Returns:
        2 for CRLF, 1 for a lone LF or CR, otherwise 0.

    """
    if raw[start : min(start + 2, end)] == b"\r\n":
        return 2
    return 1 if raw[start : min(start + 1, end)] in {b"\n", b"\r"} else 0


def _first_line_is_header_like(raw: bytes, start: int, end: int) -> bool:
    """Return whether an entity begins with a colon-bearing physical header line.

    Returns:
        Whether its initial physical line can be interpreted as a header field.

    """
    first_line = raw[start : line_end(raw, start, end)]
    name, colon, _value = first_line.partition(b":")
    return bool(colon) and is_header_name(name)
