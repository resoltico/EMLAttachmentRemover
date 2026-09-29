"""RFC 2045/2231 structured-field parameters: wire spelling and decoded meaning."""

from __future__ import annotations

from dataclasses import dataclass
from typing import Final

from .domain import AppError, ExitCode
from .mime_headers import TOKEN_RE

BACKSLASH: Final = ord("\\")
DOUBLE_QUOTE: Final = ord('"')
PERCENT: Final = ord("%")
MIN_QUOTED_BYTES: Final = 2
ASCII_DIGIT_START: Final = ord("0")
ASCII_DIGIT_END: Final = ord("9")
ASCII_UPPER_START: Final = ord("A")
ASCII_UPPER_END: Final = ord("Z")
ASCII_LOWER_START: Final = ord("a")
ASCII_LOWER_END: Final = ord("z")
MAX_RFC2231_SEGMENTS: Final = 512
MAX_RFC2231_INDEX_DIGITS: Final = 3
RFC2231_ATTR_PUNCTUATION: Final = frozenset({
    33,
    35,
    36,
    38,
    43,
    45,
    46,
    94,
    95,
    96,
    124,
    126,
})


def _unquote(value: bytes) -> bytes:
    """Return an exact quoted-string value with quoted-pairs decoded once.

    Returns:
        The unquoted parameter byte sequence.

    Raises:
        AppError: If quote or quoted-pair syntax is incomplete.

    """
    if not value.startswith(b'"'):
        return value
    if len(value) < MIN_QUOTED_BYTES or not value.endswith(b'"'):
        raise AppError(ExitCode.PARSE_ERROR, "malformed MIME quoted parameter")
    result = bytearray()
    body = value[1:-1]
    iterator = iter(body)
    for byte in iterator:
        if byte == BACKSLASH:
            try:
                escaped_byte = next(iterator)
            except StopIteration as error:
                raise AppError(
                    ExitCode.PARSE_ERROR, "unterminated MIME quoted-pair"
                ) from error
            result.append(escaped_byte)
        else:
            result.append(byte)
    return bytes(result)


def _parameter_name(name: bytes) -> tuple[bytes, int | None, bool]:
    """Split a regular, extended, or RFC 2231 continuation parameter name.

    Returns:
        Normalized base name, optional continuation number, and extended flag.

    Raises:
        AppError: If the parameter name is not part of the supported grammar.

    """
    base, marker, suffix = name.partition(b"*")
    if not base or not TOKEN_RE.fullmatch(base):
        raise AppError(ExitCode.PARSE_ERROR, "malformed MIME parameter name")
    if not marker:
        return base, None, False
    if not suffix:
        return base, None, True
    encoded = suffix.endswith(b"*")
    index_text = suffix[:-1] if encoded else suffix
    if not index_text.isdigit() or len(index_text) > MAX_RFC2231_INDEX_DIGITS:
        raise AppError(ExitCode.PARSE_ERROR, "malformed MIME parameter extension")
    index = int(index_text)
    if index >= MAX_RFC2231_SEGMENTS:
        raise AppError(
            ExitCode.PARSE_ERROR, "MIME parameter continuation index exceeds limit"
        )
    return base, index, encoded


def _extended_parameter(value: bytes, *, initial: bool) -> bytes:
    """Validate an RFC 2231 extension spelling without charset-decoding its bytes.

    Returns:
        The byte-exact extended parameter spelling.

    Raises:
        AppError: If percent escapes or allowed attribute characters are invalid.

    """
    payload = _extended_payload(value) if initial else value
    for position, byte in enumerate(payload):
        if byte == PERCENT:
            if position + 2 >= len(payload) or any(
                digit not in b"0123456789abcdefABCDEF"
                for digit in payload[position + 1 : position + 3]
            ):
                raise AppError(ExitCode.PARSE_ERROR, "malformed RFC 2231 escape")
        elif _is_rfc2231_attr_char(byte):
            continue
        else:
            raise AppError(ExitCode.PARSE_ERROR, "malformed RFC 2231 parameter")
    return value


def _is_rfc2231_attr_char(byte: int) -> bool:
    """Return whether one byte is a permitted RFC 2231 attribute character.

    Returns:
        Whether the byte belongs to the RFC 2231 attr-char alphabet.

    """
    return (
        ASCII_DIGIT_START <= byte <= ASCII_DIGIT_END
        or ASCII_UPPER_START <= byte <= ASCII_UPPER_END
        or ASCII_LOWER_START <= byte <= ASCII_LOWER_END
        or byte in RFC2231_ATTR_PUNCTUATION
    )


