"""Exact MIME-validation receipts for live mutation-survivor boundaries."""

from __future__ import annotations

import pytest

from eml_attachment_remover import mime_validation
from eml_attachment_remover.domain import AppError, ExitCode
from eml_attachment_remover.mime_headers import Header
from eml_attachment_remover.mime_validation import ContentSpec


def test_ascii_token_preserves_the_exact_non_ascii_error_receipt() -> None:
    """A byte token that cannot decode as ASCII has one public parse failure."""
    with pytest.raises(AppError) as rejected:
        mime_validation._ascii_token(  # ruff: ignore[private-member-access] - exact ASCII decoder receipt.
            b"\xff"
        )
    assert rejected.value == AppError(ExitCode.PARSE_ERROR, "non-ASCII MIME token")
    assert rejected.value.__cause__ is None


def test_rfc2231_escape_case_allowed_octets_and_failures_are_exact() -> None:
    """Both hexadecimal cases and all allowed attribute characters remain raw facts."""
    value = b"utf-8'en-US'AZaz09!#$&+-.^_`|~%aF%F0"
    assert (
        mime_validation._extended_parameter(  # ruff: ignore[private-member-access] - upper/lower hex and allowed-byte receipt.
            value, initial=True
        )
        == value
    )
    invalid = (
        (b"utf-8''bad%G0", "malformed RFC 2231 escape"),
        (b"utf-8''contains space", "malformed RFC 2231 parameter"),
        (b"utf-8'bad space'body", "malformed RFC 2231 language"),
        (b"utf 8''body", "malformed RFC 2231 extended parameter"),
    )
    for wire_value, message in invalid:
        with pytest.raises(AppError) as rejected:
            mime_validation._extended_parameter(  # ruff: ignore[private-member-access] - exact RFC 2231 grammar receipt.
                wire_value, initial=True
            )
        assert rejected.value == AppError(ExitCode.PARSE_ERROR, message)


def test_rfc2231_payload_uses_the_first_two_apostrophes_as_its_prefix_boundary() -> (
    None
):
    """A third apostrophe is payload syntax, not a second language delimiter."""
    with pytest.raises(AppError) as rejected:
        mime_validation._extended_parameter(  # ruff: ignore[private-member-access] - first-apostrophe partition receipt.
            b"utf-8'en'payload'not-language", initial=True
        )
    assert rejected.value == AppError(
        ExitCode.PARSE_ERROR, "malformed RFC 2231 parameter"
    )


def test_semicolon_scanner_preserves_escaped_quote_termination() -> None:
    """Only unquoted semicolons split fields; terminal quote states are closed."""
    assert mime_validation._split_semicolons(  # ruff: ignore[private-member-access] - exact structured-field scanner receipt.
        b'attachment; filename="semi;quote\\"kept"; size=7'
    ) == [b"attachment", b'filename="semi;quote\\"kept"', b"size=7"]
    for wire_value in (b'filename="unterminated', b'filename="unfinished\\'):
        with pytest.raises(AppError) as rejected:
            mime_validation._split_semicolons(  # ruff: ignore[private-member-access] - exact unterminated scanner receipt.
                wire_value
            )
        assert rejected.value == AppError(
            ExitCode.PARSE_ERROR, "unterminated MIME quoted parameter"
        )


def test_unquote_decodes_one_escaped_octet_and_preserves_exact_failures() -> None:
    """Quoted-pairs lose one slash, and malformed terminal syntax stays visible."""
    assert (
        mime_validation._unquote(  # ruff: ignore[private-member-access] - exact quoted-pair decoding receipt.
            b'"a\\;b\\"c"'
        )
        == b'a;b"c'
    )
    for wire_value, message in (
        (b'"unterminated', "malformed MIME quoted parameter"),
        (b'"trailing\\"', "unterminated MIME quoted-pair"),
    ):
        with pytest.raises(AppError) as rejected:
            mime_validation._unquote(  # ruff: ignore[private-member-access] - exact quoted-value failure receipt.
                wire_value
            )
        assert rejected.value == AppError(ExitCode.PARSE_ERROR, message)


def test_content_type_disposition_and_cte_grammar_have_separate_receipts() -> None:
    """Media slash grammar, disposition parameters, and CTE tokens are independent."""
    valid_headers = (
        Header(b"content-type", b"Text/Plain; charset=utf-8", 0, 39),
        Header(
            b"content-disposition",
            b'Attachment; filename="semi;colon.eml"',
            39,
            91,
        ),
        Header(b"content-transfer-encoding", b"BASE64", 91, 125),
    )
    assert mime_validation.content_specs(valid_headers) == (
        ContentSpec("text/plain", {b"charset": b"utf-8"}),
        ContentSpec("attachment", {b"filename": b"semi;colon.eml"}),
        "base64",
    )
    invalid = (
        (
            (Header(b"content-type", b"text/", 0, 22),),
            "malformed MIME media type",
        ),
        (
            (Header(b"content-disposition", b"attachment bad", 0, 34),),
            "malformed MIME structured header",
        ),
        (
            (Header(b"content-transfer-encoding", b"base 64", 0, 34),),
            "malformed Content-Transfer-Encoding",
        ),
    )
    for headers, message in invalid:
        with pytest.raises(AppError) as rejected:
            mime_validation.content_specs(headers)
        assert rejected.value == AppError(ExitCode.PARSE_ERROR, message)
