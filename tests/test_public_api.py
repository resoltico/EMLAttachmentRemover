"""Verify the package's intentionally lazy public processing interface."""

from __future__ import annotations

from dataclasses import fields

import pytest

import eml_attachment_remover
from eml_attachment_remover import models, processing


def test_public_process_result_is_the_canonical_model() -> None:
    """Resolve documented lazy objects without introducing duplicate identities."""
    assert eml_attachment_remover.__doc__ == (
        "Create verified text-only EML working copies without ordinary attachments."
    )
    assert eml_attachment_remover.ProcessResult is models.ProcessResult
    assert eml_attachment_remover.process_file is processing.process_file


def test_process_result_exposes_only_the_v2_text_only_audit_contract() -> None:
    """Require the clean public break from ambiguous v1 result fields."""
    assert tuple(field.name for field in fields(models.ProcessResult)) == (
        "source",
        "destination",
        "source_size",
        "output_size",
        "removed_attachments",
        "selected_plain_text_bodies",
        "discarded_body_representations",
        "discarded_body_resources",
        "warnings",
        "dry_run",
    )


def test_unknown_public_attribute_preserves_the_requested_name() -> None:
    """Expose Python's exact missing-attribute context to callers."""
    name = "public_missing_name"
    with pytest.raises(AttributeError) as raised:
        getattr(eml_attachment_remover, name)
    assert raised.value.args == (name,)
