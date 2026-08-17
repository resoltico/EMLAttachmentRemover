"""Mutation-complete boundaries for deterministic plain-body selection."""

from __future__ import annotations

from email.message import EmailMessage

from eml_attachment_remover.mime_text_selection import (
    can_select_plain,
    is_safe_body_container,
    related_root_index,
)


def _plain() -> EmailMessage:
    """Return one safe standalone plain body.

    Returns:
        The configured body.

    """
    part = EmailMessage()
    part.set_content("PUBLIC PLAIN BODY")
    return part


def _html() -> EmailMessage:
    """Return one unselectable HTML leaf.

    Returns:
        The configured HTML entity.

    """
    part = EmailMessage()
    part.set_content("<p>PUBLIC HTML</p>", subtype="html")
    return part


def _container(kind: str, *children: EmailMessage) -> EmailMessage:
    """Return one populated multipart aggregate.

    Returns:
        The configured aggregate.

    """
    part = EmailMessage()
    getattr(part, f"make_{kind}")()
    for child in children:
        part.attach(child)
    return part


def test_safe_container_rejects_each_independent_file_identity() -> None:
    """Require both disposition and filename to be absent."""
    disposed = _container("mixed", _plain())
    disposed["Content-Disposition"] = "attachment"
    named = _container("mixed", _plain())
    named.set_param("name", "public.mime", header="Content-Type")

    assert not is_safe_body_container(disposed)
    assert not is_safe_body_container(named)


def test_selection_guard_rejects_unsupported_and_filelike_containers() -> None:
    """Keep every container-type, metadata, and payload guard independent."""
    unsupported = EmailMessage()
    unsupported.set_type("multipart/digest")
    unsupported.set_payload([_plain()])
    filelike = _container("mixed", _plain())
    filelike["Content-Disposition"] = "attachment"

    assert not can_select_plain(unsupported)
    assert not can_select_plain(filelike)


def test_alternative_counts_only_direct_resource_free_plain_children() -> None:
    """Do not apply mixed-container recursive candidate semantics to alternatives."""
    nested_plain = _container("mixed", _plain())
    alternative = _container("alternative", _plain(), nested_plain)

    assert can_select_plain(alternative)


def test_related_selection_uses_only_its_resolved_root() -> None:
    """Ignore selectable non-root siblings and require a selectable root."""
    unselectable_root = _container("related", _html(), _plain())
    selectable_root = _container("related", _plain(), _plain())

    assert not can_select_plain(unselectable_root)
    assert can_select_plain(selectable_root)


def test_related_start_parameter_selects_one_unique_nonfirst_root() -> None:
    """Read the exact start parameter and resolve its matching child."""
    first = _html()
    first["Content-ID"] = "<first@example.test>"
    second = _container("mixed", _plain())
    second["Content-ID"] = "<body@example.test>"
    related = _container("related", first, second)
    related.set_param("start", "<body@example.test>")
    raw_payload = related.get_payload()
    assert isinstance(raw_payload, list)
    payload = [child for child in raw_payload if isinstance(child, EmailMessage)]
    assert len(payload) == len(raw_payload)

    assert related_root_index(related, payload) == 1
    assert can_select_plain(related)

    duplicate_second = _container("mixed", _plain())
    duplicate_second["Content-ID"] = "<body@example.test>"
    duplicate = _container("related", second, duplicate_second)
    duplicate.set_param("start", "<body@example.test>")
    raw_duplicate_payload = duplicate.get_payload()
    assert isinstance(raw_duplicate_payload, list)
    duplicate_payload = [
        child for child in raw_duplicate_payload if isinstance(child, EmailMessage)
    ]
    assert len(duplicate_payload) == len(raw_duplicate_payload)
    assert related_root_index(duplicate, duplicate_payload) is None
    assert not can_select_plain(duplicate)
