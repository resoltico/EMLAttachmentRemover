# ruff: file-ignore[private-member-access]
"""Adversarial contracts for MIME traversal and protected-container policy."""

from __future__ import annotations

from email.message import EmailMessage

import pytest

from eml_attachment_remover import mime_body, mime_policy, mime_references
from eml_attachment_remover.models import (
    CliError,
    ExitCode,
    PartContext,
    ReferenceIndex,
    RemovalState,
)
from tests.test_internal_support import attachment, empty_references


def _mixed(*children: EmailMessage) -> EmailMessage:
    """Return a mixed container with the supplied public synthetic children.

    Returns:
        The configured multipart message.

    """
    message = EmailMessage()
    message.make_mixed()
    for child in children:
        message.attach(child)
    return message


def _context(
    *,
    parent_type: str = "multipart/alternative",
    related: bool = False,
    root: bool = False,
) -> PartContext:
    """Return a context with observable structural flags.

    Returns:
        The configured synthetic traversal context.

    """
    return PartContext(
        path=(2,),
        parent_type=parent_type,
        under_related=related,
        references=ReferenceIndex(frozenset(), frozenset({"public.png"})),
        location_base="https://public.example/root/",
        is_root=root,
    )


def test_container_context_preserves_every_field_and_rescopes_references() -> None:
    """Keep traversal identity while narrowing a related aggregate's references."""
    related = EmailMessage()
    related.make_related()
    body = EmailMessage()
    body.set_content('<img src="public.png">', subtype="html")
    related.attach(body)
    context = _context(related=True, root=True)

    narrowed = mime_policy._container_context(related, context)

    assert narrowed == PartContext(
        path=(2,),
        parent_type="multipart/alternative",
        under_related=True,
        references=ReferenceIndex(
            frozenset(),
            frozenset({
                "https://public.example/root/public.png",
            }),
        ),
        location_base="https://public.example/root/",
        is_root=True,
    )


def test_container_context_ignores_an_ordinary_multipart() -> None:
    """Return the original context object when no reference scope begins."""
    context = _context(related=True, root=True)

    assert mime_policy._container_context(_mixed(), context) is context


def test_message_container_context_starts_an_independent_reference_scope() -> None:
    """Rescope an encapsulated message even when it is not multipart/related."""
    nested = EmailMessage()
    nested["Content-Type"] = "message/rfc822"
    body = EmailMessage()
    body.set_content('<img src="nested.png">', subtype="html")
    nested.set_payload([body])

    narrowed = mime_policy._container_context(nested, _context())

    assert narrowed.references.locations == frozenset({
        "https://public.example/root/nested.png",
    })


def test_signed_guard_names_an_unnamed_attachment_at_the_exact_path() -> None:
    """Expose a stable refusal diagnostic when the signed file has no name."""
    unnamed = EmailMessage()
    unnamed["Content-Disposition"] = "attachment"
    signed = EmailMessage()
    signed["Content-Type"] = "multipart/signed"
    signed.set_payload([_mixed(EmailMessage(), unnamed), EmailMessage()])

    with pytest.raises(CliError) as raised:
        mime_policy._guard_signed_container(signed, _context())

    assert raised.value.code is ExitCode.PROTECTED_MESSAGE
    assert raised.value.message == (
        "attachment 'unnamed MIME entity' at MIME path 3.1.2 is inside "
        "multipart/signed content; refusing to invalidate the MIME signature"
    )


def test_signed_parent_is_recorded_and_never_traversed() -> None:
    """Stop before changing a signed parent that has no removable signed content."""
    signed = EmailMessage()
    signed["Content-Type"] = "multipart/signed"
    signed.set_payload([EmailMessage(), attachment("signature.asc")])
    state = RemovalState()

    assert mime_policy._handle_protected_parent(signed, _context(), state)
    assert state.protected_types == {"multipart/signed"}
    payload = signed.get_payload()
    assert isinstance(payload, list)
    signature = payload[1]
    assert isinstance(signature, EmailMessage)
    assert signature.get_filename() == "signature.asc"


@pytest.mark.parametrize(
    "content_type",
    [
        "multipart/encrypted",
        "application/pkcs7-mime",
        "application/x-pkcs7-mime",
        "application/pgp-encrypted",
    ],
)
def test_every_opaque_root_returns_without_removal(content_type: str) -> None:
    """Preserve every protected root before ordinary attachment policy runs."""
    message = EmailMessage()
    message["Content-Type"] = content_type
    message["Content-Disposition"] = 'attachment; filename="protected.bin"'
    message.set_payload("PUBLIC PROTECTED PAYLOAD")

    state = mime_policy._remove_attachments(message, empty_references())

    assert state.removed == []
    assert state.protected_types == {content_type}
    assert message.get_payload() == "PUBLIC PROTECTED PAYLOAD"


