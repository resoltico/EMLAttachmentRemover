"""Additional exact MIME parsing and transfer-decoding receipts."""

from __future__ import annotations

import pytest

from eml_attachment_remover import mime_encoding, mime_validation
from eml_attachment_remover.domain import AppError, ExitCode


def test_structured_media_token_requires_one_nonempty_slash_pair() -> None:
    """Media syntax needs one slash with a token on each side."""
    assert (
        mime_validation._media_token(  # ruff: ignore[private-member-access] - normalized media token receipt.
            b"APPLICATION/JSON"
        )
        == b"application/json"
    )
    for value in (b"text/", b"text/plain/extra"):
        with pytest.raises(AppError) as rejected:
            mime_validation._media_token(  # ruff: ignore[private-member-access] - malformed media-token receipt.
                value
            )
        assert rejected.value == AppError(
            ExitCode.PARSE_ERROR, "malformed MIME media type"
        )


def test_quoted_printable_soft_breaks_preserve_following_payload() -> None:
    """Both CRLF and LF soft breaks join source fragments without byte regeneration."""
    assert (
        mime_encoding.decode_payload(b"one=\r\ntwo=\nthree", "quoted-printable")
        == b"onetwothree"
    )
