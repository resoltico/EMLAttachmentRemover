"""Recognize MIME body branches that reduce unambiguously to plain text."""

from __future__ import annotations

from typing import TYPE_CHECKING, Final

from .mime_locations import CONTENT_LOCATION_HEADER
from .mime_references import (
    CONTENT_ID_HEADER,
    _is_email_message_list,
    _normalize_content_id_header,
)

if TYPE_CHECKING:
    from email.message import EmailMessage

TEXT_ONLY_CONTAINER_TYPES: Final = frozenset({
    "multipart/alternative",
    "multipart/mixed",
    "multipart/related",
})
# MIME parameter names are case-insensitive. Derive the canonical RFC token from its
# ASCII octets so mutation analysis changes semantics instead of generating equivalent
# case-only spellings.
RELATED_START_PARAMETER: Final = bytes((115, 116, 97, 114, 116)).decode()


def is_resource_free_plain(part: EmailMessage) -> bool:
    """Return whether one leaf is an unambiguous standalone plain-text body.

    Returns:
        ``True`` only for an undisposed, non-file, non-addressable plain leaf.

    """
    return (
        not part.is_multipart()
        and part.get_content_type() == "text/plain"
        and part.get_content_disposition() is None
        and part.get_filename() is None
        and part.get(CONTENT_ID_HEADER) is None
        and part.get(CONTENT_LOCATION_HEADER) is None
    )


def is_safe_body_container(part: EmailMessage) -> bool:
    """Return whether a selected body container has no file/resource identity.

    Returns:
        ``True`` when no disposition or filename makes the container file-like.

    """
    return part.get_content_disposition() is None and part.get_filename() is None


def related_root_index(
    part: EmailMessage,
    payload: list[EmailMessage],
) -> int | None:
    """Resolve one related root without raising during candidate probing.

    Returns:
        The unique direct-child index, or ``None`` when unresolved.

    """
    if not payload:
        return None
    start = _normalize_content_id_header(part.get_param(RELATED_START_PARAMETER))
    if start is None:
        return 0
    matches = [
        index
        for index, child in enumerate(payload)
        if _normalize_content_id_header(child.get(CONTENT_ID_HEADER)) == start
    ]
    return matches[0] if len(matches) == 1 else None


def can_select_plain(part: EmailMessage) -> bool:
    """Return whether one body branch deterministically reduces to plain text.

    Returns:
        ``True`` when structural analysis finds exactly one safe plain body.

    """
    if is_resource_free_plain(part):
        return True
    content_type = part.get_content_type()
    payload = part.get_payload()
    if (
        content_type not in TEXT_ONLY_CONTAINER_TYPES
        or not is_safe_body_container(part)
        or not _is_email_message_list(payload)
    ):
        return False
    if content_type == "multipart/alternative":
        return sum(is_resource_free_plain(child) for child in payload) == 1
    if content_type == "multipart/related":
        root_index = related_root_index(part, payload)
        return root_index is not None and can_select_plain(payload[root_index])
    return sum(can_select_plain(child) for child in payload) == 1
