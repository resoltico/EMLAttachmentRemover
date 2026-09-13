"""Final exact low-level MIME receipts for the current mutation batch."""

from __future__ import annotations

import pytest

from eml_attachment_remover import mime_execution, mime_validation
from eml_attachment_remover.domain import AppError, ExitCode


def test_parameter_name_preserves_extended_and_numbered_forms() -> None:
    """Star spelling distinguishes direct extensions from numbered continuations."""
    assert mime_validation._parameter_name(  # ruff: ignore[private-member-access] - direct extended-name receipt.
        b"filename*"
    ) == (b"filename", None, True)
    assert mime_validation._parameter_name(  # ruff: ignore[private-member-access] - numbered extended-name receipt.
        b"filename*12*"
    ) == (b"filename", 12, True)


def test_nonoverlapping_sorts_adjacent_raw_spans_without_coalescing() -> None:
    """Adjacent spans are separately retained in sorted source order."""
    assert mime_execution._nonoverlapping(  # ruff: ignore[private-member-access] - exact adjacency receipt.
        [(5, 8), (2, 5), (8, 11)]
    ) == [(2, 5), (5, 8), (8, 11)]
    with pytest.raises(AppError) as rejected:
        mime_execution._nonoverlapping(  # ruff: ignore[private-member-access] - negative edit-span receipt.
            [(-1, 1)]
        )
    assert rejected.value == AppError(
        ExitCode.VERIFICATION_ERROR, "overlapping raw MIME edits"
    )
