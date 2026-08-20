"""Execute and verify a source-bound plain-text MIME transformation plan."""

from __future__ import annotations

import hashlib
from typing import TYPE_CHECKING

from .mime_references import _is_email_message_list
from .models import CliError, ExitCode, MimePath, _format_mime_path

if TYPE_CHECKING:
    from email.message import EmailMessage

    from .mime_text_plan import TextOnlyPlan


def canonical_text_payload(part: EmailMessage) -> bytes:
    """Return decoded text bytes with MIME transport newlines normalized.

    Returns:
        The decoded payload with CRLF and CR represented as LF.

    """
    decoded = part.get_payload(decode=True)
    if isinstance(decoded, bytes):
        payload = decoded
    else:
        raw_payload = part.get_payload()
        if isinstance(raw_payload, bytes):
            payload = raw_payload
        elif isinstance(raw_payload, str):
            payload = raw_payload.encode(errors="surrogateescape")
        else:
            payload = b""
    return payload.replace(b"\r\n", b"\n").replace(b"\r", b"\n")


def _verify_plan_binding(message: EmailMessage, plan: TextOnlyPlan) -> None:
    """Require one retained plain payload to match the planned output digest.

    Raises:
        CliError: If execution changed or duplicated the selected body.

    """
    payloads = [
        canonical_text_payload(part)
        for part in message.walk()
        if not part.is_multipart() and part.get_content_type() == "text/plain"
    ]
    digests = tuple(hashlib.sha256(payload).hexdigest() for payload in payloads)
    expected_payload = plan.projection.expected_payload_sha256
    if digests != (expected_payload,):
        detail = (
            "text-only execution changed the selected plain-text body"
            if plan.rendered_text is None
            else "text-only execution did not produce the planned text body"
        )
        raise CliError(
            ExitCode.VERIFICATION_ERROR,
            detail,
        )


def _resolve_part(message: EmailMessage, path: MimePath) -> EmailMessage:
    """Resolve one immutable source path before any mutation.

    Returns:
        The exact source-tree MIME entity.

    Raises:
        RuntimeError: If the path no longer resolves.

    """
    part = message
    for index in path:
        payload = part.get_payload()
        if not _is_email_message_list(payload) or index < 0 or index >= len(payload):
            raise RuntimeError(
                "text-only plan path no longer resolves: " + _format_mime_path(path),
            )
        part = payload[index]
    return part


def _verify_source_binding(selected: EmailMessage, plan: TextOnlyPlan) -> None:
    """Require the selected plain source to match the immutable plan.

    Raises:
        CliError: If the source leaf differs from the planned payload.

    """
    if hashlib.sha256(canonical_text_payload(selected)).hexdigest() != (
        plan.selected_payload_sha256
    ):
        raise CliError(
            ExitCode.VERIFICATION_ERROR,
            "text-only execution changed the selected plain-text source",
        )


def _promote_plain_body(
    message: EmailMessage,
    selected: EmailMessage,
    rendered_text: str | None,
) -> None:
    """Replace root content metadata and payload with the selected plain body."""
    root_content_headers = tuple(
        name
        for name, _value in message.raw_items()
        if name.casefold().startswith("content-")
    )
    for name in dict.fromkeys(root_content_headers):
        del message[name]
    if rendered_text is None:
        selected_headers = tuple(
            (name, value)
            for name, value in selected.raw_items()
            if name.casefold().startswith("content-")
        )
        for name, value in selected_headers:
            message[name] = value
        message.set_payload(selected.get_payload())
    else:
        message.set_content(
            rendered_text,
            cte="quoted-printable",
        )
    message.preamble = None
    message.epilogue = None


def execute_text_only_plan(message: EmailMessage, plan: TextOnlyPlan) -> None:
    """Apply one complete analyzed plan, then verify its source precondition."""
    selected = _resolve_part(message, plan.selected_body.path)
    for discard_path in plan.discard_paths:
        _resolve_part(message, discard_path)
    _verify_source_binding(selected, plan)
    if plan.selected_body.path:
        _promote_plain_body(message, selected, plan.rendered_text)
    _verify_plan_binding(message, plan)
