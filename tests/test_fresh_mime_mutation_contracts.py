# ruff: file-ignore[private-member-access]
"""Exact contracts for fresh MIME and reference-parser mutation boundaries."""

from __future__ import annotations

from email.message import EmailMessage
from unittest.mock import patch

from eml_attachment_remover import (
    css_references,
    html_references,
    mime_locations,
    mime_policy,
    mime_references,
    mime_serialization,
    srcset_references,
)
from eml_attachment_remover.models import ReferenceIndex


def test_css_comment_and_escaped_quote_cursors_use_exact_token_edges() -> None:
    """Recognize the earliest comment end and skip an escaped string quote."""
    assert css_references._after_comment("/**/tail", 0) == len("/**/")
    assert css_references._after_comment("/*unterminated", 0) == len(
        "/*unterminated",
    )
    assert css_references._after_string('""tail', 0) == len('""')
    assert css_references._after_string('"a"tail', 0) == len('"a"')
    assert css_references._after_string('"\\x"tail', 0) == len('"\\x"')
    assert css_references._after_string('"\\"tail"rest', 0) == len('"\\"tail"')


def test_qualified_names_require_exactly_two_populated_components() -> None:
    """Reject absent, empty, and extra namespace-name components."""
    assert html_references._qualified_name("v:fill") == ("v", "fill")
    assert html_references._qualified_name("fill") is None
    assert html_references._qualified_name(":fill") is None
    assert html_references._qualified_name("v:") is None
    assert html_references._qualified_name("v:extra:fill") is None


def test_namespace_declaration_after_ordinary_attribute_is_applied() -> None:
    """Continue scanning attributes until a later exact namespace declaration."""
    references = mime_references._references_from_html(
        '<v:fill id="public" xmlns:v="urn:schemas-microsoft-com:vml" '
        'src="public.png"></v:fill>',
    )

    assert references.locations == frozenset({"public.png"})


def test_namespace_close_finds_latest_matching_frame_without_skips() -> None:
    """Restore the outer binding across both well-formed and malformed nesting."""
    well_formed = mime_references._references_from_html(
        '<section xmlns:v="urn:schemas-microsoft-com:vml"><div>'
        '<section xmlns:v="urn:example:decoy"></section>'
        '<v:fill src="first.png"></v:fill></div></section>',
    )
    malformed = mime_references._references_from_html(
        '<section xmlns:v="urn:schemas-microsoft-com:vml">'
        '<section xmlns:v="urn:example:decoy"><div></section>'
        '<v:fill src="second.png"></v:fill></section>',
    )

    assert well_formed.locations == frozenset({"first.png"})
    assert malformed.locations == frozenset({"second.png"})


def test_nested_template_depth_and_unrelated_end_tags_preserve_state() -> None:
    """Keep nested templates inert and close CSS only on a style end tag."""
    template_references = mime_references._references_from_html(
        "<template><template></template>"
        '<img src="decoy.png"></template><img src="public.png">',
    )
    parser = html_references._ReferenceValueParser()
    parser.handle_starttag("style", [])
    parser.handle_data("url(first.png)")
    parser.handle_endtag("section")
    parser.handle_data("url(second.png)")
    parser.handle_endtag("style")

    assert template_references.locations == frozenset({"public.png"})
    assert parser.style_elements == ["url(first.png)", "url(second.png)"]


def test_conditional_ancestry_accepts_the_exact_public_limit() -> None:
    """Advance ancestry once per recursively parsed conditional fragment."""
    parser = html_references._ReferenceValueParser(
        conditional_ancestors=("outer",)
        * (html_references.MAX_MSO_CONDITIONAL_DEPTH - 2),
    )
    parser.handle_comment(
        '[if mso]><!--[if mso]><img src="public.png"><![endif]--><![endif]',
    )

    assert parser.uris == ["public.png"]


def test_conditional_ancestry_rejects_recursion_beyond_public_limit() -> None:
    """Propagate ancestry so a nested conditional cannot reset the depth limit."""
    parser = html_references._ReferenceValueParser(
        conditional_ancestors=("ancestor",)
        * (html_references.MAX_MSO_CONDITIONAL_DEPTH - 1),
    )
    parser.handle_comment(
        '[if mso]><!--[if mso]><img src="decoy.png"><![endif]--><![endif]',
    )

    assert parser.uris == []


def test_void_element_namespace_declaration_does_not_leak() -> None:
    """Close a void element's namespace frame before parsing later siblings."""
    references = mime_references._references_from_html(
        '<img xmlns:v="urn:schemas-microsoft-com:vml">'
        '<v:fill src="decoy.png"></v:fill>',
    )

    assert references == ReferenceIndex(frozenset(), frozenset())


def test_absolute_uri_and_dot_segment_transitions_use_first_delimiter() -> None:
    """Accept later colons in a URI and consume only one leading dot segment."""
    assert mime_locations._is_absolute_uri("http:a:b")
    assert not mime_locations._is_absolute_uri(":value")
    assert not mime_locations._is_absolute_uri("1http:value")
    assert not mime_locations._is_absolute_uri("relative")
    assert mime_locations._normalize_path("/../public") == "/public"
    assert mime_locations._remove_dot_segment_step("./a/b", "") == ("a/b", "")
    assert mime_locations._remove_dot_segment_step("../../a", "") == ("../a", "")


