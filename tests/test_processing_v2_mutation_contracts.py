# ruff: file-ignore[private-member-access]
"""Bind stable source paths and warning order in v2 processing."""

from __future__ import annotations

from email.message import EmailMessage

from eml_attachment_remover import processing


def _multipart_inventory() -> EmailMessage:
    """Return a nested tree with mixed and logical-message parent types.

    Returns:
        The populated synthetic tree.

    """
    root = EmailMessage()
    root.make_mixed()
    plain = EmailMessage()
    plain.set_content("PUBLIC OUTER BODY")
    inner = EmailMessage()
    inner.set_content("PUBLIC INNER BODY")
    wrapper = EmailMessage()
    wrapper.set_type("message/rfc822")
    wrapper.set_payload([inner])
    root.attach(plain)
    root.attach(wrapper)
    return root


def test_located_parts_bind_every_path_and_direct_parent_type() -> None:
    """Keep immutable source paths and logical parent media types together."""
    message = _multipart_inventory()

    located = processing._located_parts(message)

    assert [(path, parent) for path, parent, _part in located] == [
        ((), None),
        ((0,), "multipart/mixed"),
        ((1,), "multipart/mixed"),
        ((1, 0), "message/rfc822"),
    ]


def _signed_part(signature: str) -> EmailMessage:
    """Return one stale transport-signed MIME entity.

    Returns:
        The configured synthetic entity.

    """
    part = EmailMessage()
    part["DKIM-Signature"] = signature
    part["Content-Length"] = "999"
    return part


def test_removal_warnings_continue_past_irrelevant_parts_and_bind_nested_path() -> None:
    """Preserve parser order while warning only for lowercase logical parents."""
    unchanged = _signed_part("PUBLIC UNCHANGED")
    changed_nonlogical = _signed_part("PUBLIC NONLOGICAL")
    changed_nested = _signed_part("PUBLIC NESTED")
    uppercase_parent = _signed_part("PUBLIC UPPERCASE DECOY")
    located = (
        ((0,), "multipart/mixed", unchanged),
        ((1,), "multipart/mixed", changed_nonlogical),
        ((2, 0), "message/rfc822", changed_nested),
        ((3, 0), "MESSAGE/RFC822", uppercase_parent),
    )

    warnings = processing._removal_warnings(
        located,
        ("PUBLIC PARSER WARNING",),
        ((1,), (2, 0), (3, 0)),
    )

    assert warnings == [
        "PUBLIC PARSER WARNING",
        (
            "rewriting nested message at MIME path 3.1 invalidates existing "
            "transport signatures (DKIM-Signature); the original EML remains "
            "unchanged"
        ),
    ]
    assert unchanged["Content-Length"] == "999"
    assert changed_nonlogical["Content-Length"] is None
    assert changed_nested["Content-Length"] is None
    assert uppercase_parent["Content-Length"] is None
