"""Bounded raw-wire MIME indexing and retained-payload validation.

The stdlib parser is used as an independent structural check, while this module owns
the source byte spans.  It never regenerates body text or decodes character sets.
"""

from __future__ import annotations

import string
from dataclasses import dataclass
from typing import Final

from .domain import AppError, ExitCode
from .mime_headers import TOKEN_RE, Header
from .mime_identifiers import parse_message_identifier

BACKSLASH: Final = ord("\\")
DOUBLE_QUOTE: Final = ord('"')
SEMICOLON: Final = ord(";")
PERCENT: Final = ord("%")
MIN_QUOTED_BYTES: Final = 2


@dataclass(frozen=True, slots=True)
class ContentSpec:
    """A lower-cased media/disposition token plus raw parameter values."""

    token: str
    parameters: dict[bytes, bytes]


def _split_semicolons(value: bytes) -> list[bytes]:
    """Split a MIME structured field while respecting quoted strings.

    Returns:
        The semicolon-separated field components with outer white space removed.

    Raises:
        AppError: If a quoted string or quoted pair is unfinished.

    """
    pieces: list[bytes] = []
    current = bytearray()
    quoted = False
    escaped = False
    for byte in value:
        if escaped:
            current.append(byte)
            escaped = False
        elif quoted and byte == BACKSLASH:
            current.append(byte)
            escaped = True
        elif byte == DOUBLE_QUOTE:
            current.append(byte)
            quoted = not quoted
        elif byte == SEMICOLON and not quoted:
            pieces.append(bytes(current).strip())
            current.clear()
        else:
            current.append(byte)
    if quoted or escaped:
        raise AppError(ExitCode.PARSE_ERROR, "unterminated MIME quoted parameter")
    pieces.append(bytes(current).strip())
    return pieces


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
    escaped = False
    for byte in value[1:-1]:
        if escaped:
            result.append(byte)
            escaped = False
        elif byte == BACKSLASH:
            escaped = True
        else:
            result.append(byte)
    if escaped:
        raise AppError(ExitCode.PARSE_ERROR, "unterminated MIME quoted-pair")
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
    if not index_text.isdigit():
        raise AppError(ExitCode.PARSE_ERROR, "malformed MIME parameter extension")
    return base, int(index_text), encoded


def _extended_parameter(value: bytes, *, initial: bool) -> bytes:
    """Validate an RFC 2231 extension spelling without charset-decoding its bytes.

    Returns:
        The byte-exact extended parameter spelling.

    Raises:
        AppError: If percent escapes or allowed attribute characters are invalid.

    """
    payload = _extended_payload(value) if initial else value
    allowed = (
        b"!#$&+-.^_`|~0123456789ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz"
    )
    position = 0
    while position < len(payload):
        byte = payload[position]
        if byte == PERCENT:
            if position + 2 >= len(payload) or any(
                digit not in b"0123456789abcdefABCDEF"
                for digit in payload[position + 1 : position + 3]
            ):
                raise AppError(ExitCode.PARSE_ERROR, "malformed RFC 2231 escape")
            position += 3
        elif byte in allowed:
            position += 1
        else:
            raise AppError(ExitCode.PARSE_ERROR, "malformed RFC 2231 parameter")
    return value


def _extended_payload(value: bytes) -> bytes:
    """Return the RFC 2231 payload after a mandatory charset/language prefix.

    Returns:
        The percent-encoded payload after exactly two apostrophe delimiters.

    Raises:
        AppError: If the initial extended parameter has invalid prefix syntax.

    """
    charset, first_quote, remainder = value.partition(b"'")
    language, second_quote, payload = remainder.partition(b"'")
    language_bytes = (string.digits + string.ascii_letters + "-").encode("ascii")
    if not first_quote or not second_quote or not TOKEN_RE.fullmatch(charset):
        raise AppError(ExitCode.PARSE_ERROR, "malformed RFC 2231 extended parameter")
    if language and not all(byte in language_bytes for byte in language):
        raise AppError(ExitCode.PARSE_ERROR, "malformed RFC 2231 language")
    return payload


def _structured(value: bytes, *, media: bool) -> ContentSpec:
    """Parse a closed token-and-parameter MIME field.

    Returns:
        The normalized token and unique validated raw parameter values.

    """
    pieces = _split_semicolons(value)
    token = _structured_token(pieces[0], media=media)
    parameters = _structured_parameters(pieces[1:])
    return ContentSpec(_ascii_token(token), parameters)


def _structured_token(value: bytes, *, media: bool) -> bytes:
    """Validate the primary token in a structured MIME field.

    Returns:
        The lower-case primary token.

    Raises:
        AppError: If the token is malformed for a media or disposition field.

    """
    token = value.lower()
    if not token or not TOKEN_RE.fullmatch(token.replace(b"/", b"")):
        raise AppError(ExitCode.PARSE_ERROR, "malformed MIME structured header")
    if media and (
        token.count(b"/") != 1 or any(not side for side in token.split(b"/"))
    ):
        raise AppError(ExitCode.PARSE_ERROR, "malformed MIME media type")
    return token


