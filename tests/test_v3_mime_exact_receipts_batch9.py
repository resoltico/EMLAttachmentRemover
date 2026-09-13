"""Narrow raw-header and retained-payload behavior receipts."""

from __future__ import annotations

import pytest

from eml_attachment_remover import mime_encoding, mime_headers, mime_raw
from eml_attachment_remover.domain import AppError, ExitCode


def test_first_header_line_requires_a_token_before_its_first_colon() -> None:
    """The first colon decides header syntax; later colons belong to the value."""
    assert mime_raw._first_line_is_header_like(  # ruff: ignore[private-member-access] - first-colon receipt.
        b"X-Test: one: two\r\n", 0, 19
    )
    assert not mime_raw._first_line_is_header_like(  # ruff: ignore[private-member-access] - token-before-colon receipt.
        b"not a token: value\r\n", 0, 20
    )


def test_header_and_quoted_printable_reject_exact_malformed_forms() -> None:
    """A bare header continuation and malformed retained escape retain exact errors."""
    with pytest.raises(AppError) as continuation:
        mime_headers.parse_headers(b" orphan\r\n", 0, 8)
    assert continuation.value == AppError(
        ExitCode.PARSE_ERROR, "orphaned MIME header continuation"
    )
    with pytest.raises(AppError) as encoded:
        mime_encoding.decode_payload(b"before=Q0after", "quoted-printable")
    assert encoded.value == AppError(
        ExitCode.PARSE_ERROR, "invalid retained quoted-printable payload"
    )
