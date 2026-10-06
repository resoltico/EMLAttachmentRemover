"""Exact related-compound and message-identifier policy receipts."""

from __future__ import annotations

import pytest

from eml_attachment_remover import mime_identifiers, mime_policy
from eml_attachment_remover.domain import (
    AppError,
    DecisionAction,
    ExitCode,
    Removal,
    RemovalReason,
)
from eml_attachment_remover.mime_headers import Header
from eml_attachment_remover.mime_raw import RawNode, parse_raw_mime
from eml_attachment_remover.mime_validation import ContentSpec


def test_related_keeps_nested_start_info_targets_below_the_declared_root() -> None:
    """Related metadata may name retained descendants, but no removed resources."""
    raw = (
        b"Content-Type: multipart/related; boundary=outer; "
        b'type="multipart/alternative"; start="<root@x>"; '
        b'start-info="<plain@x> (and) <html@x>"\r\n\r\n'
        b"--outer\r\nContent-Type: multipart/alternative; boundary=inner\r\n"
        b"Content-ID: <root@x>\r\n\r\n"
        b"--inner\r\nContent-Type: text/plain\r\nContent-ID: <plain@x>\r\n\r\n"
        b"plain\r\n--inner\r\nContent-Type: text/html\r\n"
        b"Content-ID: <html@x>\r\n\r\n<html/>\r\n--inner--\r\n"
        b"--outer\r\nContent-Type: image/png\r\nContent-ID: <resource@x>\r\n\r\n"
        b"opaque\r\n--outer--\r\n"
    )
    plan = mime_policy.classify(parse_raw_mime(raw).root)
    assert plan.actions == {
        (): DecisionAction.RECURSE,
        (0,): DecisionAction.RECURSE,
        (0, 0): DecisionAction.KEEP,
        (0, 1): DecisionAction.KEEP,
        (1,): DecisionAction.REMOVE_SUBTREE,
    }
    assert plan.removals == (
        Removal((1,), "image/png", RemovalReason.RELATED_NONROOT_COMPONENT),
    )


def test_related_start_cannot_select_a_descendant_instead_of_a_direct_root() -> None:
    """A ``start`` ID names exactly one direct related child, never a descendant."""
    nested = RawNode(
        (0, 0),
        0,
        0,
        0,
        (Header(b"content-id", b"<nested@x>", 0, 0),),
        ContentSpec("text/plain", {}),
        None,
        "7bit",
    )
    candidate = RawNode(
        (0,),
        0,
        0,
        0,
        (),
        ContentSpec("multipart/alternative", {}),
        None,
        "7bit",
        [nested],
    )
    root = RawNode(
        (),
        0,
        0,
        0,
        (),
        ContentSpec("multipart/related", {b"start": b"<nested@x>"}),
        None,
        "7bit",
        [candidate],
    )
    with pytest.raises(AppError) as captured:
        mime_policy.classify(root)
    assert captured.value == AppError(
        ExitCode.TRANSFORMATION_UNAVAILABLE,
        "related start has no direct root",
        (),
    )


def test_related_rejects_duplicate_ids_across_direct_and_nested_nodes() -> None:
    """The related ID namespace covers the full compound, not merely direct children."""
    raw = (
        b"Content-Type: multipart/related; boundary=outer\r\n\r\n"
        b"--outer\r\nContent-Type: multipart/alternative; boundary=inner\r\n"
        b"Content-ID: <root@x>\r\n\r\n"
        b"--inner\r\nContent-Type: text/plain\r\nContent-ID: <same@x>\r\n\r\n"
        b"plain\r\n--inner--\r\n--outer\r\nContent-Type: image/png\r\n"
        b"Content-ID: <same@x>\r\n\r\nopaque\r\n--outer--\r\n"
    )
    with pytest.raises(AppError) as captured:
        mime_policy.classify(parse_raw_mime(raw).root)
    assert captured.value == AppError(
        ExitCode.TRANSFORMATION_UNAVAILABLE,
        "duplicate canonical Content-ID in multipart/related",
        (0, 0),
    )


