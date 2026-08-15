"""Require safe diagnostics and metadata cleanup in nested messages."""

from __future__ import annotations

import tempfile
from email import policy
from email.message import EmailMessage
from pathlib import Path

from eml_attachment_remover import process_file
from eml_attachment_remover.processing import STALE_ROOT_HEADERS
from tests.test_support import parse


def _nested_message() -> EmailMessage:
    """Return an inline forwarded message with a removable PDF.

    Returns:
        A transport-signed logical message carrying stale metadata.

    """
    inner = EmailMessage()
    inner["Subject"] = "Nested public message"
    inner["DKIM-Signature"] = "v=1; d=public.example; b=PUBLIC"
    inner.set_content("Nested public body")
    inner.add_attachment(
        b"PUBLIC PDF",
        maintype="application",
        subtype="pdf",
        filename="nested.pdf",
    )
    for header in STALE_ROOT_HEADERS:
        inner[header] = "public stale value"
    wrapper = EmailMessage()
    wrapper.set_type("message/rfc822")
    wrapper.set_payload([inner])
    for header in STALE_ROOT_HEADERS:
        wrapper[header] = "public stale value"
    return wrapper


def test_nested_message_removal_warns_and_clears_changed_boundaries() -> None:
    """Warn for a changed inner signature and clear every affected boundary."""
    message = EmailMessage()
    message.set_content("Outer public body")
    message.make_mixed()
    message.attach(_nested_message())
    for header in STALE_ROOT_HEADERS:
        message[header] = "public stale value"

    with tempfile.TemporaryDirectory() as directory:
        source = Path(directory) / "source.eml"
        destination = Path(directory) / "output.eml"
        source.write_bytes(message.as_bytes(policy=policy.SMTP))

        result = process_file(source, destination, force=False, dry_run=False)
        output = parse(destination)

    assert [part.filename for part in result.removed] == ["nested.pdf"]
    expected_warning = (
        "rewriting nested message at MIME path 2.1 invalidates existing "
        "transport signatures (DKIM-Signature); the original EML remains unchanged"
    )
    assert result.warnings == (expected_warning,)
    changed_parts = [
        part
        for part in output.walk()
        if part is output
        or part.get_content_type() == "message/rfc822"
        or part["Subject"] == "Nested public message"
    ]
    assert len(changed_parts) == 3
    for part in changed_parts:
        assert all(part[header] is None for header in STALE_ROOT_HEADERS)
