"""Bounded RFC-style comments for MIME fields that explicitly permit CFWS."""

from __future__ import annotations

from typing import Final

from .domain import AppError, ExitCode

BACKSLASH: Final = ord("\\")
OPEN_PAREN: Final = ord("(")
CLOSE_PAREN: Final = ord(")")
DOUBLE_QUOTE: Final = ord('"')
SPACE: Final = ord(" ")
DELETE: Final = 127
MAX_COMMENT_DEPTH: Final = 8
_BOUNDARY: Final = b" \t\r\n;=/"
_CFWS_BYTES: Final = frozenset({b" ", b"\t", b"\r", b"\n"})


def without_comments(value: bytes, *, allowed: bool) -> bytes:
    """Replace legal comments with CFWS while retaining quoted-string octets.

    Returns:
        Semantic field bytes whose comments have been replaced by one space.

    """
    result = bytearray()
    position = 0
    while position < len(value):
        position = _copy_one(value, position, result, allowed=allowed)
    return bytes(result)


def _copy_one(value: bytes, position: int, result: bytearray, *, allowed: bool) -> int:
    """Copy one raw unit or replace one independently bounded comment.

    Returns:
        The strictly advanced source cursor.

    Raises:
        AppError: If a comment is forbidden or malformed.

    """
    byte = value[position]
    if byte == DOUBLE_QUOTE:
        return _copy_quoted(value, position, result)
    if byte == OPEN_PAREN:
        return _remove_comment(value, position, result, allowed=allowed)
    if byte == CLOSE_PAREN:
        raise AppError(ExitCode.PARSE_ERROR, "malformed MIME comment")
    result.append(byte)
    return position + 1


def _copy_quoted(value: bytes, position: int, result: bytearray) -> int:
    """Copy one complete quoted string without interpreting comment punctuation.

    Returns:
        The cursor immediately after the terminal quote.

    Raises:
        AppError: If the string or its final quoted pair is unfinished.

    """
    result.append(DOUBLE_QUOTE)
    positions = iter(enumerate(memoryview(value)[position + 1 :]))
    for offset, byte in positions:
        if byte == BACKSLASH:
            escaped_position = next(positions, None)
            if escaped_position is None:
                break
            result.extend((byte, escaped_position[1]))
        elif byte == DOUBLE_QUOTE:
            result.append(byte)
            return _advanced_quoted_cursor(position, position + offset + 2)
        else:
            result.append(byte)
    raise AppError(ExitCode.PARSE_ERROR, "unterminated MIME quoted parameter")


def _advanced_quoted_cursor(position: int, next_position: int) -> int:
    """Require one quoted-string parser completion to advance its caller.

    Returns:
        The strictly advanced quoted-string completion position.

    Raises:
        AppError: If a parser implementation proposes a non-advancing cursor.

    """
    if next_position <= position:
        raise AppError(ExitCode.PARSE_ERROR, "nonadvancing MIME quoted cursor")
    return next_position


def _remove_comment(
    value: bytes, position: int, result: bytearray, *, allowed: bool
) -> int:
    """Validate and replace one legal comment with a single CFWS byte.

    Returns:
        The cursor immediately after the closing parenthesis.

    Raises:
        AppError: If the comment lacks a legal MIME field boundary.

    """
    if not allowed:
        raise AppError(ExitCode.PARSE_ERROR, "MIME comments are not permitted")
    if not _before_comment_boundary(value, position):
        raise AppError(ExitCode.PARSE_ERROR, "comment occurs inside MIME token")
    end = _comment_end(value, position)
    if not _after_comment_boundary(value, end):
        raise AppError(ExitCode.PARSE_ERROR, "comment occurs inside MIME token")
    if not result or not result.endswith(tuple(_CFWS_BYTES)):
        result.append(SPACE)
    return end


def _before_comment_boundary(value: bytes, position: int) -> bool:
    """Return whether a comment begins after a legal semantic boundary.

    Returns:
        Whether the edge is at the field start or follows RFC white space or a
        structured-field delimiter.

    """
    return position == 0 or value[position - 1] in _BOUNDARY


def _after_comment_boundary(value: bytes, position: int) -> bool:
    """Return whether a comment ends before a legal semantic boundary.

    Returns:
        Whether the edge is at the field end or precedes RFC white space or a
        structured-field delimiter.

    """
    return position == len(value) or value[position] in _BOUNDARY


def _comment_end(value: bytes, start: int) -> int:
    """Consume one nested, quoted-pair-aware comment without retaining its text.

    Returns:
        The cursor immediately after the matching close parenthesis.

    Raises:
        AppError: If a quoted pair, fold, control, or nesting level is invalid.

    """
    depth = 1
    position = start + 1
    for _step in range(len(value) - position):
        next_position, depth = _comment_step(value, position, depth)
        if next_position <= position:
            raise AppError(ExitCode.PARSE_ERROR, "nonadvancing MIME comment cursor")
        position = next_position
        if depth == 0:
            return position
    raise AppError(ExitCode.PARSE_ERROR, "unterminated MIME comment")


def _comment_step(value: bytes, position: int, depth: int) -> tuple[int, int]:
    """Advance one comment grammar unit while carrying the bounded nesting depth.

    Returns:
        The advanced cursor and its next nesting depth.

    Raises:
        AppError: If the next comment unit is malformed or exceeds the budget.

    """
    byte = value[position]
    if byte == BACKSLASH:
        return _escaped_comment_cursor(value, position), depth
    if byte == OPEN_PAREN:
        if depth == MAX_COMMENT_DEPTH:
            raise AppError(ExitCode.PARSE_ERROR, "MIME comment nesting exceeds limit")
        return position + 1, depth + 1
    if byte == CLOSE_PAREN:
        return position + 1, depth - 1
    if byte in b"\r\n":
        return _folded_comment_cursor(value, position), depth
    if byte < SPACE or byte == DELETE:
        raise AppError(ExitCode.PARSE_ERROR, "malformed MIME comment")
    return position + 1, depth


def _escaped_comment_cursor(value: bytes, position: int) -> int:
    """Return the cursor after one complete quoted pair inside a comment.

    Returns:
        The cursor after the escaped byte.

    Raises:
        AppError: If the comment ends immediately after its escape character.

    """
    if position + 1 == len(value):
        raise AppError(ExitCode.PARSE_ERROR, "unterminated MIME comment")
    return position + 2


def _folded_comment_cursor(value: bytes, position: int) -> int:
    """Consume exactly one legal folding-white-space line break in a comment.

    Returns:
        The cursor after the required following white space.

    Raises:
        AppError: If the line break is not folding white space.

    """
    next_position = position + 2
    if (
        value[position:next_position] != b"\r\n"
        or next_position == len(value)
        or value[next_position] not in b" \t"
    ):
        raise AppError(ExitCode.PARSE_ERROR, "malformed MIME comment")
    for cursor in range(next_position, len(value)):
        if value[cursor] not in b" \t":
            return cursor
    return len(value)
