# ruff: file-ignore[private-member-access]
"""Mutation-strength contracts for decoded HTML, CSS, and source-set references."""

from __future__ import annotations

from eml_attachment_remover import (
    css_references,
    mime_references,
    srcset_references,
)


def test_css_lexer_honors_empty_comments_and_escaped_string_quotes() -> None:
    """Resume after an empty comment without parsing through an escaped quote."""
    assert css_references._css_reference_values("/**/url(after-comment.png)") == (
        "after-comment.png",
    )
    assert css_references._css_reference_values(
        '"ordinary \\" url(decoy.png)" url(after-string.png)',
    ) == ("after-string.png",)


def test_svg_rendering_href_spellings_are_real_references() -> None:
    """Collect both standard and namespaced rendering links outside link tags."""
    references = mime_references._references_from_html(
        '<svg><image href="image.png"><use xlink:href="sprite.svg#icon"></svg>',
    )

    assert references.locations == frozenset({
        "image.png",
        "sprite.svg",
        "sprite.svg#icon",
    })


def test_namespace_declaration_after_ordinary_attributes_is_effective() -> None:
    """Scan every attribute before applying an exact VML namespace binding."""
    references = mime_references._references_from_html(
        '<section id="ordinary" xmlns:v="urn:schemas-microsoft-com:vml">'
        '<v:fill src="bound.png"></section>',
    )

    assert references.locations == frozenset({"bound.png"})


def test_nested_templates_remain_inert_after_inner_and_unmatched_end_tags() -> None:
    """Keep template depth additive and ignore unrelated closing tags within it."""
    references = mime_references._references_from_html(
        '<template><template></template><img src="nested-decoy.png"></template>'
        '<template></span><img src="endtag-decoy.png"></template>'
        '<img src="public.png">',
    )

    assert references.locations == frozenset({"public.png"})


def test_nested_conditional_comment_syntax_is_inert() -> None:
    """Treat an apparent conditional nested inside one HTML comment as a decoy."""
    fragment = '<!--[if mso]><!--[if mso]><img src="decoy.png"><![endif]--><![endif]-->'

    assert not mime_references._references_from_html(fragment).locations


def test_html_void_element_namespace_scope_closes_immediately() -> None:
    """Prevent a namespace declared on a void element from leaking to siblings."""
    references = mime_references._references_from_html(
        '<input xmlns:v="urn:schemas-microsoft-com:vml"><v:fill src="decoy.png">',
    )

    assert not references.locations


def test_srcset_rejects_a_numeric_descriptor_with_an_unknown_suffix() -> None:
    """Require density syntax to end in x even when its number is valid."""
    assert srcset_references._srcset_urls(
        "invalid.png 1q, public.png 1x",
    ) == ("public.png",)
