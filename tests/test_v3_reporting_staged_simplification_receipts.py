"""Receipts for reporting and staged equivalent-surface simplifications."""

from __future__ import annotations

from eml_attachment_remover import reporting_v3


def test_canonical_json_uses_one_ascii_transport_independent_policy() -> None:
    """The production serializer has no caller-selectable Unicode visibility mode."""
    assert (
        reporting_v3._canonical_json(  # ruff: ignore[private-member-access] - internal transport policy.
            {"public": "π"}
        )
        == '{"public": "\\u03c0"}'
    )
