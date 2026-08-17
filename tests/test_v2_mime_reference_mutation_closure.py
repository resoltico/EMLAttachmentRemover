# ruff: file-ignore[private-member-access]
"""Mutation-strength contracts for MIME URI bases and reference-audit scopes."""

from __future__ import annotations

from email.message import EmailMessage

from eml_attachment_remover import mime_locations, mime_references
from eml_attachment_remover.models import ReferenceIndex


def _html(source: str) -> EmailMessage:
    """Return one HTML MIME leaf.

    Returns:
        A standalone decoded HTML entity.

    """
    part = EmailMessage()
    part.set_content(source, subtype="html")
    return part


def test_content_location_establishes_a_mime_subtree_base() -> None:
    """Use an absolute Content-Location when legacy Content-Base is absent."""
    part = EmailMessage()
    part["Content-Location"] = "https://public.example/mail/body.eml"

    assert (
        mime_locations._content_location_base(part, None)
        == "https://public.example/mail/body.eml"
    )


def test_html_location_and_legacy_base_join_or_fail_closed_exactly() -> None:
    """Resolve a valid location and retain a relative base after an invalid join."""
    located = EmailMessage()
    located["Content-Location"] = "body/index.html"
    based = EmailMessage()
    based["Content-Base"] = "assets/"

    assert (
        mime_locations._html_mime_base(
            located,
            "https://public.example/root/",
        )
        == "https://public.example/root/body/index.html"
    )
    assert mime_locations._html_mime_base(based, "http://[invalid") == "assets/"


def test_absolute_uri_uses_the_first_colon_and_requires_a_separator() -> None:
    """Accept an authority port but reject an unqualified relative token."""
    assert mime_locations._is_absolute_uri("http://public.example:8080/image.png")
    assert not mime_locations._is_absolute_uri("relative")


def test_dns_authority_removes_only_its_exact_default_port() -> None:
    """Fold a DNS host and omit the scheme's default non-IPv6 port."""
    assert (
        mime_locations._canonical_location("HTTP://Public.Example:80/image.png")
        == "http://public.example/image.png"
    )


def test_absolute_and_leading_dot_paths_use_rfc_transitions() -> None:
    """Preserve absolute-root semantics and consume only one leading segment."""
    assert mime_locations._normalize_path("/../image.png") == "/image.png"
    assert mime_locations._remove_dot_segments("./image.png") == "image.png"
    assert mime_locations._remove_dot_segments("../a/b.png") == "a/b.png"


def test_inherited_base_flows_through_container_and_html_location() -> None:
    """Resolve a nested HTML resource through every MIME-base handoff."""
    message = EmailMessage()
    message.make_mixed()
    child = _html('<img src="image.png">')
    child["Content-Location"] = "body/index.html"
    message.attach(child)

    assert mime_references._collect_references(
        message,
        inherited_base="https://public.example/root/",
    ) == ReferenceIndex(
        frozenset(),
        frozenset({"https://public.example/root/body/image.png"}),
    )


def test_container_content_location_flows_to_its_html_child() -> None:
    """Propagate an absolute container location to a relative body resource."""
    message = EmailMessage()
    message.make_mixed()
    message["Content-Location"] = "https://public.example/mail/message.eml"
    message.attach(_html('<img src="image.png">'))

    assert mime_references._collect_references(message).locations == frozenset({
        "https://public.example/mail/image.png",
    })


def test_named_scope_root_remains_an_html_reference_source() -> None:
    """Treat a named audit-scope root as body HTML rather than a nested file."""
    root = _html('<img src="named-root.png">')
    root.set_param("name", "body.html", header="Content-Type")

    assert mime_references._collect_references(root).locations == frozenset({
        "named-root.png",
    })


def test_unnamed_attachment_subtree_cannot_contribute_references() -> None:
    """Skip an attachment disposition even when it has no filename metadata."""
    attachment = _html('<img src="attachment-decoy.png">')
    attachment["Content-Disposition"] = "attachment"
    message = EmailMessage()
    message.make_mixed()
    message.attach(attachment)
    message.attach(_html('<img src="body.png">'))

    assert mime_references._collect_references(message).locations == frozenset({
        "body.png",
    })


def test_one_sided_content_id_brackets_remain_literal() -> None:
    """Strip brackets only as one complete pair in URI and header channels."""
    references = mime_references._references_from_html(
        '<img src="cid:%3Cleading"><img src="cid:trailing%3E">',
    )

    assert references.content_ids == frozenset({"<leading", "trailing>"})
    assert mime_references._normalize_content_id_header("<leading") == "<leading"
    assert mime_references._normalize_content_id_header("trailing>") == "trailing>"


def test_css_hex_escape_at_eof_never_reads_past_the_value() -> None:
    """Decode a terminal hexadecimal escape without probing beyond EOF."""
    assert mime_references._decode_css_escapes(r"\61") == "a"


def test_text_decoder_replaces_invalid_bytes_and_rejects_nonbytes() -> None:
    """Decode malformed text visibly and fail closed for a non-byte payload."""
    malformed = EmailMessage()
    malformed["Content-Type"] = "text/html; charset=utf-8"
    malformed.set_payload(b"PUBLIC\xff")
    nonbytes = EmailMessage()
    nonbytes.set_payload(["not-an-email-entity"])

    assert mime_references._decode_text_payload(malformed) == "PUBLIC�"
    assert not mime_references._decode_text_payload(nonbytes)


def test_css_attribute_and_element_keep_distinct_reference_channels() -> None:
    """Resolve CSS-attribute locations and collect style-element CIDs."""
    references = mime_references._references_from_html(
        '<base href="assets/">'
        '<div style="background:url(attribute.png)"></div>'
        "<style>.hero{background:url(cid:style@example.test)}</style>",
        base_location="https://public.example/root/body.html",
    )

    assert references == ReferenceIndex(
        frozenset({"style@example.test"}),
        frozenset({"https://public.example/root/assets/attribute.png"}),
    )
