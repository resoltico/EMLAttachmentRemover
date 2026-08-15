# ruff: file-ignore[private-member-access]
"""Ensure body references stay within their MIME aggregate scope."""

from __future__ import annotations

from email.message import EmailMessage

from eml_attachment_remover import mime_policy, mime_references


def _related(body: str, filename: str) -> EmailMessage:
    """Return a related aggregate with one explicit attachment resource.

    Returns:
        The configured synthetic multipart/related entity.

    """
    related = EmailMessage()
    related.make_related()
    html = EmailMessage()
    html.set_content(body, subtype="html")
    related.attach(html)
    resource = EmailMessage()
    resource.set_content(b"public image", maintype="image", subtype="png")
    resource.add_header("Content-Disposition", "attachment", filename=filename)
    resource["Content-Location"] = "shared.png"
    related.attach(resource)
    return related


def test_parallel_related_reference_cannot_retain_sibling_attachment() -> None:
    """Use each parallel related aggregate's body references only in that scope."""
    message = EmailMessage()
    message.make_mixed()
    message.attach(_related("<p>No image here</p>", "unreferenced.png"))
    message.attach(_related('<img src="shared.png">', "referenced.png"))

    references = mime_references._collect_references(message)
    state = mime_policy._remove_attachments(message, references)

    assert [part.filename for part in state.removed] == ["unreferenced.png"]
    assert [part.filename for part in state.preserved_file_parts] == ["referenced.png"]


def test_related_reference_cannot_retain_enclosing_sibling_attachment() -> None:
    """Prevent a nested aggregate's references from escaping to its parent scope."""
    message = EmailMessage()
    message.make_mixed()
    message.attach(_related('<img src="shared.png">', "related.png"))
    outer = EmailMessage()
    outer.set_content(b"public outer", maintype="image", subtype="png")
    outer.add_header(
        "Content-Disposition",
        "attachment",
        filename="outer.png",
    )
    outer["Content-Location"] = "shared.png"
    message.attach(outer)

    references = mime_references._collect_references(message)
    state = mime_policy._remove_attachments(message, references)

    assert [part.filename for part in state.removed] == ["outer.png"]
    assert [part.filename for part in state.preserved_file_parts] == ["related.png"]


def test_enclosing_reference_cannot_retain_nested_related_attachment() -> None:
    """Prevent a parent body reference from leaking into a nested aggregate."""
    message = EmailMessage()
    message.set_content('<img src="shared.png">', subtype="html")
    message.make_mixed()
    message.attach(_related("<p>No image here</p>", "related.png"))

    references = mime_references._collect_references(message)
    state = mime_policy._remove_attachments(message, references)

    assert [part.filename for part in state.removed] == ["related.png"]
