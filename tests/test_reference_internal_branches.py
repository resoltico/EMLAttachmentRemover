# ruff: file-ignore[private-member-access]
"""Cover defensive reference-parser and MIME-location branches."""

from __future__ import annotations

from email.message import EmailMessage
from unittest.mock import MagicMock

from eml_attachment_remover import (
    css_references,
    html_references,
    mime_locations,
    mime_references,
)


def test_css_lexer_handles_unterminated_comment_and_string() -> None:
    """Consume malformed CSS through EOF without inventing references."""
    assert css_references._css_reference_values("/* url(decoy.png)") == ()
    assert css_references._css_reference_values('"url(decoy.png)') == ()


def test_html_parser_handles_empty_duplicate_and_self_closing_attributes() -> None:
    """Keep the first base and tolerate valueless and self-closing tags."""
    values = html_references._reference_values(
        '<base href="first/"><base href="second/"><img src><style></style>'
        "url(decoy.png)<div/>",
    )

    assert values.base_href == "first/"
    assert not values.uris
    assert not values.style_elements


def test_html_parser_balances_self_closing_and_nested_templates() -> None:
    """Treat non-void self-closing syntax as a start and balance templates."""
    values = html_references._reference_values(
        '<template/><img src="decoy-one.png"></template>'
        '<template><template><img src="decoy-two.png"></template></template>'
        '<img src="public.png">',
    )

    assert values.uris == ["public.png"]


def test_html_parser_uses_only_first_duplicate_attribute() -> None:
    """Match HTML tokenization by dropping later duplicate attributes."""
    values = html_references._reference_values(
        '<img src src="decoy.png">'
        '<link rel="stylesheet" rel="alternate" href="public.css">',
    )

    assert values.uris == ["public.css"]


def test_self_closing_style_syntax_still_opens_html_raw_text() -> None:
    """Honor HTML's ignored self-closing flag on a non-void style element."""
    values = html_references._reference_values(
        "<style/>body{background:url(public.png)}</style>",
    )

    assert values.style_elements == ["body{background:url(public.png)}"]


def test_uri_authority_normalization_handles_ipv6_user_info_and_port() -> None:
    """Lowercase only scheme and host in an IPv6 authority."""
    assert (
        mime_locations._canonical_location(
            "HTTP://Public@[ABCD::EF]:8080/Case?Q=X",
        )
        == "http://Public@[abcd::ef]:8080/Case?Q=X"
    )


def test_dot_segment_normalizer_covers_every_rfc_transition() -> None:
    """Handle leading, embedded, and terminal dot-segment productions."""
    normalize = mime_locations._remove_dot_segments

    assert normalize("../a") == "a"
    assert normalize("/a/./b") == "/a/b"
    assert normalize("/a/.") == "/a/"
    assert normalize("/a/../b") == "/b"
    assert normalize("/a/..") == "/"
    assert not normalize(".")
    assert not normalize("..")
    assert normalize("/a//b") == "/a//b"
    assert mime_locations._normalize_path("a/./b") == "a/b"
    assert mime_locations._normalize_path("../../a/../b") == "../../b"
    assert mime_locations._normalize_path("../..") == "../.."


def test_empty_location_and_payload_fallbacks_are_stable() -> None:
    """Return no empty resource label and tolerate unusual payload objects."""
    assert mime_locations._canonical_location("   ") is None
    bytes_payload = MagicMock()
    bytes_payload.get_payload.side_effect = [None, b"public"]
    bytes_payload.get_content_charset.return_value = "utf-8"
    object_payload = MagicMock()
    object_payload.get_payload.side_effect = [None, object()]
    object_payload.get_content_charset.return_value = "utf-8"

    assert mime_references._decode_text_payload(bytes_payload) == "public"
    assert not mime_references._decode_text_payload(object_payload)


def test_malformed_uri_and_non_scheme_values_remain_stable() -> None:
    """Avoid crashing on malformed IPv6 and reject invalid scheme prefixes."""
    assert mime_locations._canonical_location("http://[invalid") == "http://[invalid"
    assert not mime_locations._is_absolute_uri("1public:value")
    assert not mime_locations._is_absolute_uri("public/path")


def test_html_mime_base_uses_legacy_relative_content_base() -> None:
    """Resolve a received relative Content-Base against its inherited base."""
    part = EmailMessage()
    part["Content-Base"] = "assets/"

    assert (
        mime_locations._html_mime_base(
            part,
            "https://public.example/root/",
        )
        == "https://public.example/root/assets/"
    )
    assert mime_locations._html_mime_base(part, None) == "assets/"


def test_html_mime_base_falls_back_after_invalid_join() -> None:
    """Return the inherited base when a Content-Location join is malformed."""
    part = EmailMessage()
    part["Content-Location"] = "body.html"
    inherited = "http://[invalid"

    assert mime_locations._html_mime_base(part, inherited) == inherited


def test_resource_content_base_resolves_against_inherited_base() -> None:
    """Resolve a resource's relative legacy base before its location label."""
    part = EmailMessage()
    part["Content-Base"] = "assets/"
    part["Content-Location"] = "hero.png"

    assert mime_locations._resource_location_variants(
        part,
        "https://public.example/root/",
    ) == frozenset({
        "https://public.example/root/assets/hero.png",
    })
