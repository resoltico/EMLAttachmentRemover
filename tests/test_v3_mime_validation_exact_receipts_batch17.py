"""Exact MIME-validation receipts for residual mutation boundaries."""

from __future__ import annotations

import pytest

from eml_attachment_remover import mime_validation
from eml_attachment_remover.domain import AppError, ExitCode


def test_parameter_parse_failures_keep_their_exact_public_diagnostics() -> None:
    """Distinct malformed parameter forms retain their observable classifications."""
    cases = (
        (
            lambda: mime_validation._parameter_name(b"bad space"),  # ruff: ignore[private-member-access] - exact parameter-name diagnostic.
            "malformed MIME parameter name",
        ),
        (
            lambda: mime_validation._parameter_piece(b"filename"),  # ruff: ignore[private-member-access] - exact parameter-piece diagnostic.
            "malformed MIME parameter",
        ),
        (
            lambda: mime_validation._parameter_value(  # ruff: ignore[private-member-access] - exact ordinary-value diagnostic.
                b"not a token", encoded=False, initial=False
            ),
            "malformed MIME parameter value",
        ),
        (
            lambda: mime_validation._store_parameter(  # ruff: ignore[private-member-access] - exact ownership diagnostic.
                {b"name": b"one"}, {}, b"name", None, b"two"
            ),
            "duplicate MIME parameter",
        ),
    )
    for parse, message in cases:
        with pytest.raises(AppError) as raised:
            parse()
        assert raised.value == AppError(ExitCode.PARSE_ERROR, message)


def test_encoded_parameter_piece_cannot_bypass_rfc2231_escape_validation() -> None:
    """The encoded bit must route a token-shaped bad escape to RFC 2231 checks."""
    with pytest.raises(AppError) as raised:
        mime_validation._parameter_piece(  # ruff: ignore[private-member-access] - encoded-name routing receipt.
            b"filename*=utf-8''bad%XZ"
        )
    assert raised.value == AppError(ExitCode.PARSE_ERROR, "malformed RFC 2231 escape")


def test_structured_token_ignores_media_separators_but_requires_token_octets() -> None:
    """Generic token validation retains its media-separator semantics without repair."""
    structured_token = mime_validation._structured_token  # ruff: ignore[private-member-access] - exact separator grammar receipt.
    assert structured_token(b"/Type//Subtype/") == b"/type//subtype/"
    with pytest.raises(AppError) as raised:
        structured_token(b"///")
    assert raised.value == AppError(
        ExitCode.PARSE_ERROR, "malformed MIME structured header"
    )


def test_semicolon_scanner_honors_a_delimiter_at_the_first_wire_octet() -> None:
    """An initial separator is an empty first field, not escaped ordinary data."""
    assert mime_validation._split_semicolons(  # ruff: ignore[private-member-access] - initial-delimiter scanner receipt.
        b"; tail"
    ) == [b"", b"tail"]
