# ruff: file-ignore[private-member-access]
"""Cover malformed and boundary representations in the URI reference parser."""

from __future__ import annotations

from eml_attachment_remover import html_references, mime_references, srcset_references


def test_css_escape_decoder_preserves_or_consumes_boundary_escapes() -> None:
    """Handle a trailing marker, line continuations, and a literal escape."""
    decode = mime_references._decode_css_escapes

    assert decode("public\\") == "public\\"
    assert decode("public\\\r\ncontinued") == "publiccontinued"
    assert decode("public\\\ncontinued") == "publiccontinued"
    assert decode("public\\q") == "publicq"


def test_css_hex_escape_decoder_handles_invalid_and_terminated_values() -> None:
    """Replace invalid scalars and distinguish optional terminator whitespace."""
    decode = mime_references._decode_css_escapes

    assert decode("public\\0.png") == "public�.png"
    assert decode("public\\110000.png") == "public�.png"
    assert decode("public\\68x.png") == "publichx.png"
    assert decode("public\\68 x.png") == "publichx.png"


def test_srcset_tokenizer_handles_empty_trailing_and_parenthesized_fields() -> None:
    """Skip empty and invalid candidates while retaining valid URL boundaries."""
    urls = srcset_references._srcset_urls

    assert urls(",") == ()
    assert urls("public.png,") == ("public.png",)
    assert urls("invalid.png type(public,wide), next.png 2x") == ("next.png",)


def test_srcset_descriptor_validation_matches_html_numeric_rules() -> None:
    """Accept valid width/density forms and reject conflicting descriptors."""
    urls = srcset_references._srcset_urls

    assert urls("plain.png, zero.png 0x, density.png .5e+2x") == (
        "plain.png",
        "zero.png",
        "density.png",
    )
    assert urls("future.png 100w 50h") == ("future.png",)
    assert (
        urls(
            "zero-width.png 0w, negative.png -1x, "
            "mixed.png 1w 2x, duplicate.png 1x 2x, bogus.png bogus",
        )
        == ()
    )


def test_invalid_html_base_falls_back_to_the_mime_base() -> None:
    """Keep a malformed public base from aborting conservative extraction."""
    invalid_mime_base = "http://[invalid-ipv6"

    assert (
        mime_references._effective_html_base(
            "public.png",
            invalid_mime_base,
        )
        == invalid_mime_base
    )


def test_only_an_actual_base_element_can_override_the_mime_base() -> None:
    """Ignore base-like text in comments and raw-text elements."""
    source = (
        '<!-- <base href="https://decoy.example/"> -->'
        "<script>const decoy = '<base href=\"https://script.example/\">';</script>"
        '<base href="../assets/">'
    )

    assert (
        mime_references._effective_html_base(
            html_references._reference_values(source).base_href,
            "https://public.example/mail/body/index.html",
        )
        == "https://public.example/mail/assets/"
    )


def test_css_valid_escapes_comments_and_raw_entities_are_indexed() -> None:
    """Retain every valid CSS spelling without corrupting raw-text entities."""
    references = mime_references._references_from_html(
        r"<style>"
        r".escaped{background:u\72l(escaped.png)}"
        r".commented{background:url/**/(/**/commented.png/**/)}"
        r".raw{background:url(literal&amp;name.png)}"
        r"</style>",
    )

    assert references.locations == frozenset({
        "commented.png",
        "escaped.png",
        "literal&amp;name.png",
    })


def test_comment_and_raw_text_markup_cannot_create_references() -> None:
    """Ignore URI syntax where HTML does not interpret it as markup or CSS."""
    references = mime_references._references_from_html(
        '<!-- <img src="comment.png"><style>url(comment.css)</style> -->'
        "<script>const decoy = '<img src=\"script.png\"> url(script.css)';"
        "</script>"
        '<textarea><img src="textarea.png"></textarea>'
        "<style>.public{background:url(real.png)}</style>",
    )

    assert references.locations == frozenset({"real.png"})


def test_empty_cid_representation_adds_no_reference() -> None:
    """Ignore a syntactically present CID scheme with no identifier."""
    content_ids: set[str] = set()
    locations: set[str] = set()

    mime_references._record_uri_reference(
        "cid:",
        content_ids,
        locations,
        None,
    )

    assert not content_ids
    assert not locations
