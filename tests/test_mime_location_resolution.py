# ruff: file-ignore[private-member-access]
"""Exercise MIME base resolution used to protect rendered resources."""

from __future__ import annotations

import string
from email.message import EmailMessage
from typing import Final
from urllib.parse import urljoin

from hypothesis import example, given
from hypothesis import strategies as st

from eml_attachment_remover import mime_policy, mime_references
from eml_attachment_remover.models import KeepReason

PATH_SEGMENT: Final[st.SearchStrategy[str]] = st.text(
    alphabet=string.ascii_letters + string.digits + "-_",
    min_size=1,
    max_size=24,
)


def _resource(location: str, filename: str) -> EmailMessage:
    """Return an attachment labeled with one public Content-Location.

    Returns:
        The configured synthetic image part.

    """
    part = EmailMessage()
    part.set_content(b"public image", maintype="image", subtype="png")
    part.add_header("Content-Disposition", "attachment", filename=filename)
    part["Content-Location"] = location
    return part


def _remove(message: EmailMessage) -> tuple[list[str | None], list[str | None]]:
    """Apply reference-aware removal and return removed and retained names.

    Returns:
        Filenames removed and filenames retained by a body reference.

    """
    references = mime_references._collect_references(message)
    state = mime_policy._remove_attachments(message, references)
    retained = [
        part.filename
        for part in state.preserved_file_parts
        if part.reason is KeepReason.BODY_LOCATION_REFERENCE
    ]
    return [part.filename for part in state.removed], retained


@example(segment="a-0")
@given(segment=PATH_SEGMENT)
def test_nested_content_location_base_protects_only_exact_resource(
    segment: str,
) -> None:
    """Resolve an inherited and then nested MIME Content-Location base."""
    outer_base = f"https://public.example/{segment}/"
    nested_base = urljoin(outer_base, "nested/")
    relative_location = f"images/{segment}.png"

    message = EmailMessage()
    message.make_mixed()
    message["Content-Location"] = outer_base
    related = EmailMessage()
    related.make_related()
    related["Content-Location"] = "nested/"
    body = EmailMessage()
    body.set_content(f'<img src="{relative_location}">', subtype="html")
    related.attach(body)
    related.attach(
        _resource(urljoin(nested_base, relative_location), "referenced.png"),
    )
    related.attach(
        _resource(urljoin(nested_base, f"images/{segment}x.png"), "near.png"),
    )
    message.attach(related)

    assert _remove(message) == (["near.png"], ["referenced.png"])


def test_legacy_content_base_resolves_a_resource_location() -> None:
    """Honor received RFC 2110 Content-Base when resolving a resource label."""
    message = EmailMessage()
    message.set_content(
        '<img src="https://public.example/assets/hero.png">',
        subtype="html",
    )
    message.make_mixed()
    referenced = _resource("hero.png", "referenced.png")
    referenced["Content-Base"] = "https://public.example/assets/"
    message.attach(referenced)
    message.attach(_resource("herox.png", "near.png"))

    assert _remove(message) == (["near.png"], ["referenced.png"])


def test_ancestor_content_base_is_inherited_by_body_and_resource() -> None:
    """Honor a legacy Content-Base on a surrounding MIME container."""
    message = EmailMessage()
    message.make_mixed()
    message["Content-Base"] = "https://public.example/archive/"
    related = EmailMessage()
    related.make_related()
    body = EmailMessage()
    body.set_content('<img src="images/hero.png">', subtype="html")
    related.attach(body)
    related.attach(_resource("images/hero.png", "referenced.png"))
    related.attach(_resource("images/herox.png", "near.png"))
    message.attach(related)

    assert _remove(message) == (["near.png"], ["referenced.png"])


@example(segment="logo")
@given(segment=PATH_SEGMENT)
def test_equal_relative_labels_under_different_bases_do_not_match(
    segment: str,
) -> None:
    """Compare resolved resource identities instead of ambiguous raw labels."""
    location = f"{segment}.png"
    message = EmailMessage()
    message.make_related()
    message["Content-Location"] = "https://one.example.test/mail/"
    body = EmailMessage()
    body.set_content(f'<img src="{location}">', subtype="html")
    message.attach(body)
    collision = _resource(location, "collision.png")
    collision["Content-Base"] = "https://two.example.test/assets/"
    message.attach(collision)

    assert _remove(message) == (["collision.png"], [])
