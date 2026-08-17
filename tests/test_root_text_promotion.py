# ruff: file-ignore[private-member-access]
"""Interoperability contracts for canonical root text/plain promotion."""

from __future__ import annotations

import tempfile
from email import policy
from email.message import EmailMessage
from pathlib import Path
from typing import TYPE_CHECKING

from eml_attachment_remover import mime_text_only, process_file
from eml_attachment_remover.mime_text_execution import (
    canonical_text_payload,
    execute_text_only_plan,
)
from tests.test_support import parse

if TYPE_CHECKING:
    from eml_attachment_remover.models import ProcessResult


def _singleton_alternative() -> EmailMessage:
    """Return an alternative wrapper containing only one plain body.

    Returns:
        The singleton body wrapper.

    """
    plain = EmailMessage()
    plain.set_content("PUBLIC SINGLETON BODY")
    message = EmailMessage()
    message.make_alternative()
    message.attach(plain)
    return message


def test_singleton_wrapper_is_a_real_promotion_action() -> None:
    """Rewrite a wrapper even when there is no representation to discard."""
    message = _singleton_alternative()
    plan = mime_text_only._plan_text_only(message)

    assert plan.discard_paths == ()
    assert plan.selected_body.path == (0,)
    assert plan.modified
    assert plan.changed_paths == ((0,),)

    execute_text_only_plan(message, plan)

    assert message.get_content_type() == "text/plain"
    assert not message.is_multipart()
    assert canonical_text_payload(message) == b"PUBLIC SINGLETON BODY\n"


def _nested_message(selected: EmailMessage) -> EmailMessage:
    """Return mixed/related/alternative wrappers around one selected body.

    Returns:
        The nested message with one body resource and ordinary attachment.

    """
    html = EmailMessage()
    html.set_content('<img src="cid:public-resource@example.test">', subtype="html")
    alternative = EmailMessage()
    alternative.make_alternative()
    alternative.attach(selected)
    alternative.attach(html)
    resource = EmailMessage()
    resource.set_content(b"PUBLIC IMAGE", maintype="image", subtype="png")
    resource["Content-ID"] = "<public-resource@example.test>"
    resource["Content-Disposition"] = 'inline; filename="public.png"'
    related = EmailMessage()
    related.make_related()
    related.attach(alternative)
    related.attach(resource)
    message = EmailMessage()
    message["From"] = "sender@example.test"
    message["To"] = "recipient@example.test"
    message["Subject"] = "PUBLIC ROOT SUBJECT"
    message["X-Public-Root"] = "preserved"
    message["MIME-Version"] = "1.0"
    message.make_mixed()
    message.attach(related)
    message.add_attachment(
        b"PUBLIC ATTACHMENT",
        maintype="application",
        subtype="octet-stream",
        filename="public.bin",
    )
    message["Content-MD5"] = "PUBLIC STALE ROOT DIGEST"
    message["Content-Length"] = "999"
    message["Lines"] = "999"
    message["X-MS-Has-Attach"] = "yes"
    message["DKIM-Signature"] = "v=1; d=public.example; b=PUBLIC"
    message.preamble = "PUBLIC STALE PREAMBLE"
    message.epilogue = "PUBLIC STALE EPILOGUE"
    return message


def _content_headers(message: EmailMessage) -> tuple[tuple[str, str], ...]:
    """Return semantic Content-* headers in wire order.

    Returns:
        Case-folded header names paired with their represented values.

    """
    return tuple(
        (name.casefold(), str(value))
        for name, value in message.raw_items()
        if name.casefold().startswith("content-")
    )


def _run_nested_promotion(
    selected: EmailMessage,
) -> tuple[
    EmailMessage,
    ProcessResult,
    ProcessResult,
    tuple[tuple[str, str], ...],
    bytes,
]:
    """Process one nested fixture twice and return stable observations.

    Returns:
        Promoted message, both results, expected headers, and expected payload.

    """
    raw = _nested_message(selected).as_bytes(policy=policy.SMTP)
    with tempfile.TemporaryDirectory() as directory:
        source = Path(directory) / "source.eml"
        output = Path(directory) / "output.eml"
        repeated = Path(directory) / "repeated.eml"
        source.write_bytes(raw)
        parsed_source = parse(source)
        source_selected = next(
            part
            for part in parsed_source.walk()
            if not part.is_multipart() and part.get_content_type() == "text/plain"
        )
        expected_headers = _content_headers(source_selected)
        expected_payload = canonical_text_payload(source_selected)
        result = process_file(source, output, force=False, dry_run=False)
        promoted = parse(output)
        second = process_file(output, repeated, force=False, dry_run=False)
        assert source.read_bytes() == raw
        assert output.read_bytes() == repeated.read_bytes()
    return promoted, result, second, expected_headers, expected_payload


def test_nested_body_promotion_preserves_envelope_and_selected_content() -> None:
    """Promote selected headers/payload and remove every wrapper artifact."""
    selected = EmailMessage()
    selected.set_content(
        "PUBLIC Ž BODY",
        charset="utf-8",
        cte="quoted-printable",
    )
    selected["Content-Language"] = "lv"
    selected["Content-MD5"] = "PUBLIC SELECTED DIGEST"
    selected["X-Leaf-Decoy"] = "must-not-be-promoted"
    promoted, result, second, expected_content_headers, expected_payload = (
        _run_nested_promotion(selected)
    )

    assert promoted.get_content_type() == "text/plain"
    assert not promoted.is_multipart()
    assert [part.get_content_type() for part in promoted.walk()] == ["text/plain"]
    assert canonical_text_payload(promoted) == expected_payload
    assert _content_headers(promoted) == expected_content_headers
    assert promoted["From"] == "sender@example.test"
    assert promoted["To"] == "recipient@example.test"
    assert promoted["Subject"] == "PUBLIC ROOT SUBJECT"
    assert promoted["X-Public-Root"] == "preserved"
    assert promoted["MIME-Version"] == "1.0"
    assert promoted["X-Leaf-Decoy"] is None
    assert promoted["Content-Length"] is None
    assert promoted["Lines"] is None
    assert promoted["X-MS-Has-Attach"] is None
    assert promoted.preamble is None
    assert promoted.epilogue is None
    assert [part.filename for part in result.removed_attachments] == ["public.bin"]
    assert [part.filename for part in result.discarded_body_resources] == ["public.png"]
    assert result.warnings == (
        (
            "rewriting the message invalidates existing transport signatures "
            "(DKIM-Signature); the original EML remains unchanged"
        ),
    )
    assert second.selected_plain_text_bodies[0].path == ()
    assert not second.removed_attachments
    assert not second.discarded_body_representations
    assert not second.discarded_body_resources


def test_plain_root_plan_remains_a_byte_exact_noop() -> None:
    """Do not reserialize an already canonical plain root message."""
    message = EmailMessage()
    message["Subject"] = "PUBLIC PLAIN ROOT"
    message.set_content("PUBLIC BODY")
    raw = message.as_bytes(policy=policy.SMTP)
    with tempfile.TemporaryDirectory() as directory:
        source = Path(directory) / "source.eml"
        output = Path(directory) / "output.eml"
        source.write_bytes(raw)

        result = process_file(source, output, force=False, dry_run=False)

        assert output.read_bytes() == raw

    assert result.selected_plain_text_bodies[0].path == ()
    assert not mime_text_only._plan_text_only(message).modified
    assert mime_text_only._plan_text_only(message).changed_paths == ()