def _structured_parameters(pieces: list[bytes]) -> dict[bytes, bytes]:
    """Parse and join unique ordinary or RFC 2231 continuation parameters.

    Returns:
        Unique base parameter names mapped to their raw normalized values.

    """
    parameters: dict[bytes, bytes] = {}
    continuations: dict[bytes, dict[int, bytes]] = {}
    for piece in pieces:
        name, segment, value = _parameter_piece(piece)
        _store_parameter(parameters, continuations, name, segment, value)
    _finish_continuations(parameters, continuations)
    return parameters


def _parameter_piece(piece: bytes) -> tuple[bytes, int | None, bytes]:
    """Parse one parameter component before duplicate/continuation ownership checks.

    Returns:
        Base name, optional continuation index, and validated raw value.

    Raises:
        AppError: If a parameter component does not use the supported grammar.

    """
    name, equals, raw_value = piece.partition(b"=")
    if not equals:
        raise AppError(ExitCode.PARSE_ERROR, "malformed MIME parameter")
    base_name, segment, encoded = _parameter_name(name.strip().lower())
    value = raw_value.strip()
    return (
        base_name,
        segment,
        _parameter_value(value, encoded=encoded, initial=segment in {None, 0}),
    )


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


def _store_parameter(
    parameters: dict[bytes, bytes],
    continuations: dict[bytes, dict[int, bytes]],
    name: bytes,
    segment: int | None,
    value: bytes,
) -> None:
    """Store one unique direct parameter or continuation segment.

    Raises:
        AppError: If it overlaps a direct value or prior continuation segment.

    """
    if segment is None:
        if name in parameters or name in continuations:
            raise AppError(ExitCode.PARSE_ERROR, "duplicate MIME parameter")
        parameters[name] = value
        return
    if name in parameters:
        raise AppError(ExitCode.PARSE_ERROR, "overlapping MIME parameter")
    entries = continuations.setdefault(name, {})
    if segment in entries:
        raise AppError(ExitCode.PARSE_ERROR, "duplicate MIME parameter segment")
    entries[segment] = value


def _finish_continuations(
    parameters: dict[bytes, bytes], continuations: dict[bytes, dict[int, bytes]]
) -> None:
    """Join each contiguous RFC 2231 continuation into its base parameter.

    Raises:
        AppError: If a continuation omits a required segment.

    """
    for name, segments in continuations.items():
        expected = list(range(len(segments)))
        if sorted(segments) != expected:
            raise AppError(ExitCode.PARSE_ERROR, "gapped MIME parameter continuation")
        parameters[name] = b"".join(segments[index] for index in expected)


def _ascii_token(token: bytes) -> str:
    """Decode one already syntax-checked ASCII MIME token.

    Returns:
        The normalized ASCII token as text.

    Raises:
        AppError: If the raw token nonetheless contains non-ASCII octets.

    """
    try:
        return token.decode("ascii")
    except UnicodeDecodeError as exc:
        raise AppError(ExitCode.PARSE_ERROR, "non-ASCII MIME token") from exc


def _header_value(headers: tuple[Header, ...], name: bytes) -> bytes | None:
    """Return a singleton field's unfolded semantic bytes.

    Returns:
        The unfolded singleton value, or ``None`` if no field exists.

    """
    for header in headers:
        if header.name == name:
            return b" ".join(part.strip() for part in header.value.splitlines())
    return None


def content_specs(
    headers: tuple[Header, ...],
) -> tuple[ContentSpec, ContentSpec | None, str]:
    """Extract closed Content-Type, disposition, and CTE values.

    Returns:
        Normalized content type, optional disposition, and normalized CTE token.

    Raises:
        AppError: If an outer MIME control cannot be parsed safely.

    """
    raw_type = _header_value(headers, b"content-type")
    content_type = (
        _structured(raw_type, media=True) if raw_type else ContentSpec("text/plain", {})
    )
    raw_disposition = _header_value(headers, b"content-disposition")
    disposition = _structured(raw_disposition, media=False) if raw_disposition else None
    raw_cte = _header_value(headers, b"content-transfer-encoding")
    _validate_content_id(_header_value(headers, b"content-id"))
    if raw_cte is None:
        return content_type, disposition, "7bit"
    cte = raw_cte.strip().lower()
    if not TOKEN_RE.fullmatch(cte):
        raise AppError(ExitCode.PARSE_ERROR, "malformed Content-Transfer-Encoding")
    return content_type, disposition, cte.decode("ascii")


def _validate_content_id(value: bytes | None) -> None:
    """Reject a malformed structural identifier before policy can rely on it."""
    if value is None:
        return
    parse_message_identifier(value, field="Content-ID")
