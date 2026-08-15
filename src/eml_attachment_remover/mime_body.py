"""Classify MIME text entities that can represent a rendered message body."""

from __future__ import annotations

from typing import TYPE_CHECKING, Final

if TYPE_CHECKING:
    from email.message import EmailMessage

BODY_TEXT_TYPES: Final = frozenset({"text/plain", "text/html"})
BODY_CONTAINER_TYPES: Final = frozenset({
    "multipart/alternative",
    "multipart/related",
})
ATTACHMENT_DISPOSITION: Final = "attachment"
INLINE_DISPOSITION: Final = "inline"


def _is_retained_body_text(
    part: EmailMessage,
    parent_type: str | None,
    *,
    under_related: bool,
    is_root: bool,
) -> bool:
    """Return whether one text entity is intrinsically retained as body content.

    Returns:
        ``True`` only when policy metadata or structure keeps the text entity.

    """
    if part.get_content_type() not in BODY_TEXT_TYPES:
        return False
    if is_root:
        return True
    disposition = part.get_content_disposition()
    if disposition == ATTACHMENT_DISPOSITION:
        return False
    if part.get_filename() is None:
        return True
    return (
        disposition == INLINE_DISPOSITION
        or under_related
        or parent_type in BODY_CONTAINER_TYPES
    )


def _replace_root_payload(message: EmailMessage, notice: str) -> None:
    """Replace a root attachment or empty multipart body with plain text."""
    message.clear_content()
    message.set_content(notice)
