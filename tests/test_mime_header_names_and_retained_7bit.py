"""Narrow MIME header and retained-payload receipts."""

from __future__ import annotations

import pytest

from eml_attachment_remover import mime_encoding, mime_headers
from eml_attachment_remover.domain import AppError, ExitCode


def test_header_name_requires_rfc_token_octets() -> None:
    """Header field names cannot contain whitespace or an empty token."""
    assert mime_headers.is_header_name(b"X-Trace_1")
    assert not mime_headers.is_header_name(b"")
    assert not mime_headers.is_header_name(b"X Trace")


def test_retained_7bit_payload_rejects_the_first_8bit_octet() -> None:
    """7bit source bytes remain ASCII-only before any downstream processing."""
    with pytest.raises(AppError) as rejected:
        mime_encoding.decode_payload(b"plain\x80tail", "7bit")
    assert rejected.value == AppError(
        ExitCode.PARSE_ERROR, "7bit payload contains an 8bit octet"
    )
