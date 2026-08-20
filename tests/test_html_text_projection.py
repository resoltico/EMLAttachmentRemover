"""Contracts for safe readable-text projection from equivalent HTML bodies."""

from __future__ import annotations

import hashlib
from email import policy
from email.message import EmailMessage
from typing import TYPE_CHECKING

import pytest

from eml_attachment_remover import mime_text_only, process_file
from eml_attachment_remover.html_text import (
    project_html_text,
    text_equivalent,
)
from eml_attachment_remover.mime_text_execution import (
    canonical_text_payload,
    execute_text_only_plan,
)
from eml_attachment_remover.mime_text_only import html_representation_text
from eml_attachment_remover.mime_text_plan import TextOnlyPlan, TextProjection
from eml_attachment_remover.models import CliError, ExitCode, SelectedPlainTextBody
from tests.test_support import parse

if TYPE_CHECKING:
    from pathlib import Path


def _alternative(plain: str, html: str) -> EmailMessage:
    """Return one ordinary plain/HTML alternative.

    Returns:
        A message whose first branch is plain text and second is HTML.

    """
    message = EmailMessage()
    message.set_content(plain)
    message.add_alternative(html, subtype="html")
    return message


def test_html_projection_preserves_visible_blocks_and_lists() -> None:
    """Render readable document structure while omitting hidden markup content."""
    rendered = project_html_text(
        "<h1>Heading &amp; entity</h1><p>Lead <strong>text</strong></p>"
        "<ol><li>First</li><li>Second<ul><li>Nested</li></ul></li></ol>"
        "<script>private()</script><style>.hidden { display: none; }</style>",
    )

    assert rendered == (
        "Heading & entity\n\nLead text\n\n1. First\n2. Second\n\n- Nested\n"
    )


def test_equivalence_allows_only_layout_whitespace() -> None:
    """Accept split/merged whitespace but reject any visible character difference."""
    assert text_equivalent("Question1Answer", "Question 1\n\nAnswer\n")
    assert not text_equivalent("Question1Answer", "Question 2\n\nAnswer\n")


def test_html_projection_handles_void_tags_tables_and_loose_list_items() -> None:
    """Keep semantic table/list text while discarding nontext void elements."""
    rendered = project_html_text(
        "<table><tr><th>Head</th><td>Value</td></tr></table>"
        "<br><br/><img/><img alt=omit>"
        "<li>Loose</li><script>hidden</script><style>hidden {}</style>",
    )

    assert rendered == "Head Value\n\n- Loose\n"
    assert project_html_text("Before<img/>After") == "BeforeAfter\n"
    assert project_html_text("<table><tr><th>One</th><th>Two</th></tr></table>") == (
        "One Two\n"
    )


def test_html_projection_of_hidden_only_markup_is_empty() -> None:
    """Emit no body content for a document made only of ignored elements."""
    assert not project_html_text(
        "<head>title</head><template><div>hidden</div></template></ul><img/>",
    )


def test_html_projection_preserves_exact_structural_boundaries() -> None:
    """Keep blocks, table cells, list endings, and both HTML line-break spellings."""
    assert (
        project_html_text(
            "Before<p>Paragraph</p><tr><th>Header</th><td>Cell</td></tr>"
            "After<br>Last<br/>Tail",
        )
        == "Before\n\nParagraph\n\nHeader Cell\n\nAfter\nLast\nTail\n"
    )
    assert project_html_text("Before<table><td>Cell</td></table>After") == (
        "Before\n\nCellAfter\n"
    )
    assert project_html_text("<ul><li>One</li>Tail</ul>") == "- One\nTail\n"
    assert project_html_text("Before<tr>Row") == "Before\n\nRow\n"


def test_html_projection_isolates_nested_hidden_markup_and_normalizes_newlines() -> (
    None
):
    """Prevent hidden nested markup from changing visible text or layout."""
    assert (
        project_html_text(
            "Before<template><template>hidden</template>hidden</template>After",
        )
        == "BeforeAfter\n"
    )
    assert (
        project_html_text("Before<template><img/></template>After") == "BeforeAfter\n"
    )
    assert project_html_text("One\r\nTwo\rThree \n Four") == "One\nTwo\nThree\nFour\n"


def test_html_representation_rejects_malformed_or_rootless_related_bodies() -> None:
    """Do not use a related formatting source that lacks one resolvable HTML root."""
    malformed = EmailMessage()
    malformed["Content-Type"] = "multipart/related"
    malformed.set_payload("not a MIME child list")
    rootless = EmailMessage()
    rootless.make_related()

    assert html_representation_text(malformed) is None
    assert html_representation_text(rootless) is None


def test_html_representation_rejects_a_file_like_related_container() -> None:
    """Do not use an attached related subtree as a body formatting source."""
    related = EmailMessage()
    related.make_related()
    related["Content-Disposition"] = "attachment"
    html = EmailMessage()
    html.set_content("<p>PUBLIC HTML</p>", subtype="html")
    related.attach(html)

    assert html_representation_text(related) is None


