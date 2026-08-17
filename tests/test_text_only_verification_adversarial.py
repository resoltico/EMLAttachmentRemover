# ruff: file-ignore[private-member-access]
"""Adversarial verification boundaries for text-only MIME output."""

from __future__ import annotations

import tempfile
from email import policy
from email.message import EmailMessage
from pathlib import Path
from typing import TYPE_CHECKING

import pytest

from eml_attachment_remover import mime_serialization, process_file, processing
from eml_attachment_remover.mime_text_execution import execute_text_only_plan
from eml_attachment_remover.models import CliError, ExitCode

if TYPE_CHECKING:
    from eml_attachment_remover.mime_text_only import TextOnlyPlan


def _plain() -> EmailMessage:
    """Return one safe synthetic body.

    Returns:
        The configured plain-text leaf.

    """
    part = EmailMessage()
    part.set_content("PUBLIC SELECTED BODY")
    return part


def _html() -> EmailMessage:
    """Return one synthetic discarded representation.

    Returns:
        The configured HTML leaf.

    """
    part = EmailMessage()
    part.set_content("<p>PUBLIC HTML</p>", subtype="html")
    return part


def _alternative() -> EmailMessage:
    """Return a canonical plain/HTML alternative.

    Returns:
        The populated alternative container.

    """
    message = EmailMessage()
    message.make_alternative()
    message.attach(_plain())
    message.attach(_html())
    return message


def _mixed() -> EmailMessage:
    """Return an unsafe mixed plain/HTML tree.

    Returns:
        The populated mixed container.

    """
    message = EmailMessage()
    message.make_mixed()
    message.attach(_plain())
    message.attach(_html())
    return message


def test_storage_verifier_rejects_a_self_consistent_non_text_only_tree() -> None:
    """Require semantic second-pass no-op in addition to matching fingerprints."""
    unsafe = _mixed()
    raw = unsafe.as_bytes(policy=policy.SMTP)
    with tempfile.TemporaryDirectory() as directory:
        temporary = Path(directory) / "unsafe.eml"
        temporary.write_bytes(raw)
        parsed, _warnings = mime_serialization._parse_message(raw, str(temporary))
        expected_leaves = mime_serialization._leaf_fingerprints(parsed)
        expected_structure = mime_serialization._structure_fingerprint(parsed)

        with pytest.raises(CliError) as raised:
            mime_serialization._verify_serialized_message(
                temporary,
                expected_leaves,
                expected_structure,
            )

    assert raised.value.code is ExitCode.VERIFICATION_ERROR
    assert "not a safe text-only message" in raised.value.message


def test_executor_payload_corruption_is_code_eight_and_never_published(
    monkeypatch: pytest.MonkeyPatch,
) -> None:
    """Bind selected source content before storage derives output fingerprints."""
    message = _alternative()
    raw = message.as_bytes(policy=policy.SMTP)

    def corrupt_selected_body(
        parsed: EmailMessage,
        plan: TextOnlyPlan,
    ) -> None:
        execute_text_only_plan(parsed, plan)
        selected = next(
            part
            for part in parsed.walk()
            if not part.is_multipart() and part.get_content_type() == "text/plain"
        )
        selected.set_payload("PUBLIC CORRUPTED BODY")

    monkeypatch.setattr(
        processing,
        "execute_text_only_plan",
        corrupt_selected_body,
    )
    with tempfile.TemporaryDirectory() as directory:
        base = Path(directory)
        source = base / "source.eml"
        output = base / "output.eml"
        source.write_bytes(raw)

        with pytest.raises(CliError) as raised:
            process_file(source, output, force=False, dry_run=False)

        assert raised.value.code is ExitCode.VERIFICATION_ERROR
        assert source.read_bytes() == raw
        assert not output.exists()
