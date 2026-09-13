"""Exact MIME parsing receipts for remaining semantic mutation boundaries."""

from __future__ import annotations

import pytest

from eml_attachment_remover import mime_headers, mime_identifiers, mime_raw
from eml_attachment_remover.domain import AppError, ExitCode
from eml_attachment_remover.mime_headers import Header
from eml_attachment_remover.mime_validation import ContentSpec


def test_mbox_header_offset_and_parser_honor_the_callers_entity_end() -> None:
    """An mbox envelope consumes one bounded wire line before fields are parsed."""
    prefix = b"skip\n"
    envelope = b"From sender@example.test\r\n"
    fields = b"Subject: retained\r\n"
    raw = prefix + envelope + fields + b"outside"
    start = len(prefix)
    separator = start + len(envelope) + len(fields)

    assert mime_headers._first_header_offset(  # ruff: ignore[private-member-access] - bounded mbox cursor receipt.
        raw, start, separator
    ) == start + len(envelope)
    assert mime_headers.parse_headers(raw, start, separator) == (
        Header(b"subject", b"retained", start + len(envelope), separator),
    )


def test_mbox_offset_and_parser_forward_the_exact_bounded_entity_end(
    monkeypatch: pytest.MonkeyPatch,
) -> None:
    """The outer parser forwards its caller's exact entity boundary to mbox parsing."""
    raw = b"From sender@example.test\r\nSubject: retained\r\noutside"
    separator = raw.index(b"outside")
    original = mime_headers._first_header_offset  # ruff: ignore[private-member-access] - bounded mbox collaboration receipt.
    observed: list[int] = []

    def bounded_offset(source: bytes, start: int, end: int) -> int:
        observed.append(end)
        return original(source, start, end)

    monkeypatch.setattr(mime_headers, "_first_header_offset", bounded_offset)
    assert mime_headers.parse_headers(raw, 0, separator) == (
        Header(
            b"subject", b"retained", len(b"From sender@example.test\r\n"), separator
        ),
    )
    assert observed == [separator]


def test_mbox_offset_cannot_search_past_its_explicit_entity_end() -> None:
    """An envelope terminator beyond the entity is unavailable to its header offset."""
    raw = b"From sender@example.test\r\nSubject: retained\r\n"
    separator = raw.index(b"\r\n")
    assert (
        mime_headers._first_header_offset(  # ruff: ignore[private-member-access] - bounded mbox cursor receipt.
            raw, 0, separator
        )
        == separator
    )


@pytest.mark.parametrize("name", sorted(mime_headers.SINGLETONS))
def test_each_singleton_duplicate_reports_its_exact_ascii_label(name: bytes) -> None:
    """Every singleton control has a stable public diagnostic label."""
    header = Header(name, b"value", 0, len(name) + 7)
    with pytest.raises(AppError) as duplicate:
        mime_headers._validate_header_multiplicity(  # ruff: ignore[private-member-access] - per-control singleton receipt.
            [header, header]
        )
    assert duplicate.value == AppError(
        ExitCode.PARSE_ERROR,
        f"duplicate singleton MIME header {mime_headers.SINGLETON_LABELS[name]}",
    )


def test_cfws_scanners_stop_at_the_first_non_whitespace_byte() -> None:
    """Literal X is significant rather than folding whitespace."""
    assert (
        mime_identifiers._skip_folding_white_space(  # ruff: ignore[private-member-access] - exact CFWS boundary receipt.
            b" \tX", 0
        )
        == 2
    )
    assert mime_identifiers.first_non_cfws(b" (comment)\tX") == 11


def test_shallow_node_honors_its_explicit_entity_end() -> None:
    """A shallow entity cannot borrow bytes after its caller supplied end."""
    raw = b"Content-Type: text/plain\r\n\r\nbodyoutside"
    end = raw.index(b"outside")
    node = mime_raw._shallow_node(  # ruff: ignore[private-member-access] - bounded shallow-node receipt.
        raw, 0, end, (), [0, 0]
    )
    assert (node.end, node.body_start, node.media_type) == (
        end,
        len(b"Content-Type: text/plain\r\n\r\n"),
        "text/plain",
    )
    assert node.disposition is None
    assert node.content_type == ContentSpec("text/plain", {})


def test_shallow_node_cannot_borrow_a_body_separator_past_its_entity_end() -> None:
    """A header-like fragment without its separator is rejected rather than widened."""
    raw = b"Content-Type: text/plain\r\n\r\nbody"
    end = raw.index(b"\r\n\r\n")
    with pytest.raises(AppError) as rejected:
        mime_raw._shallow_node(  # ruff: ignore[private-member-access] - bounded shallow-node rejection receipt.
            raw, 0, end, (), [0, 0]
        )
    assert rejected.value == AppError(
        ExitCode.PARSE_ERROR, "header-like MIME entity lacks a body separator"
    )
