# ruff: file-ignore[private-member-access]
"""Preserve only namespace-bound resources rendered by Outlook VML."""

from __future__ import annotations

from email import policy
from email.message import EmailMessage
from email.parser import BytesParser
from typing import TYPE_CHECKING, Literal

import pytest

from eml_attachment_remover import html_references, mime_references, process_file
from eml_attachment_remover.models import ReferenceIndex

if TYPE_CHECKING:
    from pathlib import Path

type ReferenceKind = Literal["cid", "location"]


def _reference_value(kind: ReferenceKind) -> str:
    """Return a CID or relative-location resource reference.

    Returns:
        The synthetic URI for the requested representation.

    """
    return "cid:secret@example.test" if kind == "cid" else "public-secret.bin"


def _resource(kind: ReferenceKind) -> EmailMessage:
    """Return an explicit attachment with matching identity metadata.

    Returns:
        A synthetic binary resource identified by CID or location.

    """
    resource = EmailMessage()
    resource.set_content(
        b"PUBLIC SECRET PAYLOAD", maintype="application", subtype="octet-stream"
    )
    resource.add_header("Content-Disposition", "attachment", filename="secret.bin")
    header = "Content-ID" if kind == "cid" else "Content-Location"
    value = "<secret@example.test>" if kind == "cid" else "public-secret.bin"
    resource[header] = value
    return resource


def _message(markup: str, kind: ReferenceKind) -> EmailMessage:
    """Return a related plain/HTML body with one explicit resource.

    Returns:
        The configured public MIME tree.

    """
    message = EmailMessage()
    message.set_content("PUBLIC PLAIN BODY")
    message.add_alternative(markup, subtype="html")
    related = EmailMessage()
    related.make_related()
    related.attach(message)
    related.attach(_resource(kind))
    return related


def _parse(path: Path) -> EmailMessage:
    """Parse one generated public EML output.

    Returns:
        The parsed output message.

    """
    return BytesParser(policy=policy.default).parsebytes(path.read_bytes())


def _expected(kind: ReferenceKind) -> ReferenceIndex:
    """Return the exact expected identity index for one representation.

    Returns:
        A one-CID or one-location reference index.

    """
    if kind == "cid":
        return ReferenceIndex(frozenset({"secret@example.test"}), frozenset())
    return ReferenceIndex(frozenset(), frozenset({"public-secret.bin"}))


@pytest.mark.parametrize("tag", ["v:background", "v:fill", "v:imagedata"])
@pytest.mark.parametrize("kind", ["cid", "location"])
def test_outlook_vml_source_is_a_rendering_reference(
    tag: str,
    kind: ReferenceKind,
) -> None:
    """Recognize only supported namespace-bound VML ``src`` elements."""
    reference = _reference_value(kind)

    references = mime_references._references_from_html(
        f'<html xmlns:v="urn:schemas-microsoft-com:vml"><{tag} src="{reference}">',
    )

    assert references == _expected(kind)


@pytest.mark.parametrize("kind", ["cid", "location"])
def test_outlook_vml_resource_is_audited_and_discarded_end_to_end(
    tmp_path: Path,
    kind: ReferenceKind,
) -> None:
    """Audit and discard a resource loaded by a VML image-data element."""
    reference = _reference_value(kind)
    message = _message(
        '<html xmlns:v="urn:schemas-microsoft-com:vml">'
        f'<v:imagedata src="{reference}"></v:imagedata></html>',
        kind,
    )
    source = tmp_path / "source.eml"
    destination = tmp_path / "output.eml"
    source.write_bytes(message.as_bytes(policy=policy.SMTP))

    result = process_file(source, destination, force=False, dry_run=False)
    output = _parse(destination)

    assert not result.removed_attachments
    assert [part.filename for part in result.discarded_body_resources] == ["secret.bin"]
    assert not any(part.get_filename() == "secret.bin" for part in output.walk())


def test_vml_like_decoy_elements_and_attributes_are_not_references() -> None:
    """Reject namespace lookalikes, non-rendering tags, and wrong attributes."""
    references = mime_references._references_from_html(
        '<v:shape src="cid:first@example.test">'
        '<x:imagedata src="cid:second@example.test">'
        '<v:imagedata href="cid:third@example.test">'
        '<v:imagedata src="cid:unbound@example.test">'
        '<section xmlns:v="urn:example:test">'
        '<v:imagedata src="cid:wrong@example.test"></section>',
    )

    assert references == ReferenceIndex(frozenset(), frozenset())


