"""Exact RFC 2231 alphabet and quoted-parameter state receipts."""

from __future__ import annotations

import pytest

from eml_attachment_remover import mime_validation
from eml_attachment_remover.domain import AppError, ExitCode


@pytest.mark.parametrize("language", [b"0", b"9", b"A", b"Z", b"a", b"z", b"-"])
def test_extended_payload_accepts_every_rfc2231_language_alphabet_boundary(
    language: bytes,
) -> None:
    """Every endpoint of the three alphanumeric ranges and hyphen is permitted."""
    value = b"utf-8'" + language + b"'payload"
    assert (
        mime_validation._extended_payload(  # ruff: ignore[private-member-access] - RFC 2231 language boundary.
            value
        )
        == b"payload"
    )


@pytest.mark.parametrize("language", [b":", b"@", b"[", b"`", b"{"])
def test_extended_payload_rejects_each_gap_outside_the_language_alphabet(
    language: bytes,
) -> None:
    """Adjacent ASCII punctuation never becomes a valid RFC 2231 language byte."""
    with pytest.raises(AppError) as raised:
        mime_validation._extended_payload(  # ruff: ignore[private-member-access] - RFC 2231 language gap.
            b"utf-8'" + language + b"'payload"
        )
    assert raised.value == AppError(ExitCode.PARSE_ERROR, "malformed RFC 2231 language")


def test_semicolon_splitter_tracks_quote_and_escape_positions_independently() -> None:
    """A quoted pair preserves its following quote and protects internal semicolons."""
    assert mime_validation._split_semicolons(  # ruff: ignore[private-member-access] - independent quote/escape positions.
        b'first; name="one\\\\;two"; last'
    ) == [b"first", b'name="one\\\\;two"', b"last"]
    unfinished = b'first; name="unfinished' + bytes((92,))
    for invalid in (b'first; name="unfinished', unfinished):
        with pytest.raises(AppError) as raised:
            mime_validation._split_semicolons(  # ruff: ignore[private-member-access] - terminal parser-state receipt.
                invalid
            )
        assert raised.value == AppError(
            ExitCode.PARSE_ERROR, "unterminated MIME quoted parameter"
        )
