"""CFWS-aware RFC message identifier parsing for related MIME compounds."""

from __future__ import annotations

from typing import Final

from .domain import AppError, ExitCode

BACKSLASH: Final = ord("\\")
OPEN_PAREN: Final = ord("(")
CLOSE_PAREN: Final = ord(")")
OPEN_ANGLE: Final = ord("<")


def _skip_cfws(value: bytes, position: int = 0) -> int:
    """Advance across RFC comment or folding white space without normalizing tokens.

    Returns:
        The offset at the first non-CFWS byte or the source end.

    Raises:
        AppError: If a CFWS scanner fails to advance its bounded cursor.

    """
    source_end = len(value)
    while position < source_end:
        prior_position = position
        position = _skip_folding_white_space(value, position)
        if position < prior_position:
            raise AppError(ExitCode.PARSE_ERROR, "malformed MIME comment")
        if position == source_end or value[position] != OPEN_PAREN:
            return position
        next_position = _skip_comment(value, position)
        if next_position <= position:
            raise AppError(ExitCode.PARSE_ERROR, "malformed MIME comment")
        position = next_position
    return position


def _skip_folding_white_space(value: bytes, position: int) -> int:
    """Return the offset after contiguous RFC white space.

    Returns:
        The source offset after contiguous white-space bytes.

    """
    return len(value) - len(value[position:].lstrip(b" \t\r\n"))


def _skip_comment(value: bytes, position: int) -> int:
    """Return the offset after one nested RFC comment.

    Returns:
        The source offset immediately after the matching closing parenthesis.

    Raises:
        AppError: If quote-pair or nesting syntax is not closed.

    """
    depth = 1
    escaped_position: int | None = None
    start = position + 1
    for index, byte in enumerate(value[start:], start):
        if escaped_position is not None:
            escaped_position = None
        elif byte == BACKSLASH:
            escaped_position = index
        elif byte == OPEN_PAREN:
            depth += 1
        elif byte == CLOSE_PAREN:
            depth -= 1
            if depth == 0:
                return index + 1
        elif byte in b"\r\n":
            raise AppError(ExitCode.PARSE_ERROR, "malformed MIME comment")
    raise AppError(ExitCode.PARSE_ERROR, "unterminated MIME comment")


def first_non_cfws(value: bytes) -> int:
    """Return the first significant position while validating leading CFWS.

    Returns:
        The source offset after leading comments and folding white space.

    """
    return _skip_cfws(value)


def _message_identifier_at(
    value: bytes, position: int, *, field: str
) -> tuple[bytes, int]:
    """Parse one bracketed identifier and return its exact inner octets and cursor.

    Returns:
        The exact identifier octets and cursor immediately after its close bracket.

    Raises:
        AppError: If the identifier is empty, malformed, or contains CFWS.

    """
    if position >= len(value) or value[position] != OPEN_ANGLE:
        raise AppError(ExitCode.PARSE_ERROR, f"malformed {field}")
    after_open = value[position + 1 :]
    identifier, closing, _remainder = after_open.partition(b">")
    if not closing:
        raise AppError(ExitCode.PARSE_ERROR, f"malformed {field}")
    if not identifier or any(byte in b"<> \t\r\n" for byte in identifier):
        raise AppError(ExitCode.PARSE_ERROR, f"malformed {field}")
    return identifier, len(value) - len(after_open) + len(identifier) + len(closing)


def parse_message_identifier(value: bytes, *, field: str) -> bytes:
    """Parse one CFWS-wrapped bracketed message identifier without rewriting it.

    Returns:
        The exact inner identifier octets, excluding only wrapper syntax.

    Raises:
        AppError: If CFWS or angle-bracket syntax is malformed.

    """
    position = _skip_cfws(value)
    identifier, position = _message_identifier_at(value, position, field=field)
    if _skip_cfws(value, position) != len(value):
        raise AppError(ExitCode.PARSE_ERROR, f"malformed {field}")
    return identifier


def parse_message_identifier_sequence(value: bytes, *, field: str) -> tuple[bytes, ...]:
    """Parse one or more CFWS-separated identifiers with no literal trailing text.

    Returns:
        Exact identifier octets in their input order.

    Raises:
        AppError: If sequence, CFWS, or angle-bracket syntax is malformed.

    """
    result: list[bytes] = []
    position = _skip_cfws(value)
    while position < len(value):
        identifier, position = _message_identifier_at(value, position, field=field)
        result.append(identifier)
        next_position = _skip_cfws(value, position)
        if next_position == position and next_position < len(value):
            raise AppError(ExitCode.PARSE_ERROR, f"malformed {field}")
        position = next_position
    if not result:
        raise AppError(ExitCode.PARSE_ERROR, f"malformed {field}")
    return tuple(result)
