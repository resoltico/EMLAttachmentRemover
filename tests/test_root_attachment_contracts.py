"""Require root attachment policy to apply to MIME subtree payloads."""

from __future__ import annotations

import tempfile
from collections.abc import Callable
from email import policy
from email.message import EmailMessage
from pathlib import Path
from typing import Final

import pytest

from eml_attachment_remover import (
    mime_policy,
    mime_references,
    mime_serialization,
    process_file,
)
from eml_attachment_remover.models import CliError, ExitCode, RemovedPart
from tests.test_support import parse

ROOT_NOTICE: Final = (
    "[The original root attachment was removed from this derived EML copy.]"
)
type RootFactory = Callable[[], EmailMessage]


def _forwarded_message_root() -> EmailMessage:
    """Build a message/rfc822 root represented as a MIME subtree.

    Returns:
        A named root attachment containing one complete public message.

    """
    forwarded = EmailMessage()
    forwarded["Subject"] = "Public forwarded message"
    forwarded.set_content("Public forwarded body")
    root = EmailMessage()
    root["Subject"] = "Public root envelope"
    root["X-Public-Invariant"] = "retained"
    root["Content-Type"] = "message/rfc822"
    root["Content-Disposition"] = 'attachment; filename="forwarded.eml"'
    root.set_payload([forwarded])
    return root


def _multipart_attachment_root() -> EmailMessage:
    """Build a multipart/mixed root explicitly represented as an attachment.

    Returns:
        A named multipart root containing body and binary descendants.

    """
    root = EmailMessage()
    root["Subject"] = "Public multipart root"
    root["X-Public-Invariant"] = "retained"
    root.make_mixed()
    body = EmailMessage()
    body.set_content("Public nested body")
    root.attach(body)
    binary = EmailMessage()
    binary.set_content(
        b"PUBLIC NESTED PAYLOAD",
        maintype="application",
        subtype="octet-stream",
    )
    root.attach(binary)
    root["Content-Disposition"] = 'attachment; filename="bundle.eml"'
    return root


ROOT_CASES: Final[tuple[tuple[RootFactory, str, str], ...]] = (
    (_forwarded_message_root, "message/rfc822", "forwarded.eml"),
    (_multipart_attachment_root, "multipart/mixed", "bundle.eml"),
)


@pytest.mark.parametrize(("factory", "content_type", "filename"), ROOT_CASES)
def test_subtree_root_is_classified_and_removed_as_one_entity(
    factory: RootFactory,
    content_type: str,
    filename: str,
) -> None:
    """Record a removable subtree root at the root path, not its descendants."""
    message = factory()
    references = mime_references._collect_references(  # ruff: ignore[private-member-access]
        message
    )

    state = mime_policy._remove_attachments(  # ruff: ignore[private-member-access]
        message,
        references,
    )

    assert state.removed == [RemovedPart((), content_type, filename, "attachment")]
    assert message.get_content_type() == "text/plain"
    assert message.get_content().rstrip("\n") == ROOT_NOTICE
    assert message["X-Public-Invariant"] == "retained"


@pytest.mark.parametrize(("factory", "content_type", "filename"), ROOT_CASES)
def test_subtree_root_is_removed_end_to_end_without_touching_source(
    factory: RootFactory,
    content_type: str,
    filename: str,
) -> None:
    """Write a verified notice while preserving source bytes and root metadata."""
    raw = factory().as_bytes(policy=policy.SMTP)
    with tempfile.TemporaryDirectory() as directory:
        source = Path(directory) / "source.eml"
        destination = Path(directory) / "output.eml"
        source.write_bytes(raw)

        result = process_file(source, destination, force=False, dry_run=False)
        output = parse(destination)

        assert source.read_bytes() == raw
        assert result.removed == (
            RemovedPart((), content_type, filename, "attachment"),
        )
        assert result.preserved_file_parts == ()
        assert output.get_content_type() == "text/plain"
        assert output.get_content().rstrip("\r\n") == ROOT_NOTICE
        assert output["X-Public-Invariant"] == "retained"
        assert not output.is_multipart()


@pytest.mark.parametrize(("factory", "content_type", "filename"), ROOT_CASES)
def test_verifier_rejects_a_surviving_removable_subtree_root(
    factory: RootFactory,
    content_type: str,
    filename: str,
) -> None:
    """Detect an unchanged removable root even when all leaf hashes match."""
    with tempfile.TemporaryDirectory() as directory:
        output = Path(directory) / "unfiltered.eml"
        output.write_bytes(factory().as_bytes(policy=policy.SMTP))
        parsed = parse(output)
        expected = mime_serialization._leaf_fingerprints(  # ruff: ignore[private-member-access]
            parsed
        )

        with pytest.raises(CliError) as raised:
            mime_serialization._verify_serialized_message(  # ruff: ignore[private-member-access]
                output,
                expected,
            )

    assert raised.value.code is ExitCode.VERIFICATION_ERROR
    assert raised.value.message == (
        f"generated EML still contains removable attachment {filename!r}"
    )
    assert mime_serialization._remaining_removable_part(  # ruff: ignore[private-member-access]
        parsed
    ) == RemovedPart((), content_type, filename, "attachment")


def test_protected_multipart_root_precedes_explicit_attachment_disposition() -> None:
    """Never replace signed root content merely because it is named attachment."""
    signed = EmailMessage()
    signed["Content-Type"] = "multipart/signed"
    signed["Content-Disposition"] = 'attachment; filename="signed.eml"'
    signed.set_payload([EmailMessage(), EmailMessage()])

    state = mime_policy._remove_attachments(  # ruff: ignore[private-member-access]
        signed,
        mime_references._collect_references(  # ruff: ignore[private-member-access]
            signed
        ),
    )

    assert not state.removed
    assert state.protected_types == {"multipart/signed"}
    assert signed.get_content_type() == "multipart/signed"


def test_root_html_attachment_cannot_retain_itself_via_its_own_cid() -> None:
    """Remove an explicit root attachment even when its body cites its own ID."""
    message = EmailMessage()
    message.set_content('<img src="cid:self@example.test">', subtype="html")
    message["Content-ID"] = "<self@example.test>"
    message["Content-Disposition"] = 'attachment; filename="self.html"'
    references = mime_references._collect_references(  # ruff: ignore[private-member-access]
        message
    )

    state = mime_policy._remove_attachments(  # ruff: ignore[private-member-access]
        message,
        references,
    )

    assert state.removed == [RemovedPart((), "text/html", "self.html", "attachment")]
    assert message.get_content_type() == "text/plain"
