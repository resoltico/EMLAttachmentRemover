"""Focused parser and parameter grammar receipts."""

from __future__ import annotations

import pytest

from eml_attachment_remover import mime_raw, mime_validation
from eml_attachment_remover.domain import AppError, ExitCode
from eml_attachment_remover.mime_headers import Header
from eml_attachment_remover.mime_validation import ContentSpec


def test_parameter_piece_keeps_name_segment_and_raw_value_boundaries() -> None:
    """RFC 2231 pieces preserve a numbered extension and decoded quote-pair bytes."""
    assert mime_validation._parameter_piece(  # ruff: ignore[private-member-access] - exact parameter-piece receipt.
        b"Filename*1*=two%20words"
    ) == (b"filename", 1, b"two%20words")
    assert mime_validation._parameter_piece(  # ruff: ignore[private-member-access] - quoted ordinary parameter receipt.
        b' name = "a\\;b" '
    ) == (b"name", None, b"a;b")


def test_parameter_grammar_failures_keep_exact_parse_errors() -> None:
    """Malformed extensions, encoded quotes, and gapped continuations do not repair."""
    with pytest.raises(AppError) as extension:
        mime_validation._parameter_name(  # ruff: ignore[private-member-access] - malformed extension receipt.
            b"name*bad"
        )
    assert extension.value == AppError(
        ExitCode.PARSE_ERROR, "malformed MIME parameter extension"
    )
    with pytest.raises(AppError) as quoted:
        mime_validation._parameter_value(  # ruff: ignore[private-member-access] - encoded quote receipt.
            b"\"utf-8''x\"", encoded=True, initial=True
        )
    assert quoted.value == AppError(ExitCode.PARSE_ERROR, "quoted RFC 2231 parameter")
    with pytest.raises(AppError) as gapped:
        mime_validation._finish_continuations(  # ruff: ignore[private-member-access] - continuation-gap receipt.
            {}, {b"name": {1: b"late"}}
        )
    assert gapped.value == AppError(
        ExitCode.PARSE_ERROR, "gapped MIME parameter continuation"
    )


def test_header_value_unfolds_only_the_matching_physical_control() -> None:
    """Header lookup unfolds matching lines and ignores extension fields."""
    headers = (
        Header(b"x-content-type", b"ignored", 0, 24),
        Header(b"content-type", b" text/plain\r\n\t; charset=utf-8 ", 24, 70),
    )
    assert (
        mime_validation._header_value(  # ruff: ignore[private-member-access] - physical unfold receipt.
            headers, b"content-type"
        )
        == b"text/plain ; charset=utf-8"
    )


def test_related_root_requires_one_unique_direct_content_id() -> None:
    """A related start selects exactly one direct child, never a missing identifier."""
    parent = mime_raw.RawNode(
        (3,),
        0,
        0,
        0,
        (),
        ContentSpec("multipart/related", {b"start": b"<root@x>"}),
        None,
        "7bit",
    )
    child = mime_raw.RawNode(
        (3, 0),
        0,
        0,
        0,
        (Header(b"content-id", b"<other@x>", 0, 18),),
        ContentSpec("text/plain", {}),
        None,
        "7bit",
    )
    with pytest.raises(AppError) as rejected:
        mime_raw._related_root_index(parent, [child])  # ruff: ignore[private-member-access] - direct related-root receipt.
    assert rejected.value == AppError(
        ExitCode.PARSE_ERROR, "related start has no unique direct root", (3,)
    )