def test_source_binding_rejects_a_changed_plain_source() -> None:
    """Reject execution before promotion when the selected source digest differs."""
    message = EmailMessage()
    message.set_content("PUBLIC BODY")
    plan = TextOnlyPlan(
        projection=TextProjection(
            selected_body=SelectedPlainTextBody((), "text/plain"),
            selected_payload_sha256="0" * 64,
        ),
    )

    with pytest.raises(CliError) as raised:
        execute_text_only_plan(message, plan)

    assert raised.value.code is ExitCode.VERIFICATION_ERROR
    assert (
        raised.value.message
        == "text-only execution changed the selected plain-text source"
    )


def test_equivalent_html_layout_replaces_unreadable_plain_spacing(
    tmp_path: Path,
) -> None:
    """Promote HTML's visual block boundaries when non-whitespace text is exact."""
    source = tmp_path / "source.eml"
    output = tmp_path / "output.eml"
    message = _alternative(
        "Question1Answer\n\nQuestion2Second answer\n",
        "<div>Question 1</div><div>Answer</div>"
        "<div>Question 2</div><div>Second answer</div>",
    )
    raw = message.as_bytes(policy=policy.SMTP)
    source.write_bytes(raw)

    result = process_file(source, output, force=False, dry_run=False)
    rendered = parse(output)

    assert result.selected_plain_text_bodies[0].content_type == "text/plain"
    assert (
        canonical_text_payload(rendered)
        == b"Question 1\n\nAnswer\n\nQuestion 2\n\nSecond answer\n"
    )
    assert output.read_bytes() != raw
    assert source.read_bytes() == raw


def test_projection_binds_its_distinct_semantic_and_layout_sources() -> None:
    """Record both source roles rather than treating rendered text as unowned."""
    message = _alternative(
        "Question1Answer\n",
        "<p>Question 1</p><p>Answer</p>",
    )
    plan = mime_text_only._plan_text_only(message)  # ruff: ignore[private-member-access]

    assert plan.projection.selected_body.path == (0,)
    assert plan.projection.layout_source == (1,)
    assert plan.projection.rendered_text == "Question 1\n\nAnswer\n"
    assert (
        plan.projection.expected_payload_sha256
        == hashlib.sha256(
            b"Question 1\n\nAnswer\n",
        ).hexdigest()
    )

    with pytest.raises(ValueError, match="layout source and rendered text"):
        TextProjection(
            selected_body=SelectedPlainTextBody((), "text/plain"),
            selected_payload_sha256="0" * 64,
            layout_source=(1,),
        )


def test_equivalent_related_html_layout_replaces_unreadable_plain_spacing(
    tmp_path: Path,
) -> None:
    """Use a related HTML body as a layout-only representation of the plain text."""
    source = tmp_path / "source.eml"
    output = tmp_path / "output.eml"
    message = EmailMessage()
    message.set_content("Question1Answer\n")
    related = EmailMessage()
    related.make_related()
    html = EmailMessage()
    html.set_content("<div>Question 1</div><div>Answer</div>", subtype="html")
    related.attach(html)
    message.make_alternative()
    message.attach(related)
    source.write_bytes(message.as_bytes(policy=policy.SMTP))

    process_file(source, output, force=False, dry_run=False)

    assert canonical_text_payload(parse(output)) == b"Question 1\n\nAnswer\n"


def test_projected_text_uses_canonical_utf8_quoted_printable_headers(
    tmp_path: Path,
) -> None:
    """Make the rendered-text MIME envelope exact and externally inspectable."""
    source = tmp_path / "source.eml"
    output = tmp_path / "output.eml"
    source.write_bytes(
        _alternative("Café1Answer\n", "<div>Café 1</div><div>Answer</div>").as_bytes(
            policy=policy.SMTP,
        ),
    )

    process_file(source, output, force=False, dry_run=False)

    rendered = parse(output)
    assert rendered.get_content_type() == "text/plain"
    assert rendered.get_content_charset() == "utf-8"
    assert rendered["Content-Transfer-Encoding"] == "quoted-printable"
    assert canonical_text_payload(rendered) == "Café 1\n\nAnswer\n".encode()


def test_different_html_text_keeps_the_selected_plain_source(tmp_path: Path) -> None:
    """Never use an HTML branch that changes visible non-whitespace content."""
    source = tmp_path / "source.eml"
    output = tmp_path / "output.eml"
    message = _alternative("PUBLIC PLAIN\n", "<p>DIFFERENT HTML</p>")
    source.write_bytes(message.as_bytes(policy=policy.SMTP))

    process_file(source, output, force=False, dry_run=False)

    assert canonical_text_payload(parse(output)) == b"PUBLIC PLAIN\n"