def test_vml_namespace_binding_is_lexically_scoped() -> None:
    """Honor valid VML declarations while rejecting a nested wrong rebinding."""
    references = mime_references._references_from_html(
        '<html xmlns:v="URN:SCHEMAS-MICROSOFT-COM:VML">'
        '<v:imagedata src="cid:first@example.test" />'
        '<section xmlns:v="urn:example:test">'
        '<v:imagedata src="cid:wrong@example.test" /></section>'
        '<v:fill src="cid:second@example.test" /></html>',
    )

    assert references == ReferenceIndex(
        frozenset({"first@example.test", "second@example.test"}),
        frozenset(),
    )


@pytest.mark.parametrize("condition", ["mso", "mso 16", "gte mso 9"])
@pytest.mark.parametrize("kind", ["cid", "location"])
def test_hidden_mso_conditional_vml_is_a_rendering_reference(
    condition: str,
    kind: ReferenceKind,
) -> None:
    """Parse common positive downlevel-hidden Outlook conditional markup."""
    reference = _reference_value(kind)
    references = mime_references._references_from_html(
        '<html xmlns:v="urn:schemas-microsoft-com:vml">'
        f'<!--[if {condition}]><v:fill src="{reference}" /><![endif]--></html>',
    )

    assert references == _expected(kind)


def test_outer_html_base_precedes_a_conditional_fragment_base() -> None:
    """Keep first-document-base semantics across a parsed MSO fragment."""
    references = mime_references._references_from_html(
        '<html xmlns:v="urn:schemas-microsoft-com:vml">'
        '<base href="https://public.example.test/outer/">'
        '<!--[if mso]><base href="https://public.example.test/decoy/">'
        '<v:fill src="image.png" /><![endif]--></html>',
    )

    assert references.locations == frozenset({
        "https://public.example.test/outer/image.png",
    })


@pytest.mark.parametrize("kind", ["cid", "location"])
def test_hidden_mso_conditional_resource_is_discarded_end_to_end(
    tmp_path: Path,
    kind: ReferenceKind,
) -> None:
    """Audit and discard a resource loaded only by conditional VML."""
    reference = _reference_value(kind)
    message = _message(
        '<html xmlns:v="urn:schemas-microsoft-com:vml">'
        "<!--[if gte mso 9]><v:rect>"
        f'<v:fill src="{reference}" /></v:rect><![endif]--></html>',
        kind,
    )
    source = tmp_path / "source.eml"
    destination = tmp_path / "output.eml"
    source.write_bytes(message.as_bytes(policy=policy.SMTP))

    result = process_file(source, destination, force=False, dry_run=False)
    output = _parse(destination)

    assert not result.removed_attachments
    assert [part.filename for part in result.discarded_body_resources] == ["secret.bin"]
    assert not any(part.get_filename() == "secret.bin" for part in output.walk())


@pytest.mark.parametrize(
    "comment",
    [
        'ordinary <v:fill src="cid:decoy@example.test">',
        '[if mso]><v:fill src="cid:decoy@example.test">',
        '[if !mso]><v:fill src="cid:decoy@example.test"><![endif]',
        '[if !IE]><v:fill src="cid:decoy@example.test"><![endif]',
        '[if IE]><v:fill src="cid:decoy@example.test"><![endif]',
        '[if (gte mso 9)|(IE)]><v:fill src="cid:decoy@example.test"><![endif]',
    ],
)
def test_nonpositive_or_malformed_conditional_comments_are_decoys(
    comment: str,
) -> None:
    """Ignore comment forms outside the narrow positive MSO grammar."""
    references = mime_references._references_from_html(
        f'<html xmlns:v="urn:schemas-microsoft-com:vml"><!--{comment}--></html>',
    )

    assert references == ReferenceIndex(frozenset(), frozenset())


def test_conditional_comments_inside_template_and_style_are_decoys() -> None:
    """Do not promote inert template or CSS-comment content into body references."""
    hidden = '[if mso]><v:fill src="cid:decoy@example.test" /><![endif]'
    references = mime_references._references_from_html(
        '<html xmlns:v="urn:schemas-microsoft-com:vml">'
        f"<template><!--{hidden}--></template>"
        f"<style><!--{hidden}--></style></html>",
    )

    assert references == ReferenceIndex(frozenset(), frozenset())


def test_oversized_mso_conditional_comment_is_a_decoy() -> None:
    """Bound conditional-fragment work before parsing any nested markup."""
    padding = "x" * html_references.MAX_MSO_CONDITIONAL_COMMENT_LENGTH
    references = mime_references._references_from_html(
        '<html xmlns:v="urn:schemas-microsoft-com:vml">'
        '<!--[if mso]><v:fill src="cid:decoy@example.test" />'
        f"{padding}<![endif]--></html>",
    )

    assert references == ReferenceIndex(frozenset(), frozenset())
