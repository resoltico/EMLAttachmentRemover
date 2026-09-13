"""Receipts for reporting and staged equivalent-surface simplifications."""

from __future__ import annotations

import pytest

from eml_attachment_remover import reporting_v3


def test_canonical_json_rejects_nonboolean_visibility_configuration() -> None:
    """The report serializer distinguishes false from a falsey non-boolean value."""
    with pytest.raises(TypeError, match="JSON ensure_ascii must be a boolean"):
        reporting_v3._canonical_json(  # ruff: ignore[private-member-access] - internal JSON policy guard.
            {"public": "π"}, ensure_ascii=None
        )
