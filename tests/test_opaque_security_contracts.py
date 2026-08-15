"""Require nested opaque cryptographic MIME entities to remain intact."""

from __future__ import annotations

import hashlib
import tempfile
from email import policy
from email.message import EmailMessage
from pathlib import Path
from typing import Final

import pytest

from eml_attachment_remover import mime_policy, mime_references, process_file
from eml_attachment_remover.models import KeepReason, PartContext, ReferenceIndex
from tests.test_support import decoded_hash, parse

OPAQUE_TYPES: Final = (
    "application/pkcs7-mime",
    "application/x-pkcs7-mime",
    "application/pgp-encrypted",
)
DISPOSITIONS: Final = (None, "inline", "attachment")
OPAQUE_PAYLOAD: Final = b"PUBLIC OPAQUE CRYPTOGRAPHIC PAYLOAD\x00\xff"


def _opaque_part(content_type: str, disposition: str | None) -> EmailMessage:
    """Build one nested opaque entity.

    Returns:
        A cryptographic MIME entity with the selected disposition.

    """
    maintype, subtype = content_type.split("/", maxsplit=1)
    part = EmailMessage()
    part.set_content(
        OPAQUE_PAYLOAD,
        maintype=maintype,
        subtype=subtype,
        cte="base64",
    )
    if disposition is not None:
        part.add_header(
            "Content-Disposition",
            disposition,
            filename="protected.p7m",
        )
    return part


def _context() -> PartContext:
    """Return a nested public traversal context.

    Returns:
        Context with no body reference affecting the security decision.

    """
    return PartContext(
        path=(1,),
        parent_type="multipart/mixed",
        under_related=False,
        references=ReferenceIndex(frozenset(), frozenset()),
    )


@pytest.mark.parametrize("content_type", OPAQUE_TYPES)
@pytest.mark.parametrize("disposition", DISPOSITIONS)
def test_security_type_precedes_every_disposition(
    content_type: str,
    disposition: str | None,
) -> None:
    """Classify every opaque entity as protected, including attachments."""
    part = _opaque_part(content_type, disposition)

    reason = mime_policy._metadata_keep_reason(  # ruff: ignore[private-member-access]
        part,
        _context(),
        None,
        None,
    )
    removable = mime_policy._should_remove(  # ruff: ignore[private-member-access]
        part,
        _context(),
    )

    assert reason is KeepReason.SECURITY_ENTITY
    assert not removable


@pytest.mark.parametrize("content_type", OPAQUE_TYPES)
@pytest.mark.parametrize("disposition", DISPOSITIONS)
def test_nested_opaque_entity_is_preserved_end_to_end(
    content_type: str,
    disposition: str | None,
) -> None:
    """Retain exact opaque bytes while removing an ordinary sibling attachment."""
    message = EmailMessage()
    message["Subject"] = "Public nested opaque fixture"
    message.set_content("Public body")
    message.make_mixed()
    message.attach(_opaque_part(content_type, disposition))
    message.add_attachment(
        b"PUBLIC ORDINARY ATTACHMENT",
        maintype="application",
        subtype="octet-stream",
        filename="remove.bin",
    )
    raw = message.as_bytes(policy=policy.SMTP)
    with tempfile.TemporaryDirectory() as directory:
        source = Path(directory) / "source.eml"
        destination = Path(directory) / "output.eml"
        source.write_bytes(raw)

        result = process_file(source, destination, force=False, dry_run=False)
        output = parse(destination)

        protected = next(
            part for part in output.walk() if part.get_content_type() == content_type
        )
        assert source.read_bytes() == raw
        assert [part.filename for part in result.removed] == ["remove.bin"]
        assert result.warnings == (
            f"protected MIME entity left intact: {content_type}",
        )
        assert decoded_hash(protected) == hashlib.sha256(OPAQUE_PAYLOAD).hexdigest()
        if disposition is None:
            assert not result.preserved_file_parts
        else:
            assert [part.reason for part in result.preserved_file_parts] == [
                KeepReason.SECURITY_ENTITY
            ]


@pytest.mark.parametrize("content_type", OPAQUE_TYPES)
def test_non_file_like_opaque_records_protection(content_type: str) -> None:
    """Populate warnings even when opaque content has no file-like metadata."""
    message = EmailMessage()
    message.set_content("Public body")
    message.make_mixed()
    message.attach(_opaque_part(content_type, None))

    state = mime_policy._remove_attachments(  # ruff: ignore[private-member-access]
        message,
        mime_references._collect_references(  # ruff: ignore[private-member-access]
            message
        ),
    )

    assert state.protected_types == {content_type}
    assert not state.preserved_file_parts
    assert not state.removed