def test_root_context_flags_control_root_attachment_classification() -> None:
    """Treat a named root body as body while removing the same nested entity."""
    body = EmailMessage()
    body.set_content("public body")
    body.add_header("Content-Disposition", "inline", filename="body.txt")

    assert not mime_policy._should_remove(body, _context(root=True))
    assert not mime_policy._root_is_removable(body, empty_references())
    body.replace_header("Content-Disposition", 'attachment; filename="body.txt"')
    assert mime_policy._root_is_removable(body, empty_references())


def test_under_related_flag_retains_a_named_body_without_disposition() -> None:
    """Distinguish a related body file from an ordinary named text attachment."""
    body = EmailMessage()
    body.set_content('<img src="public.png">', subtype="html")
    body.add_header("Content-Disposition", "filename=body.html")

    assert not mime_policy._should_remove(body, _context(related=True))
    assert mime_policy._should_remove(
        body,
        _context(parent_type="multipart/mixed", related=False),
    )


def test_body_eligibility_accepts_each_independent_structural_reason() -> None:
    """Accept inline, related, and alternative named text for distinct reasons."""
    inline = EmailMessage()
    inline.set_content("inline")
    inline.add_header("Content-Disposition", "inline", filename="inline.txt")
    related = EmailMessage()
    related.set_content("related")
    related.set_param("name", "related.txt", header="Content-Type")

    assert mime_body._is_retained_body_text(
        inline,
        "multipart/mixed",
        under_related=False,
        is_root=False,
    )
    assert mime_body._is_retained_body_text(
        related,
        "multipart/mixed",
        under_related=True,
        is_root=False,
    )
    assert mime_body._is_retained_body_text(
        related,
        "multipart/alternative",
        under_related=False,
        is_root=False,
    )


def test_preserved_root_file_part_is_recorded_in_the_returned_state() -> None:
    """Pass the live removal state when recording a retained inline root."""
    message = EmailMessage()
    message.set_content(b"PUBLIC", maintype="application", subtype="octet-stream")
    message.add_header("Content-Disposition", "inline", filename="public.bin")

    state = mime_policy._remove_attachments(message, empty_references())

    assert [part.filename for part in state.preserved_file_parts] == ["public.bin"]


def test_remove_children_removes_all_siblings_and_keeps_order() -> None:
    """Continue after each removal and preserve every ordinary sibling in order."""
    first = EmailMessage()
    first.set_content("first")
    second = EmailMessage()
    second.set_content("second")
    message = _mixed(
        attachment("one.bin"),
        first,
        attachment("two.bin"),
        second,
    )
    state = RemovalState()

    assert mime_policy._remove_children(message, _context(root=True), state)
    assert [part.filename for part in state.removed] == ["one.bin", "two.bin"]
    assert [part.get_content() for part in message.iter_parts()] == [
        "first\n",
        "second\n",
    ]


def test_unchanged_container_returns_exact_false_and_keeps_digest() -> None:
    """Do not rewrite or clear a digest when no descendant is removed."""
    body = EmailMessage()
    body.set_content("public")
    message = _mixed(body)
    message["Content-MD5"] = "public-digest"

    modified = mime_policy._remove_children(
        message, _context(root=True), RemovalState()
    )

    assert modified is False
    assert message["Content-MD5"] == "public-digest"


def test_empty_child_container_is_pruned_without_skipping_later_siblings() -> None:
    """Prune an emptied child and continue removing a following attachment."""
    emptied = _mixed(attachment("nested.bin"))
    body = EmailMessage()
    body.set_content("public")
    message = _mixed(emptied, attachment("later.bin"), body)
    state = RemovalState()

    assert mime_policy._remove_children(message, _context(root=True), state)
    assert [part.filename for part in state.removed] == ["nested.bin", "later.bin"]
    assert list(message.iter_parts()) == [body]


def test_outer_removal_refuses_reserialization_with_nested_signed_content() -> None:
    """Apply the stable all-or-nothing signature diagnostic after traversal."""
    signed = EmailMessage()
    signed["Content-Type"] = "multipart/signed"
    signed.set_payload([EmailMessage(), EmailMessage()])
    message = _mixed(attachment("ordinary.bin"), signed)

    with pytest.raises(CliError) as raised:
        mime_policy._remove_attachments(
            message,
            mime_references._collect_references(message),
        )

    assert raised.value.code is ExitCode.PROTECTED_MESSAGE
    assert raised.value.message == (
        "message contains multipart/signed content and removable attachments; "
        "refusing to reserialize because the MIME signature could be invalidated"
    )
