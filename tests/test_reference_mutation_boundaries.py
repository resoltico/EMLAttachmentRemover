# ruff: file-ignore[private-member-access]
"""Adversarial contracts for bounded public resource-reference parsing."""

from __future__ import annotations

from eml_attachment_remover import (
    css_references,
    html_references,
    mime_references,
    srcset_references,
)
from eml_attachment_remover.models import ReferenceIndex


def test_css_lexical_cursor_boundaries_distinguish_real_references() -> None:
    """Resume after the exact end of comments, strings, and escaped newlines."""
    source = (
        "/* closed */ /* second */url(first.png)"
        '"url(decoy.png)\\\r\ncontinued"url(second.png)'
        "'ordinary'url(third.png)X;url(after-x.png)"
    )

    assert css_references._css_reference_values(source) == (
        "first.png",
        "second.png",
        "third.png",
        "after-x.png",
    )
    assert css_references._after_comment("/* one */tail*/", 0) == 9
    assert css_references._after_string("xx'public'yy", 2) == 10
    assert css_references._after_string('"\\x', 0) == 3
    assert css_references._after_string('"\\', 0) == 2


def test_srcset_cursor_and_descriptor_boundaries_remain_finite_and_exact() -> None:
    """Consume commas, URLs, parentheses, and every supported descriptor kind."""
    value = ", first.png, second.png 2x, third.png 10w 5h"

    assert srcset_references._srcset_urls(value) == (
        "first.png",
        "second.png",
        "third.png",
    )
    assert srcset_references._srcset_urls("bad.png 1w 2x, next.png 1x") == ("next.png",)
    assert srcset_references._srcset_urls("height-only.png 5h") == ()
    assert srcset_references._consume_descriptors(" fn(a,b),next", 0) == (
        ("fn(a,b)",),
        9,
    )
    assert srcset_references._consume_descriptors("", 0) == ((), 0)
    assert srcset_references._consume_candidate("", 0) == ("", (), 0)
    assert srcset_references._srcset_urls("") == ()


def test_css_escape_scalar_boundaries_and_cursor_progress_are_exact() -> None:
    """Decode line continuations and the full valid Unicode scalar boundary."""
    decode = mime_references._decode_css_escapes

    assert decode(r"a\110000b\d800 b\dfff c") == "a�b�b�c"
    assert decode(r"a\10ffff b") == "a\U0010ffffb"
    assert decode("a\\\fcontinued\\Xtail") == "acontinuedXtail"
    assert decode(r"abc\12345 ") == "abc\U00012345"
    assert decode(r"abc\1234567") == "abc�7"


def test_reference_channels_preserve_cid_location_base_and_decoding() -> None:
    """Keep each URI channel's identity, base, and CSS-decoding boundary."""
    references = mime_references._references_from_html(
        '<base href="assets/">'
        '<img src="cid:image@example.test" '
        'srcset="first.png 1x, cid:srcset@example.test 2x" '
        'style="background:url(cid:\\73 tyle@example.test)">'
        "<style>.hero{background:url(c\\73 s.png)}</style>",
        base_location="https://public.example/root/body.html",
    )

    assert references == ReferenceIndex(
        frozenset({
            "image@example.test",
            "srcset@example.test",
            "style@example.test",
        }),
        frozenset({
            "https://public.example/root/assets/first.png",
            "https://public.example/root/assets/css.png",
        }),
    )


def test_link_rendering_policy_covers_href_xlink_and_imagesrcset() -> None:
    """Reject alternate links while accepting each rendering link channel."""
    references = mime_references._references_from_html(
        '<link href="decoy-no-rel.css">'
        '<link rel="alternate" href="decoy-one.css" '
        'xlink:href="decoy-two.css" imagesrcset="decoy-three.png">'
        '<link rel="stylesheet" href="public.css">'
        '<link rel="preload" xlink:href="public-xlink.css" '
        'imagesrcset="public.png 1x">',
    )

    assert references.locations == frozenset({
        "public.css",
        "public-xlink.css",
        "public.png",
    })


def test_input_type_requires_an_explicit_image_value() -> None:
    """Treat missing, valueless, and non-image input types as decoys."""
    references = mime_references._references_from_html(
        '<input src="missing.png"><input type src="empty.png">'
        '<input type="text" src="text.png">'
        '<input type="IMAGE" src="public.png">',
    )

    assert references.locations == frozenset({"public.png"})


def test_vml_namespace_uses_exact_qualified_names_and_late_declarations() -> None:
    """Accept a later exact declaration but reject multi-colon lookalikes."""
    references = mime_references._references_from_html(
        '<html xmlns:bad="urn:example:test" '
        'xmlns:v="urn:schemas-microsoft-com:vml">'
        '<v:fill src="public.png" />'
        '<v:extra:fill src="decoy-one.png" />'
        '<v:extra:fill xmlns:v:extra="urn:schemas-microsoft-com:vml" '
        'src="decoy-two.png" /></html>',
    )

    assert references.locations == frozenset({"public.png"})


def test_vml_namespace_declarations_end_at_normal_and_self_closing_scopes() -> None:
    """Prevent a local namespace binding from authorizing later decoy elements."""
    references = mime_references._references_from_html(
        '<section xmlns:v="urn:schemas-microsoft-com:vml">'
        '<v:fill src="first.png"></v:fill></section>'
        '<v:fill src="decoy-one.png"></v:fill>'
        '<v:fill xmlns:v="urn:schemas-microsoft-com:vml" src="second.png" />'
        '<v:fill src="decoy-two.png"></v:fill>',
    )

    assert references.locations == frozenset({"first.png", "second.png"})


def test_nested_style_and_template_end_tags_do_not_leak_parser_state() -> None:
    """Only the real style interval contributes CSS and templates stay inert."""
    references = mime_references._references_from_html(
        "<style>url(first.png)<span></span>url(second.png)</style>"
        "<template><template><img src='decoy.png'></template></template>"
        "<img src='public.png'>",
    )

    assert references.locations == frozenset({
        "first.png",
        "second.png",
        "public.png",
    })


def test_mso_exact_size_and_nested_base_contracts_are_observable() -> None:
    """Accept the published maximum and propagate the first nested base only."""
    shell = "[if mso]><base href='assets/'><img src='public.png'><![endif]"
    padding = " " * (html_references.MAX_MSO_CONDITIONAL_COMMENT_LENGTH - len(shell))
    references = mime_references._references_from_html(
        f"<!--{shell}{padding}-->",
        base_location="https://public.example/root/body.html",
    )

    assert references.locations == frozenset({
        "https://public.example/root/assets/public.png",
    })
    assert mime_references._references_from_html(
        "<!--[if mso]><!-- ordinary <![endif]-->",
    ) == ReferenceIndex(frozenset(), frozenset())


def test_mso_conditional_nesting_advances_one_level_per_fragment() -> None:
    """Retain content through the allowed depth and reject one level beyond it."""
    accepted = html_references._ReferenceValueParser(
        conditional_ancestors=("ancestor",)
        * (html_references.MAX_MSO_CONDITIONAL_DEPTH - 1),
    )
    accepted.handle_comment('[if mso]><img src="public.png"><![endif]')
    rejected = html_references._ReferenceValueParser(
        conditional_ancestors=("ancestor",) * html_references.MAX_MSO_CONDITIONAL_DEPTH,
    )
    rejected.handle_comment('[if mso]><img src="decoy.png"><![endif]')

    assert accepted.uris == ["public.png"]
    assert rejected.uris == []
