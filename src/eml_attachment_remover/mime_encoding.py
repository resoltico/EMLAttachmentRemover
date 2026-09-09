"""Strict transfer-encoding validation for retained source payloads."""

from __future__ import annotations

import base64
import binascii
import hashlib
import quopri
from typing import TYPE_CHECKING, Final

from .domain import AppError, ExitCode, RetainedFingerprint

if TYPE_CHECKING:
    from .mime_raw import RawNode

MAX_RETAINED_DECODED: Final = 96 * 1024 * 1024
MAX_7BIT_OCTET: Final = 0x7F
EQUALS: Final = ord("=")
HEX_PAIR_BYTES: Final = 2


def payload(raw: bytes, node: RawNode) -> bytes:
    """Return a leaf's exact source-encoded body bytes.

    Returns:
        The body span before a following multipart delimiter, if one exists.

    """
    return raw[node.body_start : node.payload_end or node.end]


def decode_payload(encoded: bytes, cte: str) -> bytes:
    """Strictly validate one retained CTE and return its decoded octets.

    Returns:
        The decoded octets without charset decoding or content regeneration.

    Raises:
        AppError: If the CTE or retained encoded payload is invalid.

    """
    if cte in {"", "7bit"}:
        if any(byte > MAX_7BIT_OCTET for byte in encoded):
            raise AppError(ExitCode.PARSE_ERROR, "7bit payload contains an 8bit octet")
        return encoded
    if cte in {"8bit", "binary"}:
        return encoded
    if cte == "base64":
        compact = encoded.translate(None, b" \t\r\n")
        try:
            return base64.b64decode(compact, validate=True)
        except (binascii.Error, ValueError) as exc:
            raise AppError(
                ExitCode.PARSE_ERROR, "invalid retained base64 payload"
            ) from exc
    if cte == "quoted-printable":
        _validate_quoted_printable(encoded)
        return quopri.decodestring(encoded)
    raise AppError(ExitCode.PARSE_ERROR, f"unsupported Content-Transfer-Encoding {cte}")


def _validate_quoted_printable(encoded: bytes) -> None:
    """Require each equals sign to be a hex pair or a soft line break.

    Raises:
        AppError: If any equals sign is not a valid escape or soft break.

    """
    index = 0
    while index < len(encoded):
        if encoded[index] != EQUALS:
            index += 1
            continue
        remainder = encoded[index + 1 :]
        if remainder.startswith(b"\r\n"):
            index += 3
        elif remainder.startswith(b"\n"):
            index += 2
        elif len(remainder) >= HEX_PAIR_BYTES and all(
            byte in b"0123456789abcdefABCDEF" for byte in remainder[:HEX_PAIR_BYTES]
        ):
            index += 3
        else:
            raise AppError(
                ExitCode.PARSE_ERROR, "invalid retained quoted-printable payload"
            )


def fingerprint_retained(
    raw: bytes, nodes: list[RawNode]
) -> tuple[RetainedFingerprint, ...]:
    """Compute source-bound fingerprints for every retained leaf in order.

    Returns:
        Ordered encoded and decoded digests for retained leaf payloads only.

    Raises:
        AppError: If CTE validation or the retained decoded-byte budget fails.

    """
    total = 0
    result: list[RetainedFingerprint] = []
    for node in nodes:
        if node.children:
            continue
        encoded = payload(raw, node)
        decoded = decode_payload(encoded, node.cte)
        total += len(decoded)
        if total > MAX_RETAINED_DECODED:
            raise AppError(
                ExitCode.PARSE_ERROR, "retained decoded payload limit exceeded"
            )
        result.append(
            RetainedFingerprint(
                node.path,
                node.media_type,
                node.cte,
                tuple(sorted(node.content_type.parameters.items())),
                hashlib.sha256(encoded).hexdigest(),
                hashlib.sha256(decoded).hexdigest(),
            )
        )
    return tuple(result)
