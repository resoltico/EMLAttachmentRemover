# ruff: file-ignore[private-member-access]
"""Keep resource references isolated at encapsulated-message boundaries."""

from __future__ import annotations

from email.message import EmailMessage

import pytest

from eml_attachment_remover import mime_policy, mime_references


def _attachment(filename: str, cid: str) -> EmailMessage:
    """Return one explicit attachment labeled by Content-ID.

    Returns:
        The configured synthetic image.

    """
    part = EmailMessage()
    part.set_content(b"public image", maintype="image", subtype="png")
    part.add_header("Content-Disposition", "attachment", filename=filename)
    part["Content-ID"] = f"<{cid}>"
    return part


def _encapsulated(subtype: str, reference: str, resource_cid: str) -> EmailMessage:
    """Return an inline message container with one HTML body and resource.

    Returns:
        A message-maintype MIME scope represented by a payload list.

    """
    inner = EmailMessage()
    inner.set_content(f'<img src="cid:{reference}">', subtype="html")
    inner.make_mixed()
    inner.attach(_attachment("inner.png", resource_cid))
    container = EmailMessage()
    container.set_type(f"message/{subtype}")
    container.set_payload([inner])
    return container


@pytest.mark.parametrize("subtype", ["rfc822", "global"])
def test_nested_message_reference_cannot_retain_outer_resource(subtype: str) -> None:
    """Prevent references inside an inline encapsulated message from leaking out."""
    message = EmailMessage()
    message.set_content("Outer public body")
    message.make_mixed()
    message.attach(_attachment("outer.png", "shared@example.test"))
    message.attach(
        _encapsulated(subtype, "shared@example.test", "unreferenced@example.test"),
    )

    state = mime_policy._remove_attachments(
        message,
        mime_references._collect_references(message),
    )

    assert [part.filename for part in state.removed] == ["outer.png", "inner.png"]


@pytest.mark.parametrize("subtype", ["rfc822", "global"])
def test_outer_reference_cannot_retain_nested_message_resource(subtype: str) -> None:
    """Prevent an outer body reference from leaking into an inline inner message."""
    message = EmailMessage()
    message.set_content('<img src="cid:shared@example.test">', subtype="html")
    message.make_mixed()
    message.attach(
        _encapsulated(subtype, "other@example.test", "shared@example.test"),
    )

    state = mime_policy._remove_attachments(
        message,
        mime_references._collect_references(message),
    )

    assert [part.filename for part in state.removed] == ["inner.png"]


@pytest.mark.parametrize("subtype", ["rfc822", "global"])
def test_nested_message_uses_its_own_reference_index(subtype: str) -> None:
    """Retain an inner resource when the inner body itself references it."""
    message = EmailMessage()
    message.set_content("Outer public body")
    message.make_mixed()
    message.attach(
        _encapsulated(subtype, "inner@example.test", "inner@example.test"),
    )

    state = mime_policy._remove_attachments(
        message,
        mime_references._collect_references(message),
    )

    assert not state.removed
    assert [part.filename for part in state.preserved_file_parts] == ["inner.png"]
