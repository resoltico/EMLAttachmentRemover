# ruff: file-ignore[private-member-access]
"""Close the final source-path planning and resolution mutation boundaries."""

from __future__ import annotations

from email.message import EmailMessage

import pytest

from eml_attachment_remover import mime_text_execution, mime_text_only


def test_part_resolution_rejects_a_leaf_and_the_exact_upper_bound() -> None:
    """Map both non-container and index-equals-length paths to one stable error."""
    plain = EmailMessage()
    plain.set_content("PUBLIC BODY")
    mixed = EmailMessage()
    mixed.make_mixed()
    mixed.attach(plain)

    for message, path in ((plain, (0,)), (mixed, (1,))):
        with pytest.raises(RuntimeError) as raised:
            mime_text_execution._resolve_part(message, path)

        assert str(raised.value) == (
            f"text-only plan path no longer resolves: {path[0] + 1}"
        )


def test_alternative_plan_retains_the_exact_discard_action_root() -> None:
    """Keep the unselected representation path in the immutable whole-tree plan."""
    message = EmailMessage()
    message.set_content("PUBLIC PLAIN BODY")
    message.add_alternative("<p>PUBLIC HTML BODY</p>", subtype="html")

    plan = mime_text_only._plan_text_only(message)

    assert plan.discard_paths == ((1,),)
    assert [record.path for record in plan.discarded_representations] == [(1,)]


def test_mixed_plan_retains_the_exact_attachment_action_root() -> None:
    """Keep ordinary attachment roots in the same immutable source-path plan."""
    message = EmailMessage()
    message.set_content("PUBLIC PLAIN BODY")
    message.add_attachment(
        b"PUBLIC ATTACHMENT",
        maintype="application",
        subtype="octet-stream",
        filename="public.bin",
    )

    plan = mime_text_only._plan_text_only(message)

    assert plan.discard_paths == ((1,),)
    assert [record.path for record in plan.removed_attachments] == [(1,)]
