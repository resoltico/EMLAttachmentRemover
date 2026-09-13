"""Exact MIME parsing receipts for remaining semantic mutation boundaries."""

from __future__ import annotations

import pytest

from eml_attachment_remover import mime_headers, mime_identifiers, mime_raw
from eml_attachment_remover.domain import AppError, ExitCode
from eml_attachment_remover.mime_headers import Header
from eml_attachment_remover.mime_validation import ContentSpec


def test_mbox_header_offset_and_parser_honor_the_callers_entity_end() -> None:
    """An mbox envelope consumes one bounded wire line before fields are parsed."""
    prefix = b"skip"
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
