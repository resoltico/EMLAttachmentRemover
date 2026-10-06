"""Narrow exact receipts for remaining MIME parser decisions."""

from __future__ import annotations

import pytest

from eml_attachment_remover import mime_encoding, mime_headers
from eml_attachment_remover.domain import AppError, ExitCode


def test_header_newline_variants_have_exact_wire_end_offsets() -> None:
    """CRLF, LF, CR, and final partial lines each report their exclusive offset."""
    assert mime_headers.line_end(b"x\r\ny", 0, 4) == 3
    assert mime_headers.line_end(b"x\ny", 0, 3) == 2
    assert mime_headers.line_end(b"x\ry", 0, 3) == 2
    assert mime_headers.line_end(b"xyz", 0, 3) == 3


def test_unsupported_transfer_encoding_keeps_the_normalized_token_in_error() -> None:
    """Only declared retained CTEs decode; all other normalized tokens are visible."""
    with pytest.raises(AppError) as rejected:
        mime_encoding.decode_payload(b"body", "x-unknown")
    assert rejected.value == AppError(
        ExitCode.PARSE_ERROR, "unsupported Content-Transfer-Encoding x-unknown"
    )
