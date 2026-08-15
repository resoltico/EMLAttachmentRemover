# ruff: file-ignore[private-member-access]
"""Keep body-reference extraction aligned with MIME removal eligibility."""

from __future__ import annotations

from email import policy
from email.message import EmailMessage
from email.parser import BytesParser
from typing import TYPE_CHECKING, Literal

import pytest

from eml_attachment_remover import (
    mime_body,
    mime_policy,
    mime_references,
    process_file,
)
from eml_attachment_remover.models import ReferenceIndex

if TYPE_CHECKING:
    from pathlib import Path

type ReferenceKind = Literal["cid", "location"]


def _reference_value(kind: ReferenceKind) -> str:
    """Return one synthetic URI for the requested reference form.

    Returns:
        A CID URI or relative location URI.

    """
    return "cid:secret@example.test" if kind == "cid" else "public-secret.bin"


def _resource(kind: ReferenceKind) -> EmailMessage:
    """Return one explicit attachment identified by the requested metadata.

    Returns:
        A synthetic binary attachment with CID or location metadata.

    """
    resource = EmailMessage()
    resource.set_content(
        b"PUBLIC SECRET PAYLOAD", maintype="application", subtype="octet-stream"
    )
    resource.add_header("Content-Disposition", "attachment", filename="secret.bin")
    if kind == "cid":
        resource["Content-ID"] = "<secret@example.test>"
    else:
        resource["Content-Location"] = "public-secret.bin"
    return resource


def _named_html(markup: str) -> EmailMessage:
    """Return one HTML entity with removable filename metadata.

    Returns:
        A filename-bearing HTML MIME part without a disposition.

    """
    html = EmailMessage()
    html.set_content(markup, subtype="html")
    html.set_param("name", "decoy.html", header="Content-Type")
    return html


def _mixed_with_html(html: EmailMessage, resource: EmailMessage) -> EmailMessage:
    """Return a mixed message with a real body, HTML entity, and resource.

    Returns:
        The configured synthetic MIME tree.

    """
    message = EmailMessage()
    message.set_content("Public real body")
    message.make_mixed()
    message.attach(html)
    message.attach(resource)
    return message


def _alternative_with_resource(kind: ReferenceKind) -> EmailMessage:
    """Return a mixed message whose named HTML alternative cites a resource.

    Returns:
        A legitimate multipart/alternative body and its explicit resource.

    """
    alternative = EmailMessage()
    alternative.make_alternative()
    plain = EmailMessage()
    plain.set_content("Public plain alternative")
    alternative.attach(plain)
    alternative.attach(_named_html(f'<img src="{_reference_value(kind)}">'))
    root = EmailMessage()
    root.make_mixed()
    root.attach(alternative)
    root.attach(_resource(kind))
    return root


def _parse(path: Path) -> EmailMessage:
    """Parse one generated public EML output.

    Returns:
        The parsed output message.

    """
    return BytesParser(policy=policy.default).parsebytes(path.read_bytes())


def test_body_text_eligibility_rejects_nontext_and_explicit_attachments() -> None:
    """Reject entities that cannot be a retained message-body text part."""
    binary = EmailMessage()
    binary.set_content(b"PUBLIC", maintype="application", subtype="octet-stream")
    attached_html = _named_html("<p>Public attachment</p>")
    attached_html["Content-Disposition"] = "attachment"

    assert not mime_body._is_retained_body_text(
        binary,
        "multipart/mixed",
        under_related=False,
        is_root=False,
    )
    assert not mime_body._is_retained_body_text(
        attached_html,
        "multipart/mixed",
        under_related=False,
        is_root=False,
    )


@pytest.mark.parametrize("kind", ["cid", "location"])
def test_removable_named_html_cannot_protect_an_attachment(kind: ReferenceKind) -> None:
    """Exclude references contributed by an HTML part policy will remove."""
    message = _mixed_with_html(
        _named_html(f'<img src="{_reference_value(kind)}">'),
        _resource(kind),
    )

    references = mime_references._collect_references(message)
    state = mime_policy._remove_attachments(message, references)

    assert references == ReferenceIndex(frozenset(), frozenset())
    assert [part.filename for part in state.removed] == ["decoy.html", "secret.bin"]
    assert not state.preserved_file_parts


@pytest.mark.parametrize("kind", ["cid", "location"])
def test_unreferenced_named_html_resource_cannot_protect_an_attachment(
    kind: ReferenceKind,
) -> None:
    """Do not mistake a metadata-retained HTML resource for the message body."""
    decoy = _named_html(f'<img src="{_reference_value(kind)}">')
    if kind == "cid":
        decoy["Content-ID"] = "<unreferenced-html@example.test>"
    else:
        decoy["Content-Location"] = "unreferenced.html"
    message = _mixed_with_html(decoy, _resource(kind))

    references = mime_references._collect_references(message)
    state = mime_policy._remove_attachments(message, references)

    assert references == ReferenceIndex(frozenset(), frozenset())
    assert [part.filename for part in state.removed] == ["secret.bin"]
    assert [part.filename for part in state.preserved_file_parts] == ["decoy.html"]


@pytest.mark.parametrize("kind", ["cid", "location"])
def test_removable_named_html_isolated_end_to_end(
    tmp_path: Path,
    kind: ReferenceKind,
) -> None:
    """Publish no file protected only by a removable HTML decoy reference."""
    message = _mixed_with_html(
        _named_html(f'<img src="{_reference_value(kind)}">'),
        _resource(kind),
    )
    source = tmp_path / "source.eml"
    destination = tmp_path / "output.eml"
    source.write_bytes(message.as_bytes(policy=policy.SMTP))

    result = process_file(source, destination, force=False, dry_run=False)
    output = _parse(destination)

    assert [part.filename for part in result.removed] == ["decoy.html", "secret.bin"]
    assert all(part.get_filename() is None for part in output.walk())
    assert b"PUBLIC SECRET PAYLOAD" not in destination.read_bytes()


@pytest.mark.parametrize("kind", ["cid", "location"])
def test_named_html_alternative_remains_a_body_reference_source(
    kind: ReferenceKind,
) -> None:
    """Retain resources cited by filename-bearing HTML body alternatives."""
    message = _alternative_with_resource(kind)

    references = mime_references._collect_references(message)
    state = mime_policy._remove_attachments(message, references)

    assert not state.removed
    assert [part.filename for part in state.preserved_file_parts] == ["secret.bin"]
