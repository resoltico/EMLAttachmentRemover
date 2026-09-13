"""Exact raw/header boundary receipts for remaining MIME mutations."""

from __future__ import annotations

from types import SimpleNamespace
from typing import TYPE_CHECKING, cast

import pytest

from eml_attachment_remover import mime_headers, mime_raw, mime_stdlib_check
from eml_attachment_remover.domain import AppError, ExitCode
from eml_attachment_remover.mime_validation import ContentSpec

if TYPE_CHECKING:
    from email.message import EmailMessage


def test_line_end_counts_a_newline_at_zero_and_mbox_offset_uses_bounds() -> None:
    """A physical CRLF at offset zero and a nonzero mbox start remain exact."""
    assert mime_headers.line_end(b"\r\nbody", 0, 6) == 2
    prefix = b"skip"
    raw = prefix + b"From a@x\r\nX: one\r\n"
    assert mime_headers._first_header_offset(  # ruff: ignore[private-member-access] - exact mbox offset receipt.
        raw, len(prefix), len(raw)
    ) == len(prefix) + len(b"From a@x\r\n")


def test_header_parser_treats_space_as_continuation_and_x_as_value() -> None:
    """A leading SP owns the preceding field, while a trailing X remains content."""
    raw = b"X-One: first\r\n continued X\r\nX-Two: X\r\n"
    assert mime_headers.parse_headers(raw, 0, len(raw)) == (
        mime_headers.Header(b"x-one", b"first\r\n continued X", 0, 28),
        mime_headers.Header(b"x-two", b"X", 28, len(raw)),
    )


def test_entity_headers_do_not_search_beyond_the_bounded_entity() -> None:
    """A header-like partial entity fails instead of borrowing a later separator."""
    raw = b"X: one\r\n\r\nbody"
    with pytest.raises(AppError) as rejected:
        mime_raw._entity_headers(raw, 0, 6)  # ruff: ignore[private-member-access] - bounded entity separator receipt.
    assert rejected.value == AppError(
        ExitCode.PARSE_ERROR, "header-like MIME entity lacks a body separator"
    )


def test_multipart_child_limit_accepts_the_exact_configured_ceiling(
    monkeypatch: pytest.MonkeyPatch,
) -> None:
    """Exactly the configured direct-child ceiling is accepted, not over-limit."""
    raw = b"--m\r\n\r\na\r\n--m--\r\n"
    parent = mime_raw.RawNode(
        (7,),
        0,
        len(raw),
        0,
        (),
        ContentSpec("multipart/mixed", {b"boundary": b"m"}),
        None,
        "7bit",
    )
    with monkeypatch.context() as context:
        context.setattr(mime_raw, "MAX_CHILDREN", 1)
        mime_raw._parse_multipart(  # ruff: ignore[private-member-access] - inclusive child-limit receipt.
            raw, parent, [0, 0]
        )
    assert [child.path for child in parent.children] == [(7, 0)]


def test_stdlib_header_defect_receipt_handles_empty_and_present_defects() -> None:
    """An empty stdlib defect collection is clean; a populated one fails exactly."""
    clean = cast("EmailMessage", _HeaderPart(SimpleNamespace(defects=())))
    mime_stdlib_check._validate_headers(clean)  # ruff: ignore[private-member-access] - direct stdlib-header receipt.
    broken = cast("EmailMessage", _HeaderPart(SimpleNamespace(defects=(ValueError(),))))
    with pytest.raises(AppError) as rejected:
        mime_stdlib_check._validate_headers(broken)  # ruff: ignore[private-member-access] - exact stdlib-header defect receipt.
    assert rejected.value == AppError(
        ExitCode.PARSE_ERROR, "MIME header parser reported a defect"
    )


class _HeaderPart:
    """Minimal mapping/iterator double for independent stdlib header inspection."""

    def __init__(self, header: object) -> None:
        self._header = header

    def __iter__(self) -> object:
        return iter(("X-Test",))

    def __getitem__(self, _name: str) -> object:
        return self._header
