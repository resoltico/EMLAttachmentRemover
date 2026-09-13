"""Narrow exact receipts for raw MIME parameter and delimiter invariants."""

from __future__ import annotations

import pytest

from eml_attachment_remover import mime_raw, mime_validation
from eml_attachment_remover.domain import AppError, ExitCode


def test_parameter_store_distinguishes_direct_overlap_and_duplicate_segment() -> None:
    """Direct ownership and numbered-piece ownership have separate exact failures."""
    parameters = {b"name": b"one"}
    with pytest.raises(AppError) as overlap:
        mime_validation._store_parameter(  # ruff: ignore[private-member-access] - direct/continuation overlap receipt.
            parameters, {}, b"name", 0, b"two"
        )
    assert overlap.value == AppError(ExitCode.PARSE_ERROR, "overlapping MIME parameter")
    continuations = {b"name": {0: b"one"}}
    with pytest.raises(AppError) as duplicate:
        mime_validation._store_parameter(  # ruff: ignore[private-member-access] - duplicate segment receipt.
            {}, continuations, b"name", 0, b"two"
        )
    assert duplicate.value == AppError(
        ExitCode.PARSE_ERROR, "duplicate MIME parameter segment"
    )


def test_unquote_and_delimiter_keep_nontransport_tail_bytes_visible() -> None:
    """Quoted pairs decode once; vertical whitespace cannot end a delimiter."""
    assert (
        mime_validation._unquote(  # ruff: ignore[private-member-access] - exact quote-pair receipt.
            b'"a\\\\b"'
        )
        == b"a\\b"
    )
    assert (
        mime_raw._delimiter_lines(  # ruff: ignore[private-member-access] - vertical-tail delimiter receipt.
            b"--m\v\r\n", 0, 7, b"m"
        )
        == []
    )
