"""v3.0.1 structured MIME comment acceptance and rejection contracts."""

from __future__ import annotations

import pytest

from eml_attachment_remover import mime_validation
from eml_attachment_remover.domain import AppError, ExitCode
from eml_attachment_remover.mime_headers import Header
from eml_attachment_remover.mime_validation import ContentSpec


@pytest.mark.parametrize(
    ("header", "expected"),
    [
        (
            Header(b"content-type", b"text/plain; charset=us-ascii (Plain text)", 0, 1),
            (ContentSpec("text/plain", {b"charset": b"us-ascii"}), None, "7bit"),
        ),
        (
            Header(
                b"content-type", b'text/plain; charset="us-ascii" (Plain text)', 0, 1
            ),
            (ContentSpec("text/plain", {b"charset": b"us-ascii"}), None, "7bit"),
        ),
        (
            Header(b"content-transfer-encoding", b"7bit (ASCII)", 0, 1),
            (ContentSpec("text/plain", {}), None, "7bit"),
        ),
    ],
)
def test_content_type_and_cte_accept_legal_trailing_comments(
    header: Header, expected: tuple[ContentSpec, ContentSpec | None, str]
) -> None:
    """RFC 2045 comments have no semantic effect in their permitted fields."""
    assert mime_validation.content_specs((header,)) == expected


def test_comments_preserve_quote_meaning_and_reject_ambiguous_token_splices() -> None:
    """Comments cannot turn two token fragments into an attachment disposition."""
    assert mime_validation._split_semicolons(  # ruff: ignore[private-member-access] - quotes retain literal parenthesis and semicolon.
        b'text/plain; note="(literal; value)" (outer (nested\\) note)); charset=utf-8',
        comments_allowed=True,
    ) == [b"text/plain", b'note="(literal; value)"', b"charset=utf-8"]
    for value in (b"attach(comment)ment", b"text/plain; charset=us(comment)ascii"):
        with pytest.raises(AppError) as rejected:
            mime_validation._split_semicolons(  # ruff: ignore[private-member-access] - comments require a CFWS boundary.
                value, comments_allowed=True
            )
        assert rejected.value == AppError(
            ExitCode.PARSE_ERROR, "comment occurs inside MIME token"
        )


def test_content_disposition_does_not_gain_comment_grammar() -> None:
    """RFC 2183 disposition controls remain deliberately stricter than RFC 2045."""
    with pytest.raises(AppError) as rejected:
        mime_validation.content_specs((
            Header(b"content-disposition", b"attachment (not permitted)", 0, 1),
        ))
    assert rejected.value == AppError(
        ExitCode.PARSE_ERROR, "MIME comments are not permitted"
    )


@pytest.mark.parametrize(
    ("value", "message"),
    [
        (b")", "malformed MIME comment"),
        (b"attachment (comment)evil", "comment occurs inside MIME token"),
        (b"attachment (open", "unterminated MIME comment"),
        (
            b"attachment (" + b"(" * 8 + b"nested" + b")" * 9,
            "MIME comment nesting exceeds limit",
        ),
        (b"attachment (bad\x01)", "malformed MIME comment"),
        (b"attachment (escaped" + bytes((92,)), "unterminated MIME comment"),
        (b"attachment (bare\rline)", "malformed MIME comment"),
    ],
)
def test_comment_grammar_rejects_each_bounded_malformed_state(
    value: bytes, message: str
) -> None:
    """Every comment state is finite and cannot be repaired into a token."""
    with pytest.raises(AppError) as rejected:
        mime_validation._split_semicolons(  # ruff: ignore[private-member-access] - explicit comment grammar states.
            value, comments_allowed=True
        )
    assert rejected.value == AppError(ExitCode.PARSE_ERROR, message)


def test_comments_accept_structured_delimiter_and_folding_boundaries() -> None:
    """Comments may surround a delimiter and use only actual folded white space."""
    assert mime_validation._split_semicolons(  # ruff: ignore[private-member-access] - semicolon and CRLF are semantic boundaries.
        b"text/plain;(comment\r\n folded) charset=utf-8", comments_allowed=True
    ) == [b"text/plain", b"charset=utf-8"]
