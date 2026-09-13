"""Exact quoted-pair state receipts after the finite MIME scanner refactor."""

from __future__ import annotations

from eml_attachment_remover import mime_validation


def test_unquote_distinguishes_leading_and_escaped_backslash_pairs() -> None:
    """Quoted-pair state removes one slash and retains the escaped octet exactly."""
    unquote = mime_validation._unquote  # ruff: ignore[private-member-access] - direct finite-state receipt.
    assert unquote(b'"\\leading"') == b"leading"
    assert unquote(b'"two\\\\slashes"') == b"two\\slashes"
