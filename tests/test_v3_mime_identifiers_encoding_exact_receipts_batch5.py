"""Exact identifier and quoted-printable receipts for mutation boundaries."""

from __future__ import annotations

import pytest

from eml_attachment_remover import mime_encoding, mime_identifiers
from eml_attachment_remover.domain import AppError, ExitCode


def test_identifier_keeps_x_octets_and_exactly_reports_a_missing_close() -> None:
    """Identifier validation permits X octets and binds caller field context."""
    assert (
        mime_identifiers.parse_message_identifier(b"<X@x>", field="Content-ID")
        == b"X@x"
    )
    with pytest.raises(AppError) as missing_close:
        mime_identifiers.parse_message_identifier(b"<open", field="Content-ID")
    assert missing_close.value == AppError(ExitCode.PARSE_ERROR, "malformed Content-ID")


def test_comment_scanner_honors_initial_escape_empty_comment_and_literal_x() -> None:
    """Nested-comment cursors treat an escaped close, an empty close, and X exactly."""
    assert (
        mime_identifiers._skip_comment(  # ruff: ignore[private-member-access] - escaped closing-parenthesis cursor receipt.
            b"(\\))", 0
        )
        == 4
    )
    assert (
        mime_identifiers._skip_comment(  # ruff: ignore[private-member-access] - empty comment cursor receipt.
            b"()", 0
        )
        == 2
    )
    assert (
        mime_identifiers._skip_comment(  # ruff: ignore[private-member-access] - ordinary comment octet receipt.
            b"(X)", 0
        )
        == 3
    )


def test_identifier_sequence_reports_trailing_and_empty_sequence_errors_exactly() -> (
    None
):
    """Sequence grammar rejects non-CFWS tails and a CFWS-only wire value."""
    with pytest.raises(AppError) as trailing:
        mime_identifiers.parse_message_identifier_sequence(
            b"<one@x>tail", field="related start-info"
        )
    assert trailing.value == AppError(
        ExitCode.PARSE_ERROR, "malformed related start-info"
    )
    with pytest.raises(AppError) as empty:
        mime_identifiers.parse_message_identifier_sequence(
            b" \t\r\n", field="related start-info"
        )
    assert empty.value == AppError(ExitCode.PARSE_ERROR, "malformed related start-info")


def test_identifier_sequence_preserves_all_ids_across_comments_and_folding() -> None:
    """Complete CFWS-wrapped sequences preserve source order and exact inner octets."""
    assert mime_identifiers.parse_message_identifier_sequence(
        b" (lead) <first@x>\r\n\t<second@x> (tail) ",
        field="related start-info",
    ) == (b"first@x", b"second@x")


def test_quoted_printable_requires_scanning_after_soft_breaks_and_hex_escapes() -> None:
    """One legal escape never ends validation before a following invalid equals sign."""
    for encoded in (b"=\n=G0", b"=3D=G0"):
        with pytest.raises(AppError) as rejected:
            mime_encoding.decode_payload(encoded, "quoted-printable")
        assert rejected.value == AppError(
            ExitCode.PARSE_ERROR, "invalid retained quoted-printable payload"
        )


def test_quoted_printable_accepts_both_hex_cases_but_not_x_as_a_digit() -> None:
    """Hex pairs are case-insensitive, while X remains outside the hexadecimal set."""
    assert mime_encoding.decode_payload(b"=aF=F0", "quoted-printable") == b"\xaf\xf0"
    for encoded in (b"=X0", b"=0X"):
        with pytest.raises(AppError) as rejected:
            mime_encoding.decode_payload(encoded, "quoted-printable")
        assert rejected.value == AppError(
            ExitCode.PARSE_ERROR, "invalid retained quoted-printable payload"
        )
