"""Narrow exact receipts for raw MIME parameter and delimiter invariants."""

from __future__ import annotations

import pytest

from eml_attachment_remover import mime_parameters, mime_raw
from eml_attachment_remover.domain import AppError, ExitCode


def test_parameter_store_distinguishes_direct_overlap_and_duplicate_segment() -> None:
    """Direct ownership and numbered-piece ownership have separate exact failures."""
    with pytest.raises(AppError) as overlap:
        mime_parameters.structured_parameters([b"name=one", b"name*0=two"])
    assert overlap.value == AppError(ExitCode.PARSE_ERROR, "overlapping MIME parameter")
    with pytest.raises(AppError) as duplicate:
        mime_parameters.structured_parameters([b"name*0=one", b"name*0=two"])
    assert duplicate.value == AppError(
        ExitCode.PARSE_ERROR, "duplicate MIME parameter segment"
    )


def test_unquote_and_delimiter_keep_nontransport_tail_bytes_visible() -> None:
    """Quoted pairs decode once; vertical whitespace cannot end a delimiter."""
    assert (
        mime_parameters._unquote(  # ruff: ignore[private-member-access] - exact quote-pair receipt.
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
