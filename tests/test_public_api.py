"""Verify the package's intentionally lazy public processing interface."""

from __future__ import annotations

import pytest

import eml_attachment_remover
from eml_attachment_remover import models, processing


def test_public_process_result_is_the_canonical_model() -> None:
    """Resolve documented lazy objects without introducing duplicate identities."""
    assert eml_attachment_remover.ProcessResult is models.ProcessResult
    assert eml_attachment_remover.process_file is processing.process_file


def test_unknown_public_attribute_preserves_the_requested_name() -> None:
    """Expose Python's exact missing-attribute context to callers."""
    name = "public_missing_name"
    with pytest.raises(AttributeError) as raised:
        getattr(eml_attachment_remover, name)
    assert raised.value.args == (name,)
