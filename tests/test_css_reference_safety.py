# ruff: file-ignore[private-member-access]
"""Protect resources referenced through valid CSS representations."""

from __future__ import annotations

import html
import string
from email.message import EmailMessage
from typing import Final

import pytest
from hypothesis import given
from hypothesis import strategies as st

from eml_attachment_remover import mime_policy, mime_references
from eml_attachment_remover.models import KeepReason

CSS_NAME: Final[st.SearchStrategy[str]] = st.text(
    alphabet=string.ascii_letters + string.digits + "-_",
    min_size=1,
    max_size=30,
)


def _attachment(location: str, filename: str) -> EmailMessage:
    """Return one explicit image attachment with a resource label.

    Returns:
        The configured public synthetic part.

    """
    part = EmailMessage()
    part.set_content(b"public image", maintype="image", subtype="png")
    part.add_header("Content-Disposition", "attachment", filename=filename)
    part["Content-Location"] = location
    return part


@pytest.mark.parametrize(
    ("css", "location"),
    [
        (r".public{background:u\72l(escaped.png)}", "escaped.png"),
        (".public{background:url(/**/commented.png/**/)}", "commented.png"),
        (".public{background:url(literal&amp;name.png)}", "literal&amp;name.png"),
        (r'@import "escaped\2e css";', "escaped.css"),
        (r'@\69mport "escaped-import.css";', "escaped-import.css"),
        (r'@i\6dport "escaped-m.css";', "escaped-m.css"),
        (r'@\69mport/**/"comment-gap.css";', "comment-gap.css"),
        (r'@\49mport "uppercase-import.css";', "uppercase-import.css"),
        (r".public{background:\55\52\4c(uppercase-url.png)}", "uppercase-url.png"),
        (r'@/**/i/**/mport "commented-import.css";', "commented-import.css"),
        (r".public{background:u/**/r/**/l(commented-url.png)}", "commented-url.png"),
    ],
)
def test_css_reference_retains_exact_attachment(css: str, location: str) -> None:
    """Retain the exact CSS resource and remove its normalized-distinct twin."""
    message = EmailMessage()
    message.set_content(f"<style>{css}</style>", subtype="html")
    message.make_mixed()
    message.attach(_attachment(location, "referenced.png"))
    message.attach(_attachment(f"near-{location}", "near.png"))

    references = mime_references._collect_references(message)
    state = mime_policy._remove_attachments(message, references)

    assert [part.filename for part in state.removed] == ["near.png"]
    assert [part.reason for part in state.preserved_file_parts] == [
        KeepReason.BODY_LOCATION_REFERENCE
    ]


@pytest.mark.parametrize(
    ("body", "actual", "spurious"),
    [
        (
            '<div style="background:url(a&amp;#0)"></div>',
            "a&#0",
            "a�",
        ),
        (
            "<style>.public{background:url(a&amp;#0)}</style>",
            "a&amp;#0",
            "a&#0",
        ),
    ],
)
def test_css_html_decoding_boundary_retains_only_actual_location(
    body: str,
    actual: str,
    spurious: str,
) -> None:
    """Decode entities once according to their source context."""
    message = EmailMessage()
    message.set_content(body, subtype="html")
    message.make_mixed()
    message.attach(_attachment(actual, "actual.png"))
    message.attach(_attachment(spurious, "spurious.png"))

    references = mime_references._collect_references(message)
    state = mime_policy._remove_attachments(message, references)

    assert [part.filename for part in state.removed] == ["spurious.png"]
    assert [part.filename for part in state.preserved_file_parts] == ["actual.png"]


@pytest.mark.parametrize(
    "css",
    [
        '.public{content:"url(decoy.png)"}',
        ".public{content:'url(decoy.png)'}",
        '.public{content:"@import \\"decoy.png\\""}',
        '.public{content:"escaped \\" url(decoy.png)"}',
    ],
)
def test_css_string_content_cannot_retain_attachment(css: str) -> None:
    """Ignore resource-shaped text nested inside ordinary CSS strings."""
    message = EmailMessage()
    message.set_content(f"<style>{css}</style>", subtype="html")
    message.make_mixed()
    message.attach(_attachment("decoy.png", "decoy.png"))

    references = mime_references._collect_references(message)
    state = mime_policy._remove_attachments(message, references)

    assert [part.filename for part in state.removed] == ["decoy.png"]


def test_whitespace_before_url_parenthesis_cannot_retain_attachment() -> None:
    """Require the function parenthesis to be part of the CSS function token."""
    assert (
        mime_references._references_from_html(
            "<style>.public{background:url (decoy.png)}</style>",
        ).locations
        == frozenset()
    )


@pytest.mark.parametrize(
    "css",
    [
        r'@\69mports "decoy.png";',
        r'@\69mportant "decoy.png";',
        r'@\49mports "decoy.png";',
    ],
)
def test_escaped_import_near_syntax_cannot_retain_attachment(css: str) -> None:
    """Require the complete escaped import identifier and grammar boundary."""
    message = EmailMessage()
    message.set_content(f"<style>{css}</style>", subtype="html")
    message.make_mixed()
    message.attach(_attachment("decoy.png", "decoy.png"))

    state = mime_policy._remove_attachments(
        message,
        mime_references._collect_references(message),
    )

    assert [part.filename for part in state.removed] == ["decoy.png"]


@given(
    context=st.sampled_from(("attribute", "element")),
    entity=st.sampled_from(("&amp;#0", "&amp;amp;")),
)
def test_css_context_decodes_generated_entity_text_exactly_once(
    context: str,
    entity: str,
) -> None:
    """Distinguish raw style text from HTML-decoded style attributes."""
    represented = f"public{entity}image.png"
    actual = html.unescape(represented) if context == "attribute" else represented
    spurious = html.unescape(actual if context == "attribute" else represented)
    body = (
        f'<div style="background:url({represented})"></div>'
        if context == "attribute"
        else f"<style>.public{{background:url({represented})}}</style>"
    )
    message = EmailMessage()
    message.set_content(body, subtype="html")
    message.make_mixed()
    message.attach(_attachment(actual, "actual.png"))
    message.attach(_attachment(spurious, "spurious.png"))

    state = mime_policy._remove_attachments(
        message,
        mime_references._collect_references(message),
    )

    assert [part.filename for part in state.removed] == ["spurious.png"]


@given(name=CSS_NAME, quote=st.sampled_from(('"', "'")))
def test_generated_css_string_decoy_never_authorizes_retention(
    name: str,
    quote: str,
) -> None:
    """Ignore generated URL-shaped text inside either CSS string form."""
    location = f"{name}.png"
    body = f"<style>.public{{content:{quote}url({location}){quote}}}</style>"
    message = EmailMessage()
    message.set_content(body, subtype="html")
    message.make_mixed()
    message.attach(_attachment(location, "decoy.png"))

    state = mime_policy._remove_attachments(
        message,
        mime_references._collect_references(message),
    )

    assert [part.filename for part in state.removed] == ["decoy.png"]
