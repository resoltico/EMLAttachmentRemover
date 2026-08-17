# ruff: file-ignore[private-member-access]
"""V2 contracts for resource-reference parsing and audit-scope isolation."""

from __future__ import annotations

from email.message import EmailMessage

from eml_attachment_remover import css_references, mime_references
from eml_attachment_remover.models import ReferenceIndex


def _html(source: str) -> EmailMessage:
    """Return one decoded HTML entity for a synthetic audit tree.

    Returns:
        A standalone HTML MIME leaf.

    """
    part = EmailMessage()
    part.set_content(source, subtype="html")
    return part


def test_css_audit_extracts_quoted_import_and_exhausts_plain_source() -> None:
    """Index an imported resource and terminate exactly after ordinary CSS text."""
    assert css_references._css_reference_values('@import "theme.css";') == (
        "theme.css",
    )
    assert css_references._css_reference_values("ordinary") == ()


def test_html_audit_decodes_bracketed_cid_with_unknown_charset() -> None:
    """Recover one encoded CID while ignoring non-fetching URI representations."""
    part = EmailMessage()
    part["Content-Type"] = 'text/html; charset="x-unknown-public-charset"'
    part.set_payload(
        '<img src=" CID:%3Chero@example.test%3E ">'
        '<img src=""><img src="#section">'
        '<img src="data:image/png;base64,AAAA">',
    )

    assert mime_references._collect_references(part) == ReferenceIndex(
        frozenset({"hero@example.test"}),
        frozenset(),
    )


def test_reference_audit_isolates_non_body_mime_subtrees() -> None:
    """Scan eligible HTML bodies without crossing attachment or message scopes."""
    ordinary = _html('<img src="ordinary.png">')

    inline_named = _html('<img src="inline.png">')
    inline_named.add_header(
        "Content-Disposition",
        "inline",
        filename="inline.html",
    )

    non_body_named = _html('<img src="named-decoy.png">')
    non_body_named.set_param("name", "named.html", header="Content-Type")

    attached = _html('<img src="attachment-decoy.png">')
    attached.add_header(
        "Content-Disposition",
        "attachment",
        filename="attached.html",
    )

    encapsulated = _html('<img src="message-decoy.png">')
    message_part = EmailMessage()
    message_part.set_type("message/rfc822")
    message_part.set_payload([encapsulated])

    related = EmailMessage()
    related.make_related()
    related.attach(_html('<img src="related-decoy.png">'))

    named_alternative = _html('<img src="alternative.png">')
    named_alternative.set_param("name", "alternative.html", header="Content-Type")
    alternative = EmailMessage()
    alternative.make_alternative()
    alternative.attach(named_alternative)

    message = EmailMessage()
    message.make_mixed()
    for part in (
        ordinary,
        inline_named,
        non_body_named,
        attached,
        message_part,
        related,
        alternative,
    ):
        message.attach(part)

    assert mime_references._collect_references(message) == ReferenceIndex(
        frozenset(),
        frozenset({"ordinary.png", "inline.png", "alternative.png"}),
    )


def test_reference_audit_fails_closed_for_non_message_multipart_payload() -> None:
    """Return an empty index when a synthetic multipart tree is not traversable."""
    malformed = EmailMessage()
    malformed.set_payload(["not-an-email-entity"])

    assert mime_references._collect_references(malformed) == ReferenceIndex(
        frozenset(),
        frozenset(),
    )
