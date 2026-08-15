"""Public synthetic helper values shared by direct implementation tests."""

from __future__ import annotations

from email.message import EmailMessage

from eml_attachment_remover import models


def empty_references() -> models.ReferenceIndex:
    """Return an empty resource-reference index.

    Returns:
        An index without Content-ID or Content-Location references.

    """
    return models.ReferenceIndex(frozenset(), frozenset())


def child_context(*, under_related: bool = False) -> models.PartContext:
    """Return context for a non-root MIME child.

    Returns:
        The requested MIME-tree context.

    """
    return models.PartContext(
        path=(0,),
        parent_type="multipart/mixed",
        under_related=under_related,
        references=empty_references(),
    )


def attachment(name: str = "public.bin") -> EmailMessage:
    """Return a public synthetic MIME attachment.

    Returns:
        A file-like MIME part with an attachment disposition.

    """
    part = EmailMessage()
    part["Content-Disposition"] = f'attachment; filename="{name}"'
    part.set_payload("PUBLIC")
    return part