def test_related_id_index_maps_every_exact_identifier_to_its_source_node() -> None:
    """The policy index preserves exact CID bytes and the unique original nodes."""
    raw = (
        b"Content-Type: multipart/related; boundary=outer\r\n\r\n"
        b"--outer\r\nContent-Type: multipart/alternative; boundary=inner\r\n"
        b"Content-ID: <root@x>\r\n\r\n"
        b"--inner\r\nContent-Type: text/plain\r\nContent-ID: <plain@x>\r\n\r\n"
        b"plain\r\n--inner--\r\n--outer\r\nContent-Type: image/png\r\n"
        b"Content-ID: <resource@x>\r\n\r\nopaque\r\n--outer--\r\n"
    )
    root = parse_raw_mime(raw).root
    assert {
        identifier: node.path
        for identifier, node in mime_policy._related_ids(root).items()  # ruff: ignore[private-member-access] - complete related namespace receipt.
    } == {
        b"root@x": (0,),
        b"plain@x": (0, 0),
        b"resource@x": (1,),
    }
    assert mime_policy._cid(root) is None  # ruff: ignore[private-member-access] - absent root Content-ID receipt.


def test_related_identifier_helpers_preserve_exact_cfws_wrapped_sequences() -> None:
    """Policy metadata accepts only ID syntax and preserves inner bytes exactly."""
    assert mime_policy._parse_start(b" (lead) <root+tag@x> (tail) ") == b"root+tag@x"  # ruff: ignore[private-member-access] - exact related start grammar.
    assert mime_policy._start_info_ids(  # ruff: ignore[private-member-access] - exact related start-info grammar.
        b"(lead)<first@x> (between) <second+tag@x>(tail)"
    ) == (b"first@x", b"second+tag@x")
    assert mime_policy._start_info_ids(b"text/plain --mode=display") is None  # ruff: ignore[private-member-access] - literal start-info receipt.
    with pytest.raises(AppError) as captured:
        mime_policy._start_info_ids(b"<same@x> (again) <same@x>")  # ruff: ignore[private-member-access] - duplicate retained metadata must fail.
    assert captured.value == AppError(
        ExitCode.TRANSFORMATION_UNAVAILABLE, "duplicate related start-info ID"
    )


def test_identifier_comment_scanner_returns_exact_cursors_and_errors() -> None:
    """Nested and escaped comments are CFWS; line breaks and untermination fail."""
    comment = b"(outer (nested) escaped\\) close) <id@x>"
    assert mime_identifiers._skip_comment(comment, 0) == comment.index(b" <")  # ruff: ignore[private-member-access] - nested RFC comment cursor.
    assert mime_identifiers.first_non_cfws(b" \t(one (two))\r\n <id@x>") == 16
    for value, message in (
        (b"(line\r) <id@x>", "malformed MIME comment"),
        (b"(open", "unterminated MIME comment"),
        (b"(escaped\\", "unterminated MIME comment"),
    ):
        with pytest.raises(AppError) as captured:
            mime_identifiers.first_non_cfws(value)
        assert captured.value == AppError(ExitCode.PARSE_ERROR, message)


def test_identifier_parser_keeps_inner_octets_and_rejects_every_invalid_form() -> None:
    """The shared parser provides exact IDs and a field-qualified malformed receipt."""
    value = b"<a.b+tag@x> trailing"
    assert mime_identifiers._message_identifier_at(  # ruff: ignore[private-member-access] - exact low-level ID cursor.
        value, 0, field="related"
    ) == (b"a.b+tag@x", 11)
    assert mime_identifiers.parse_message_identifier_sequence(
        b"(lead)<one@x>\t(two)<two+tag@x>(tail)", field="related"
    ) == (b"one@x", b"two+tag@x")
    for malformed in (
        b"",
        b"<>",
        b"<id with space>",
        b"<id\r\n>",
        b"<id> trailing",
    ):
        with pytest.raises(AppError) as captured:
            mime_identifiers.parse_message_identifier(malformed, field="related")
        assert captured.value == AppError(ExitCode.PARSE_ERROR, "malformed related")
