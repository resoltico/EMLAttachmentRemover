# ruff: file-ignore[private-member-access]
"""Adversarial contracts for wrapper-free root text/plain interoperability."""

from __future__ import annotations

import base64
from email import policy
from email.message import EmailMessage
from email.parser import BytesParser
from typing import TYPE_CHECKING

from eml_attachment_remover import mime_text_execution, mime_text_only, process_file

if TYPE_CHECKING:
    from pathlib import Path

SELECTED_BYTES = b"PUBLIC SELECTED\r\nLINE\rTAIL"
SELECTED_WIRE = base64.b64encode(SELECTED_BYTES).decode("ascii")


def _selected_plain() -> EmailMessage:
    """Return one plain leaf with distinctive content metadata and wire payload.

    Returns:
        The configured selected representation.

    """
    part = EmailMessage()
    part["Content-Type"] = 'text/plain; charset="utf-8"; format="flowed"'
    part["Content-Transfer-Encoding"] = "base64"
    part["Content-Language"] = "en"
    part["Content-Description"] = "PUBLIC SELECTED BODY"
    part["Content-X-Selected"] = "first"
    part["Content-X-Selected"] = "second"
    part.set_payload(SELECTED_WIRE)
    return part


def _wrapped_message(*, include_html: bool) -> EmailMessage:
    """Return a multipart root with duplicate envelope and stale content headers.

    Returns:
        The configured wrapper around the selected plain body.

    """
    message = EmailMessage()
    message["From"] = "sender@example.test"
    message["To"] = "recipient@example.test"
    message["Subject"] = "Public interoperability fixture"
    message["Received"] = "first public hop"
    message["Received"] = "second public hop"
    message["X-Public-Audit"] = "first"
    message["X-Public-Audit"] = "second"
    message["MIME-Version"] = "1.0"
    message.make_mixed()
    message.attach(_selected_plain())
    if include_html:
        html = EmailMessage()
        html.set_content("<p>PUBLIC HTML</p>", subtype="html")
        html["Content-Disposition"] = "attachment"
        message.attach(html)
    message["Content-Language"] = "root-language"
    message["Content-ID"] = "<stale-root@example.test>"
    message["Content-Description"] = "PUBLIC STALE ROOT"
    message["Content-X-Stale"] = "first"
    message["Content-X-Stale"] = "second"
    message.preamble = "PUBLIC PREAMBLE"
    message.epilogue = "PUBLIC EPILOGUE"
    return message


def _parse(raw: bytes) -> EmailMessage:
    """Parse one synthetic wire message using the production-compatible policy.

    Returns:
        The parsed message.

    """
    return BytesParser(policy=policy.default.clone(refold_source="none")).parsebytes(
        raw
    )


def _raw_headers(
    message: EmailMessage,
    *,
    content: bool,
) -> tuple[tuple[str, str], ...]:
    """Return content or non-content raw headers in their original order.

    Returns:
        The selected ordered header category.

    """
    return tuple(
        (name, value)
        for name, value in message.raw_items()
        if name.casefold().startswith("content-") is content
    )


def test_promotion_preserves_envelope_and_replaces_the_content_entity_exactly() -> None:
    """Keep duplicate root headers while adopting selected CTE and payload bytes."""
    message = _parse(_wrapped_message(include_html=True).as_bytes(policy=policy.SMTP))
    payload = message.get_payload()
    assert isinstance(payload, list)
    selected = payload[0]
    assert isinstance(selected, EmailMessage)
    expected_envelope = _raw_headers(message, content=False)
    expected_content = _raw_headers(selected, content=True)
    expected_payload = selected.get_payload()
    plan = mime_text_only._plan_text_only(message)

    mime_text_execution.execute_text_only_plan(message, plan)

    assert plan.selected_body.path == (0,)
    assert plan.modified
    assert _raw_headers(message, content=False) == expected_envelope
    assert _raw_headers(message, content=True) == expected_content
    assert message.get_payload() == expected_payload == SELECTED_WIRE
    assert message.get_content_type() == "text/plain"
    assert message.get("Content-ID") is None
    assert message.get_all("Content-X-Stale") is None
    assert message.get_all("Content-X-Selected") == ["first", "second"]
    assert message.preamble is None
    assert message.epilogue is None


def test_one_child_wrapper_is_modified_and_second_pass_is_byte_identical(
    tmp_path: Path,
) -> None:
    """Collapse a no-discard wrapper once, then make the root-plain pass a no-op."""
    raw = _wrapped_message(include_html=False).as_bytes(policy=policy.SMTP)
    source = tmp_path / "source.eml"
    output = tmp_path / "output.eml"
    repeated = tmp_path / "repeated.eml"
    source.write_bytes(raw)
    source_message = _parse(raw)
    plan = mime_text_only._plan_text_only(source_message)
    source_payload = source_message.get_payload()
    assert isinstance(source_payload, list)
    selected = source_payload[0]
    assert isinstance(selected, EmailMessage)
    expected_headers = (
        _raw_headers(source_message, content=False),
        _raw_headers(selected, content=True),
    )

    first_result = process_file(source, output, force=False, dry_run=False)
    second_result = process_file(output, repeated, force=False, dry_run=False)
    parsed_output = _parse(output.read_bytes())

    assert plan.discard_paths == ()
    assert plan.modified
    assert plan.changed_paths == ((0,),)
    assert first_result.selected_plain_text_bodies[0].path == (0,)
    assert parsed_output.get_content_type() == "text/plain"
    assert not parsed_output.is_multipart()
    assert (
        _raw_headers(parsed_output, content=False),
        _raw_headers(parsed_output, content=True),
    ) == expected_headers
    assert parsed_output.get_payload() == SELECTED_WIRE
    assert mime_text_execution.canonical_text_payload(parsed_output) == (
        b"PUBLIC SELECTED\nLINE\nTAIL"
    )
    assert parsed_output.preamble is None
    assert parsed_output.epilogue is None
    assert output.read_bytes() != raw
    assert second_result.selected_plain_text_bodies[0].path == ()
    assert repeated.read_bytes() == output.read_bytes()
    assert source.read_bytes() == raw


def test_existing_root_plain_message_remains_byte_exact(tmp_path: Path) -> None:
    """Never serialize an already-canonical root plain message."""
    raw = (
        b"From: sender@example.test\n"
        b"Received: first public hop\n"
        b"Received: second public hop\n"
        b"X-Public:  value with source spacing\n"
        b"Content-Type: text/plain; charset=utf-8\n"
        b"Content-Transfer-Encoding: 8bit\n"
        b"\n"
        b"PUBLIC ROOT BODY\n"
    )
    source = tmp_path / "root.eml"
    output = tmp_path / "copy.eml"
    source.write_bytes(raw)
    plan = mime_text_only._plan_text_only(_parse(raw))

    result = process_file(source, output, force=False, dry_run=False)

    assert not plan.modified
    assert plan.changed_paths == ()
    assert result.selected_plain_text_bodies[0].path == ()
    assert output.read_bytes() == raw
    assert source.read_bytes() == raw
