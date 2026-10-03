"""Bounded raw-wire MIME indexing and retained-payload validation.

The stdlib parser is used as an independent structural check, while this module owns
the source byte spans.  It never regenerates body text or decodes character sets.
"""

from __future__ import annotations

from dataclasses import dataclass, field
from typing import TYPE_CHECKING, Final

from .domain import AppError, ExitCode
from .mime_comments import without_comments
from .mime_headers import TOKEN_RE, Header
from .mime_identifiers import parse_message_identifier
from .mime_parameters import BACKSLASH, DOUBLE_QUOTE, structured_parameters
from .mime_quote_state import (
    ESCAPE_CLEAR,
    ESCAPE_PENDING,
    QUOTE_CLOSED,
    QUOTE_OPEN,
    validated_escape_state,
)

if TYPE_CHECKING:
    from collections.abc import Callable

SEMICOLON: Final = ord(";")
SLASH: Final = ord("/")


@dataclass(frozen=True, slots=True)
class ContentSpec:
    """A lower-cased media/disposition token plus raw parameter values.

    ``parameters`` keeps each value's exact wire spelling for evidence. ``decoded``
    holds the RFC 2231 meaning of every parameter that used an extended segment,
    for structural controls such as a multipart boundary.
    """

    token: str
    parameters: dict[bytes, bytes]
    decoded: dict[bytes, bytes] = field(default_factory=dict)


def _split_semicolons(value: bytes, *, comments_allowed: bool) -> list[bytes]:
    """Split a MIME structured field while respecting quoted strings.

    Returns:
        The semicolon-separated field components with outer white space removed.

    """
    pieces: list[bytes] = []
    value = without_comments(value, allowed=comments_allowed)
    current = bytearray()
    quoted = QUOTE_CLOSED
    escaped = ESCAPE_CLEAR
    for byte in value:
        escaped = validated_escape_state(escaped)
        if quoted == QUOTE_OPEN and escaped == ESCAPE_PENDING:
            escaped = ESCAPE_CLEAR
            current.append(byte)
        elif quoted == QUOTE_OPEN and byte == BACKSLASH:
            escaped = ESCAPE_PENDING
            current.append(byte)
        elif byte == DOUBLE_QUOTE:
            current.append(byte)
            quoted = QUOTE_OPEN if quoted == QUOTE_CLOSED else QUOTE_CLOSED
        elif byte == SEMICOLON and quoted == QUOTE_CLOSED:
            pieces.append(bytes(current).strip())
            current.clear()
        else:
            current.append(byte)
    pieces.append(bytes(current).strip())
    return pieces


def _structured(
    value: bytes,
    token_validator: Callable[[bytes], bytes],
    *,
    comments_allowed: bool,
) -> ContentSpec:
    """Parse a closed token-and-parameter MIME field.

    Returns:
        The normalized token and unique validated raw parameter values.

    """
    pieces = _split_semicolons(value, comments_allowed=comments_allowed)
    token = token_validator(pieces[0])
    parameters, decoded = structured_parameters(pieces[1:])
    return ContentSpec(_ascii_token(token), parameters, decoded)


def _field_spec(
    value: bytes,
    validator: Callable[[bytes], bytes],
    field_name: str,
    *,
    comments_allowed: bool,
) -> ContentSpec:
    """Attach a field name to a structured-value rejection.

    Returns:
        The admitted structured MIME value.

    Raises:
        AppError: If structured syntax is malformed.

    """
    try:
        return _structured(value, validator, comments_allowed=comments_allowed)
    except AppError as error:
        raise AppError(error.code, f"{field_name}: {error.message}") from error


def _structured_token(value: bytes) -> bytes:
    """Validate the primary token in a structured MIME field.

    Returns:
        The lower-case primary token.

    Raises:
        AppError: If the token is malformed for a MIME structured field.

    """
    token = value.lower()
    token_without_slashes = bytearray()
    for byte in token:
        if byte != SLASH:
            token_without_slashes.append(byte)
    if not token_without_slashes or not TOKEN_RE.fullmatch(token_without_slashes):
        raise AppError(ExitCode.PARSE_ERROR, "malformed MIME structured header")
    return token


def _media_token(value: bytes) -> bytes:
    """Validate the primary token for a Content-Type field.

    Returns:
        The lower-case type/subtype token.

    Raises:
        AppError: If the token does not have exactly one nonempty slash separator.

    """
    token = _structured_token(value)
    if token.count(b"/") != 1 or any(not side for side in token.split(b"/")):
        raise AppError(ExitCode.PARSE_ERROR, "malformed MIME media type")
    return token


def _ascii_token(token: bytes) -> str:
    """Decode one already syntax-checked ASCII MIME token.

    Returns:
        The normalized ASCII token as text.

    Raises:
        AppError: If the raw token nonetheless contains non-ASCII octets.

    """
    if not token.isascii():
        raise AppError(ExitCode.PARSE_ERROR, "non-ASCII MIME token")
    return token.decode()


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
        _field_spec(raw_type, _media_token, "Content-Type", comments_allowed=True)
        if raw_type
        else ContentSpec("text/plain", {})
    )
    raw_disposition = _header_value(headers, b"content-disposition")
    disposition = (
        _field_spec(
            raw_disposition,
            _structured_token,
            "Content-Disposition",
            comments_allowed=False,
        )
        if raw_disposition
        else None
    )
    raw_cte = _header_value(headers, b"content-transfer-encoding")
    _validate_content_id(_header_value(headers, b"content-id"))
    if raw_cte is None:
        return content_type, disposition, "7bit"
    cte = without_comments(raw_cte, allowed=True).strip().lower()
    if not TOKEN_RE.fullmatch(cte):
        raise AppError(ExitCode.PARSE_ERROR, "malformed Content-Transfer-Encoding")
    return content_type, disposition, cte.decode()


def _validate_content_id(value: bytes | None) -> None:
    """Reject a malformed structural identifier before policy can rely on it."""
    if value is None:
        return
    parse_message_identifier(value, field="Content-ID")
