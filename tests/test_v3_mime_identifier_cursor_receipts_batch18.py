"""Cursor-safety receipts for CFWS-wrapped message identifiers."""

from __future__ import annotations

import pytest

from eml_attachment_remover import mime_identifiers
from eml_attachment_remover.domain import AppError, ExitCode


def test_cfws_cursor_advances_across_each_comment_before_the_identifier() -> None:
    """Nested CFWS components each advance one bounded cursor to the identifier."""
    value = b" \t(first)(second (nested))\r\n\t<root@x>"

    assert mime_identifiers.first_non_cfws(b"") == 0
    assert mime_identifiers.first_non_cfws(value) == value.index(b"<root@x>")
    assert (
        mime_identifiers.parse_message_identifier(value, field="Content-ID")
        == b"root@x"
    )


def test_identifier_sequence_requires_a_separator_after_each_complete_identifier() -> (
    None
):
    """A second identifier may follow only CFWS, while a literal tail is rejected."""
    assert mime_identifiers.parse_message_identifier_sequence(
        b"<first@x> (between) <second@x>", field="related start-info"
    ) == (b"first@x", b"second@x")

    with pytest.raises(AppError) as captured:
        mime_identifiers.parse_message_identifier_sequence(
            b"<first@x><second@x>", field="related start-info"
        )
    assert captured.value == AppError(
        ExitCode.PARSE_ERROR, "malformed related start-info"
    )


def test_cfws_rejects_backward_or_stalled_internal_cursor_transitions(
    monkeypatch: pytest.MonkeyPatch,
) -> None:
    """Scanner transition guards fail before a compromised helper can repeat."""
    with monkeypatch.context() as context:
        context.setattr(
            mime_identifiers,
            "_skip_folding_white_space",
            lambda _value, position: position - 1,
        )
        with pytest.raises(AppError) as backward:
            mime_identifiers._skip_cfws(  # ruff: ignore[private-member-access] - direct bounded-transition receipt.
                b"X", 0
            )
    assert backward.value == AppError(ExitCode.PARSE_ERROR, "malformed MIME comment")

    with monkeypatch.context() as context:
        context.setattr(
            mime_identifiers, "_skip_comment", lambda _value, position: position
        )
        with pytest.raises(AppError) as stalled:
            mime_identifiers._skip_cfws(  # ruff: ignore[private-member-access] - comment must strictly advance.
                b"(comment)", 0
            )
    assert stalled.value == AppError(ExitCode.PARSE_ERROR, "malformed MIME comment")

    with monkeypatch.context() as context:
        context.setattr(
            mime_identifiers, "_skip_comment", lambda _value, position: position - 1
        )
        with pytest.raises(AppError) as backward_comment:
            mime_identifiers._skip_cfws(  # ruff: ignore[private-member-access] - comment cursor must remain bounded and forward.
                b"(comment)", 0
            )
    assert backward_comment.value == AppError(
        ExitCode.PARSE_ERROR, "malformed MIME comment"
    )


def test_cfws_byte_budget_rejects_an_exhausted_comment_cursor(
    monkeypatch: pytest.MonkeyPatch,
) -> None:
    """Each source byte permits at most one comment transition."""
    with monkeypatch.context() as context:
        context.setattr(
            mime_identifiers, "_skip_comment", lambda _value, position: position + 1
        )
        with pytest.raises(AppError) as exhausted:
            mime_identifiers._skip_cfws(b"(((", 0)  # ruff: ignore[private-member-access] - bounded comment-transition receipt.
    assert exhausted.value == AppError(
        ExitCode.PARSE_ERROR, "malformed MIME comment cursor"
    )


def test_identifier_sequence_rejects_a_cursor_that_moves_backward(
    monkeypatch: pytest.MonkeyPatch,
) -> None:
    """The sequence loop refuses a post-identifier CFWS cursor rewind."""
    calls = 0

    def cursor(_value: bytes, position: int) -> int:
        nonlocal calls
        calls += 1
        return 0 if calls == 1 else position - 1

    with monkeypatch.context() as context:
        context.setattr(mime_identifiers, "_skip_cfws", cursor)
        with pytest.raises(AppError) as backward:
            mime_identifiers.parse_message_identifier_sequence(
                b"<only@x>", field="related start-info"
            )
    assert backward.value == AppError(
        ExitCode.PARSE_ERROR, "malformed related start-info"
    )
