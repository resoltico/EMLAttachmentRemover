# ruff: file-ignore[private-member-access]
"""Exact defensive branches for the internal text-only core."""

from __future__ import annotations

from dataclasses import replace
from email.message import EmailMessage
from unittest.mock import MagicMock

import pytest

from eml_attachment_remover import (
    mime_text_execution,
    mime_text_only,
    mime_text_resources,
    mime_text_selection,
)
from eml_attachment_remover.models import CliError, ExitCode, ReferenceIndex


def _plain() -> EmailMessage:
    """Return one safe plain body.

    Returns:
        The configured plain-text leaf.

    """
    part = EmailMessage()
    part.set_content("PUBLIC PLAIN BODY")
    return part


def _html() -> EmailMessage:
    """Return one synthetic HTML representation.

    Returns:
        The configured HTML leaf.

    """
    part = EmailMessage()
    part.set_content("<p>PUBLIC HTML</p>", subtype="html")
    return part


def _container(kind: str, *children: EmailMessage) -> EmailMessage:
    """Return one populated multipart container.

    Returns:
        The multipart entity selected by ``kind``.

    """
    part = EmailMessage()
    getattr(part, f"make_{kind}")()
    for child in children:
        part.attach(child)
    return part


def _binary(*, inline: bool = False) -> EmailMessage:
    """Return one anonymous or inline binary entity.

    Returns:
        The configured application payload.

    """
    part = EmailMessage()
    part.set_content(
        b"PUBLIC BINARY",
        maintype="application",
        subtype="octet-stream",
    )
    if inline:
        part["Content-Disposition"] = "inline"
    return part


@pytest.mark.parametrize(
    ("raw_payload", "expected"),
    [
        (b"PUBLIC\rBYTES", b"PUBLIC\nBYTES"),
        ("PUBLIC\r\nTEXT", b"PUBLIC\nTEXT"),
        (object(), b""),
    ],
)
def test_canonical_payload_defensive_fallbacks(
    raw_payload: object,
    expected: bytes,
) -> None:
    """Normalize every supported non-decoded payload fallback."""
    part = MagicMock(spec=EmailMessage)
    part.get_payload.side_effect = [None, raw_payload]

    assert mime_text_execution.canonical_text_payload(part) == expected


def test_executor_clears_digest_and_rejects_an_unresolved_action() -> None:
    """Clear changed-container digests and reject stale immutable paths."""
    message = _container("alternative", _plain(), _html())
    message["Content-MD5"] = "PUBLIC STALE DIGEST"
    plan = mime_text_only._plan_text_only(message)

    mime_text_execution.execute_text_only_plan(message, plan)

    assert message.get("Content-MD5") is None
    assert message.get_content_type() == "text/plain"
    assert not message.is_multipart()
    stale_message = _container("alternative", _plain(), _html())
    stale_message["Content-MD5"] = "PUBLIC UNCHANGED DIGEST"
    stale = replace(
        mime_text_only._plan_text_only(stale_message),
        discard_paths=((99,),),
    )
    with pytest.raises(RuntimeError, match="no longer resolves: 100"):
        mime_text_execution.execute_text_only_plan(stale_message, stale)
    assert stale_message.is_multipart()
    assert stale_message["Content-MD5"] == "PUBLIC UNCHANGED DIGEST"


@pytest.mark.parametrize("kind", ["alternative", "related", "mixed"])
def test_planner_rejects_nontraversable_multipart_payloads(kind: str) -> None:
    """Fail closed for a multipart declaration without a child-message list."""
    malformed = EmailMessage()
    malformed["Content-Type"] = f"multipart/{kind}"
    malformed.set_payload("PUBLIC NON-LIST PAYLOAD")

    with pytest.raises(CliError) as raised:
        mime_text_only._plan_text_only(malformed)

    assert raised.value.code is ExitCode.TRANSFORMATION_UNAVAILABLE
    assert "no traversable" in raised.value.message


@pytest.mark.parametrize("kind", ["alternative", "mixed"])
def test_planner_rejects_multiple_plain_body_candidates(kind: str) -> None:
    """Refuse to guess between two structurally eligible plain bodies."""
    ambiguous = _container(kind, _plain(), _plain())

    with pytest.raises(CliError) as raised:
        mime_text_only._plan_text_only(ambiguous)

    assert raised.value.code is ExitCode.TRANSFORMATION_UNAVAILABLE
    assert "exactly one" in raised.value.message or "one unique" in raised.value.message


def test_related_planner_rejects_unresolved_root_and_declared_type_mismatch() -> None:
    """Require a unique root whose declared and actual types agree."""
    empty = EmailMessage()
    empty.make_related()
    empty.set_payload([])
    with pytest.raises(CliError, match="does not resolve one unique root"):
        mime_text_only._plan_text_only(empty)

    mismatched = _container("related", _plain())
    mismatched.set_param("type", "text/html")
    with pytest.raises(CliError, match="type does not match"):
        mime_text_only._plan_text_only(mismatched)


def test_mixed_inline_binary_is_a_resource_and_not_an_attachment() -> None:
    """Give explicit resource identity precedence over binary fallback."""
    message = _container("mixed", _plain(), _binary(inline=True))

    plan = mime_text_only._plan_text_only(message)

    assert not plan.removed_attachments
    assert [resource.path for resource in plan.discarded_resources] == [(1,)]


@pytest.mark.parametrize(
    "content_type",
    ["text/html", "application/pkcs7-mime"],
)
def test_root_unsupported_or_protected_entity_is_not_a_plain_body(
    content_type: str,
) -> None:
    """Exercise protected and ordinary unsupported root diagnostics."""
    message = EmailMessage()
    message.set_type(content_type)
    message.set_payload("PUBLIC UNSUPPORTED BODY")

    with pytest.raises(CliError) as raised:
        mime_text_only._plan_text_only(message)

    assert raised.value.code is ExitCode.TRANSFORMATION_UNAVAILABLE
    assert "not a safe plain-text body" in raised.value.message


def test_resource_scanner_rejects_invalid_start_and_models_empty_aggregate() -> None:
    """Do not invent reference edges and retain empty action-root identity."""
    related = _container("related", _html())
    related.set_param("start", "<missing@example.test>")
    assert not mime_text_resources.html_reference_sources(related, (), None)

    empty = EmailMessage()
    empty.make_mixed()
    empty.set_payload([])
    sources = (
        (
            (1,),
            ReferenceIndex(frozenset(), frozenset()),
        ),
    )
    omitted = mime_text_resources.ResourceScan(
        reference_sources=sources,
        inherited_base=None,
        include_text=True,
        root_inclusion=mime_text_resources.RootInclusion.OMIT,
    )
    retained = mime_text_resources.ResourceScan(
        reference_sources=sources,
        inherited_base=None,
        include_text=True,
        root_inclusion=mime_text_resources.RootInclusion.INCLUDE,
    )

    assert not mime_text_resources.resource_leaves(empty, (2,), omitted)
    records = mime_text_resources.resource_leaves(empty, (2,), retained)
    assert [record.path for record in records] == [(2,)]


def test_selection_handles_empty_related_and_nested_mixed_candidate() -> None:
    """Cover empty root resolution and recursive mixed candidate selection."""
    related = EmailMessage()
    related.make_related()
    assert mime_text_selection.related_root_index(related, []) is None

    nested = _container("mixed", _plain(), _binary())
    assert mime_text_selection.can_select_plain(nested)
