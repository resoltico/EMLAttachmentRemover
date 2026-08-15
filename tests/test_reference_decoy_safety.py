# ruff: file-ignore[private-member-access]
"""Reject non-resource HTML text as an attachment-retention authority."""

from __future__ import annotations

from email.message import EmailMessage

import pytest

from eml_attachment_remover import mime_policy, mime_references


def _attachment(*, cid: str | None = None, location: str | None = None) -> EmailMessage:
    """Return an explicitly removable attachment with selected labels.

    Returns:
        The configured public synthetic MIME entity.

    """
    part = EmailMessage()
    part.set_content(
        b"public attachment", maintype="application", subtype="octet-stream"
    )
    part.add_header("Content-Disposition", "attachment", filename="decoy.bin")
    if cid is not None:
        part["Content-ID"] = f"<{cid}>"
    if location is not None:
        part["Content-Location"] = location
    return part


def _removed_from(body: str, attachment: EmailMessage) -> list[str | None]:
    """Return filenames removed from one synthetic HTML message.

    Returns:
        The removal report's filename list.

    """
    message = EmailMessage()
    message.set_content(body, subtype="html")
    message.make_mixed()
    message.attach(attachment)
    references = mime_references._collect_references(message)
    state = mime_policy._remove_attachments(message, references)
    return [part.filename for part in state.removed]


def test_cid_in_prose_or_css_comment_cannot_retain_attachment() -> None:
    """Ignore CID-shaped prose and commented-out CSS syntax."""
    body = (
        "<p>Write cid:decoy@example.test in your reply.</p>"
        "<style>/* url(cid:decoy@example.test) */</style>"
    )

    assert _removed_from(
        body,
        _attachment(cid="decoy@example.test"),
    ) == ["decoy.bin"]


def test_uri_shaped_prose_cannot_retain_attachment() -> None:
    """Ignore attribute and CSS syntax written as ordinary visible prose."""
    assert _removed_from(
        "<p>Type src=decoy.png or url(decoy.png) in the editor.</p>",
        _attachment(location="decoy.png"),
    ) == ["decoy.bin"]


def test_base_href_is_not_itself_a_resource_reference() -> None:
    """Use base href only for resolution, never as a fetched resource."""
    assert _removed_from(
        '<base href="https://public.example/assets/"><p>Public body</p>',
        _attachment(location="https://public.example/assets/"),
    ) == ["decoy.bin"]


def test_inert_template_content_cannot_set_base_or_retain_resources() -> None:
    """Ignore resource syntax in template contents until instantiated by script."""
    body = (
        '<template><base href="https://decoy.example/">'
        '<img src="decoy.png"><style>body{background:url(decoy.png)}</style>'
        "</template>"
        '<base href="https://public.example/"><img src="real.png">'
    )

    assert _removed_from(
        body,
        _attachment(location="https://decoy.example/decoy.png"),
    ) == ["decoy.bin"]
    references = mime_references._references_from_html(body)
    assert references.locations == frozenset({"https://public.example/real.png"})


def test_navigation_href_cannot_retain_downloadable_attachment() -> None:
    """Ignore anchor navigation while accepting a rendering link resource."""
    assert _removed_from(
        '<a href="report.pdf">Download</a>',
        _attachment(location="report.pdf"),
    ) == ["decoy.bin"]


def test_anchor_cid_cannot_retain_attachment() -> None:
    """Ignore a CID used as anchor navigation rather than rendered content."""
    assert _removed_from(
        '<a href="cid:decoy@example.test">Open</a>',
        _attachment(cid="decoy@example.test"),
    ) == ["decoy.bin"]


def test_link_and_svg_rendering_hrefs_are_references() -> None:
    """Recognize stylesheet and SVG image/use resource-bearing href forms."""
    references = mime_references._references_from_html(
        '<link rel="stylesheet" href="theme.css">'
        '<svg><image href="hero.png"><use xlink:href="sprite.svg#icon"></svg>',
    )

    assert references.locations == frozenset({
        "hero.png",
        "sprite.svg",
        "sprite.svg#icon",
        "theme.css",
    })


@pytest.mark.parametrize(
    ("tag", "attribute"),
    [
        ("a", "href"),
        ("area", "href"),
        ("div", "src"),
        ("img", "href"),
        ("input", "src"),
        ("link", "href"),
        ("script", "href"),
        ("span", "data"),
        ("table", "poster"),
        ("video", "srcset"),
    ],
)
def test_non_rendering_tag_attribute_pair_is_ignored(
    tag: str,
    attribute: str,
) -> None:
    """Reject URI-shaped values on elements where the attribute does not render."""
    assert _removed_from(
        f'<{tag} {attribute}="decoy.png"></{tag}>',
        _attachment(location="decoy.png"),
    ) == ["decoy.bin"]


def test_later_duplicate_rendering_attribute_cannot_retain_attachment() -> None:
    """Use the first duplicate attribute exactly as an HTML parser does."""
    assert _removed_from(
        '<img src src="decoy.png">',
        _attachment(location="decoy.png"),
    ) == ["decoy.bin"]


def test_invalid_srcset_descriptor_cannot_retain_attachment() -> None:
    """Discard one invalid candidate while retaining a valid sibling candidate."""
    body = '<img srcset="decoy.png bogus, actual.png 2x">'

    assert _removed_from(
        body,
        _attachment(location="decoy.png"),
    ) == ["decoy.bin"]
    assert mime_references._references_from_html(body).locations == frozenset({
        "actual.png",
    })


def test_image_input_src_is_a_rendering_reference() -> None:
    """Recognize input image sources only when their type requests an image."""
    references = mime_references._references_from_html(
        '<input type="image" src="button.png">',
    )

    assert references.locations == frozenset({"button.png"})
