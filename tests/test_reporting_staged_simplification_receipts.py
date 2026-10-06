"""Receipts for reporting and staged equivalent-surface simplifications."""

from __future__ import annotations

from eml_attachment_remover import report_document


def test_canonical_json_uses_one_ascii_transport_independent_policy() -> None:
    """The production serializer has no caller-selectable Unicode visibility mode."""
    assert report_document.canonical_json({"public": "π"}) == '{"public": "\\u03c0"}'
