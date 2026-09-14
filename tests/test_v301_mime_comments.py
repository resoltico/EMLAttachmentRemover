"""v3.0.1 structured MIME comment acceptance and rejection contracts."""

from __future__ import annotations

import pytest

from eml_attachment_remover import mime_comments, mime_validation
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


def test_comment_scanner_preserves_exact_cfws_replacement_boundaries() -> None:
    """Leading, adjacent, and whitespace-adjacent comments retain exact spacing."""
    assert (
        mime_comments.without_comments(b"(comment); text", allowed=True) == b" ; text"
    )
    assert (
        mime_comments.without_comments(b"a (comment) text", allowed=True) == b"a  text"
    )
    assert (
        mime_comments.without_comments(b"a;(comment) text", allowed=True) == b"a;  text"
    )


def test_comment_scanner_rejects_nonfolding_control_and_advances_escapes() -> None:
    """Only CRLF FWS is legal, and escaped comment octets consume exactly two bytes."""
    assert mime_comments._escaped_comment_cursor(b"x\\q", 1) == 3  # ruff: ignore[private-member-access] - exact comment quoted-pair cursor.
    assert mime_comments._folded_comment_cursor(b"\r\n\t x", 0) == 4  # ruff: ignore[private-member-access] - exact CFWS cursor after horizontal white space.
    assert mime_comments.without_comments(b"(X) ", allowed=True) == b"  "
    for value in (b"\r ", b"\n ", b"\rX", b"\rX ", b"\r\nX"):
        with pytest.raises(AppError) as rejected:
            mime_comments._folded_comment_cursor(value, 0)  # ruff: ignore[private-member-access] - direct malformed FWS grammar.
        assert rejected.value == AppError(
            ExitCode.PARSE_ERROR, "malformed MIME comment"
        )


def test_quoted_and_folded_comment_scans_are_bounded_and_exact() -> None:
    """Quoted strings and trailing FWS cannot skip, stall, or consume extra bytes."""
    quoted = b'xx"a\\b"'
    result = bytearray()
    assert mime_comments._copy_quoted(quoted, 2, result) == len(quoted)  # ruff: ignore[private-member-access] - direct quoted-string cursor contract.
    assert bytes(result) == b'"a\\b"'
    assert mime_comments._folded_comment_cursor(b"\r\n \t", 0) == 4  # ruff: ignore[private-member-access] - terminal FWS is consumed exactly.
    with pytest.raises(AppError) as escaped:
        mime_comments._escaped_comment_cursor(b"\\", 0)  # ruff: ignore[private-member-access] - terminal quoted-pair fault.
    assert escaped.value == AppError(ExitCode.PARSE_ERROR, "unterminated MIME comment")
    with pytest.raises(AppError) as nonadvancing:
        mime_comments._advanced_quoted_cursor(2, 2)  # ruff: ignore[private-member-access] - quoted completion must advance.
    assert nonadvancing.value == AppError(
        ExitCode.PARSE_ERROR, "nonadvancing MIME quoted cursor"
    )


def test_comment_replacement_uses_the_actual_preceding_cfws_byte() -> None:
    """A comment adds CFWS only after a non-CFWS accumulated byte."""
    remove_comment = mime_comments._remove_comment  # ruff: ignore[private-member-access] - direct accumulated-byte predicate proof.
    no_extra_space = bytearray(b"a ")
    assert remove_comment(b"a (c) ", 2, no_extra_space, allowed=True) == 5
    assert no_extra_space == b"a "
    add_space = bytearray(b"aX")
    assert remove_comment(b"a (c) ", 2, add_space, allowed=True) == 5
    assert add_space == b"aX "
    assert mime_comments._folded_comment_cursor(b"\r\n X", 0) == 3  # ruff: ignore[private-member-access] - first non-FWS byte terminates the fold.


def test_comment_end_rejects_a_nonadvancing_parser_step(
    monkeypatch: pytest.MonkeyPatch,
) -> None:
    """The bounded scanner rejects a faulty cursor before it can loop forever."""

    def stalled(_value: bytes, position: int, depth: int) -> tuple[int, int]:
        return position, depth

    monkeypatch.setattr(mime_comments, "_comment_step", stalled)
    with pytest.raises(AppError) as rejected:
        mime_comments._comment_end(b"()", 0)  # ruff: ignore[private-member-access] - progress invariant belongs to the raw comment scanner.
    assert rejected.value == AppError(
        ExitCode.PARSE_ERROR, "nonadvancing MIME comment cursor"
    )