def _extended_payload(value: bytes) -> bytes:
    """Return the RFC 2231 payload after a mandatory charset/language prefix.

    Returns:
        The percent-encoded payload after exactly two apostrophe delimiters.

    Raises:
        AppError: If the initial extended parameter has invalid prefix syntax.

    """
    charset, first_quote, remainder = value.partition(b"'")
    language, second_quote, payload = remainder.partition(b"'")
    language_bytes = (
        bytes(range(48, 58))
        + bytes(range(65, 91))
        + bytes(range(97, 123))
        + bytes((45,))
    )
    # RFC 2231 permits an empty charset; both apostrophes remain mandatory.
    if (
        not first_quote
        or not second_quote
        or (charset and not TOKEN_RE.fullmatch(charset))
    ):
        raise AppError(ExitCode.PARSE_ERROR, "malformed RFC 2231 extended parameter")
    if language and not all(byte in language_bytes for byte in language):
        raise AppError(ExitCode.PARSE_ERROR, "malformed RFC 2231 language")
    return payload


def structured_parameters(
    pieces: list[bytes],
) -> tuple[dict[bytes, bytes], dict[bytes, bytes]]:
    """Parse and join unique ordinary or RFC 2231 continuation parameters.

    Returns:
        Wire values by base name, and decoded values of the extended ones.

    """
    segments: dict[bytes, dict[int | None, _Segment]] = {}
    for piece in pieces:
        name, index, segment = _parameter_piece(piece)
        _store_segment(segments, name, index, segment)
    parameters: dict[bytes, bytes] = {}
    decoded: dict[bytes, bytes] = {}
    for name, parts in segments.items():
        ordered = _ordered_segments(parts)
        parameters[name] = b"".join(part.wire for part in ordered)
        if any(part.encoded for part in ordered):
            decoded[name] = b"".join(part.meaning for part in ordered)
    return parameters, decoded


@dataclass(frozen=True, slots=True)
class _Segment:
    """One parameter segment's wire bytes, meaning, and RFC 2231 encoding flag."""

    wire: bytes
    meaning: bytes
    encoded: bool


def _parameter_piece(piece: bytes) -> tuple[bytes, int | None, _Segment]:
    """Parse one parameter component before duplicate/continuation ownership checks.

    Returns:
        Base name, optional continuation index, and the validated segment.

    Raises:
        AppError: If a parameter component does not use the supported grammar.

    """
    name, equals, raw_value = piece.partition(b"=")
    if not equals:
        raise AppError(ExitCode.PARSE_ERROR, "malformed MIME parameter")
    base_name, index, encoded = _parameter_name(name.strip().lower())
    value = raw_value.strip()
    initial = index in {None, 0}
    wire = _parameter_value(value, encoded=encoded, initial=initial)
    meaning = _percent_decoded(wire, initial=initial) if encoded else wire
    return base_name, index, _Segment(wire, meaning, encoded)


def _percent_decoded(value: bytes, *, initial: bool) -> bytes:
    """Decode one validated RFC 2231 segment exactly once.

    Returns:
        The segment's octets; an initial segment drops its charset/language prefix.

    """
    payload = _extended_payload(value) if initial else value
    result = bytearray()
    position = 0
    while position < len(payload):
        if payload[position] == PERCENT:
            result.append(int(payload[position + 1 : position + 3], 16))
            position += 3
        else:
            result.append(payload[position])
            position += 1
    return bytes(result)


def _parameter_value(value: bytes, *, encoded: bool, initial: bool) -> bytes:
    """Return one normal or RFC 2231 extended parameter value.

    Returns:
        The validated parameter bytes without character-set decoding.

    Raises:
        AppError: If quoting or extension syntax conflicts with the value form.

    """
    if encoded:
        if value.startswith(b'"'):
            raise AppError(ExitCode.PARSE_ERROR, "quoted RFC 2231 parameter")
        return _extended_parameter(value, initial=initial)
    if not value.startswith(b'"') and not TOKEN_RE.fullmatch(value):
        raise AppError(ExitCode.PARSE_ERROR, "malformed MIME parameter value")
    return _unquote(value)


def _store_segment(
    segments: dict[bytes, dict[int | None, _Segment]],
    name: bytes,
    index: int | None,
    segment: _Segment,
) -> None:
    """Store one unique direct parameter or continuation segment.

    Raises:
        AppError: If it overlaps a direct value or prior continuation segment.

    """
    parts = segments.setdefault(name, {})
    if index is None:
        if parts:
            raise AppError(ExitCode.PARSE_ERROR, "duplicate MIME parameter")
    elif None in parts:
        raise AppError(ExitCode.PARSE_ERROR, "overlapping MIME parameter")
    elif index in parts:
        raise AppError(ExitCode.PARSE_ERROR, "duplicate MIME parameter segment")
    parts[index] = segment


def _ordered_segments(parts: dict[int | None, _Segment]) -> list[_Segment]:
    """Return a direct value or a contiguous RFC 2231 continuation in order.

    Returns:
        The segments in index order.

    Raises:
        AppError: If a continuation omits a required segment.

    """
    if None in parts:
        return [parts[None]]
    indexes = sorted(index for index in parts if index is not None)
    if indexes != list(range(len(indexes))):
        raise AppError(ExitCode.PARSE_ERROR, "gapped MIME parameter continuation")
    return [parts[index] for index in indexes]
