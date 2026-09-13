"""Additional narrow source-bound MIME receipts."""

from __future__ import annotations

import pytest

from eml_attachment_remover import mime_encoding, mime_headers, mime_raw
from eml_attachment_remover.domain import AppError, ExitCode


def test_line_end_prefers_the_earliest_physical_newline() -> None:
    """A CR occurring before LF ends a line even when both are in range."""
    assert mime_headers.line_end(b"a\rb\n", 0, 4) == 2
    assert mime_headers.line_end(b"a\nb\r", 0, 4) == 2


def test_delimiter_rejects_suffix_and_separator_requires_a_marker() -> None:
    """Near delimiter text stays payload and a separator absence has an exact error."""
    assert (
        mime_raw._delimiter_lines(  # ruff: ignore[private-member-access] - exact delimiter-tail receipt.
            b"--bound-suffix\r\n", 0, 16, b"bound"
        )
        == []
    )
    with pytest.raises(AppError) as rejected:
        mime_raw._find_separator(  # ruff: ignore[private-member-access] - exact separator absence receipt.
            b"X: one\r\nbody", 0, 12
        )
    assert rejected.value == AppError(
        ExitCode.PARSE_ERROR, "MIME entity has no header/body separator"
    )


def test_base64_rejection_retains_a_stable_parse_error() -> None:
    """Non-base64 octets must not be silently repaired in retained payload bytes."""
    with pytest.raises(AppError) as rejected:
        mime_encoding.decode_payload(b"not*base64", "base64")
    assert rejected.value == AppError(
        ExitCode.PARSE_ERROR, "invalid retained base64 payload"
    )