def test_mime_base_resolution_uses_location_and_handles_invalid_bases() -> None:
    """Resolve location-only metadata and fail soft on malformed inherited URIs."""
    location_part = EmailMessage()
    location_part["Content-Location"] = "mail/body.html"
    assert (
        mime_locations._content_location_base(
            location_part,
            "https://public.example/root/",
        )
        == "https://public.example/root/mail/body.html"
    )
    assert (
        mime_locations._html_mime_base(
            location_part,
            "https://public.example/root/",
        )
        == "https://public.example/root/mail/body.html"
    )
    assert (
        mime_locations._content_location_base(
            EmailMessage(),
            "https://public.example/root/",
        )
        == "https://public.example/root/"
    )

    base_part = EmailMessage()
    base_part["Content-Base"] = "assets/"
    assert mime_locations._html_mime_base(base_part, "http://[invalid") == "assets/"


def test_content_id_headers_require_paired_angle_brackets() -> None:
    """Remove brackets only when both exact delimiters surround the identifier."""
    normalize = mime_references._normalize_content_id_header

    assert normalize("<public@example.test>") == "public@example.test"
    assert normalize("<public@example.test") == "<public@example.test"
    assert normalize("public@example.test>") == "public@example.test>"


def test_named_html_under_root_related_contributes_references() -> None:
    """Propagate related ancestry so a named HTML body remains reference-bearing."""
    body = EmailMessage()
    body.set_content('<img src="cid:public@example.test">', subtype="html")
    body.set_param("name", "body.html", header="Content-Type")
    mixed = EmailMessage()
    mixed.make_mixed()
    mixed.attach(body)
    related = EmailMessage()
    related.make_related()
    related.attach(mixed)

    assert mime_references._collect_references(related) == ReferenceIndex(
        frozenset({"public@example.test"}),
        frozenset(),
    )


def test_css_attribute_base_and_style_element_cid_use_correct_channels() -> None:
    """Pass both reference sets and the effective base through each CSS channel."""
    references = mime_references._references_from_html(
        '<base href="assets/"><div style="background:url(public.png)"></div>'
        "<style>body{background:url(cid:public@example.test)}</style>",
        base_location="https://public.example/root/body.html",
    )

    assert references == ReferenceIndex(
        frozenset({"public@example.test"}),
        frozenset({"https://public.example/root/assets/public.png"}),
    )


def test_css_escape_consumption_stops_safely_at_exact_end_of_input() -> None:
    """Decode an EOF-terminated hexadecimal escape without indexing past it."""
    assert mime_references._consume_css_escape("41", 0) == ("A", len("41"))
    assert mime_references._consume_css_escape("41 ", 0) == ("A", len("41 "))
    assert mime_references._consume_css_escape("", 0) == ("\\", 0)


def test_srcset_invalid_suffix_and_terminal_comma_are_rejected_exactly() -> None:
    """Reject a non-density suffix and remove only one candidate separator."""
    assert srcset_references._descriptor_kind("1q") is None
    assert srcset_references._consume_candidate("public.png,", 0) == (
        "public.png",
        (),
        len("public.png,"),
    )
    assert not srcset_references._valid_descriptors(("1h",))
    assert srcset_references._valid_descriptors(("1w", "1h"))
    assert not srcset_references._valid_descriptors(("1x", "1w"))
    assert not srcset_references._valid_descriptors(("1x", "1h"))


def test_unnamed_inline_part_is_still_file_like() -> None:
    """Recognize normalized inline disposition without requiring a filename."""
    part = EmailMessage()
    part["Content-Disposition"] = "inline"

    assert part.get_filename() is None
    assert mime_policy._is_file_like(part)


def test_structure_fingerprint_keeps_parameter_before_boundary() -> None:
    """Retain semantic parameters regardless of the boundary parameter's position."""
    message = EmailMessage()
    message["Content-Type"] = (
        'multipart/mixed; protocol="public-protocol"; boundary="public-boundary"'
    )
    message.set_payload([])

    assert mime_serialization._structure_fingerprint(message)[0][2] == (
        ("protocol", "public-protocol"),
    )


def test_malformed_multipart_and_absent_content_type_fail_closed() -> None:
    """Stop at a non-list multipart payload and accept absent parameter metadata."""
    malformed = EmailMessage()
    malformed["Content-Type"] = "multipart/mixed"
    malformed.set_payload("public malformed payload")

    assert mime_serialization._leaf_fingerprints(malformed)
    with patch.object(malformed, "is_multipart", return_value=True):
        assert mime_references._collect_references(malformed) == ReferenceIndex(
            frozenset(),
            frozenset(),
        )
        assert not mime_serialization._leaf_fingerprints(malformed)
    assert mime_serialization._structure_fingerprint(EmailMessage())[0][2] == ()


def test_payload_type_guard_rejects_non_message_list_members() -> None:
    """Narrow runtime multipart payloads instead of relying on erased casts."""
    message = EmailMessage()

    assert mime_references._is_email_message_list([message])
    assert not mime_references._is_email_message_list(["decoy"])
