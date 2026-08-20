# ruff: file-ignore[private-member-access]
"""Mutation-complete boundaries for text-plan execution and digest binding."""

from __future__ import annotations

import base64
import hashlib
from dataclasses import replace
from email.message import EmailMessage

import pytest

from eml_attachment_remover import mime_text_execution, mime_text_only
from eml_attachment_remover.mime_text_plan import TextProjection
from eml_attachment_remover.models import CliError, ExitCode, SelectedPlainTextBody


def _plain(text: str = "PUBLIC SELECTED BODY") -> EmailMessage:
    """Return one safe plain-text body leaf.

    Returns:
        The configured body.

    """
    part = EmailMessage()
    part.set_content(text)
    return part


def _container(kind: str, *children: EmailMessage) -> EmailMessage:
    """Return one populated multipart container.

    Returns:
        The configured aggregate.

    """
    part = EmailMessage()
    getattr(part, f"make_{kind}")()
    for child in children:
        part.attach(child)
    return part


def test_canonical_text_payload_decodes_transfer_encoding_before_normalizing() -> None:
    """Bind the source digest to decoded text rather than base64 wire text."""
    represented = b"PUBLIC\r\nBODY\rTAIL"
    part = EmailMessage()
    part["Content-Type"] = 'text/plain; charset="utf-8"'
    part["Content-Transfer-Encoding"] = "base64"
    part.set_payload(base64.b64encode(represented).decode("ascii"))

    assert mime_text_execution.canonical_text_payload(part) == b"PUBLIC\nBODY\nTAIL"


def test_binding_ignores_nonplain_leaves_and_has_an_exact_failure() -> None:
    """Count only plain leaves and expose the stable verification diagnostic."""
    plain = _plain()
    binary = EmailMessage()
    binary.set_content(
        b"PUBLIC BINARY",
        maintype="application",
        subtype="octet-stream",
    )
    message = _container("mixed", plain, binary)
    digest = hashlib.sha256(
        mime_text_execution.canonical_text_payload(plain)
    ).hexdigest()
    plan = mime_text_only.TextOnlyPlan(
        projection=TextProjection(
            selected_body=SelectedPlainTextBody((0,), "text/plain"),
            selected_payload_sha256=digest,
        ),
    )

    mime_text_execution._verify_plan_binding(message, plan)
    with pytest.raises(CliError) as raised:
        mime_text_execution._verify_plan_binding(
            message,
            replace(
                plan,
                projection=replace(
                    plan.projection,
                    selected_payload_sha256="0" * 64,
                ),
            ),
        )

    assert raised.value.code is ExitCode.VERIFICATION_ERROR
    assert raised.value.message == (
        "text-only execution changed the selected plain-text body"
    )


def test_singleton_container_is_promoted_even_without_discard_roots() -> None:
    """Treat removal of the last wrapper as a real root rewrite."""
    message = _container("mixed", _plain())
    message["Content-MD5"] = "PUBLIC UNCHANGED DIGEST"
    plan = mime_text_only._plan_text_only(message)

    mime_text_execution.execute_text_only_plan(message, plan)

    assert plan.modified
    assert not plan.discard_paths
    assert message.get_content_type() == "text/plain"
    assert message.get("Content-MD5") is None
    assert message.get_content().strip() == "PUBLIC SELECTED BODY"


def test_nested_promotion_clears_root_digest_without_rewriting_detached_wrapper() -> (
    None
):
    """Replace the root atomically and leave detached source entities untouched."""
    html = EmailMessage()
    html.set_content("<p>PUBLIC HTML</p>", subtype="html")
    body = _container("alternative", _plain(), html)
    body["Content-MD5"] = "PUBLIC INNER DIGEST"
    message = _container("mixed", body)
    message["Content-MD5"] = "PUBLIC OUTER DIGEST"
    plan = mime_text_only._plan_text_only(message)

    mime_text_execution.execute_text_only_plan(message, plan)

    assert message.get("Content-MD5") is None
    assert body.get("Content-MD5") == "PUBLIC INNER DIGEST"
    assert [part.get_content_type() for part in message.walk()] == [
        "text/plain",
    ]
