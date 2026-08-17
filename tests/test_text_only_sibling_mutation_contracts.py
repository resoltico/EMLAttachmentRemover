# ruff: file-ignore[private-member-access]
"""Exact v2 mutation contracts for mixed-sibling classification."""

from __future__ import annotations

from email.message import EmailMessage
from unittest.mock import patch

import pytest

from eml_attachment_remover import mime_text_only
from eml_attachment_remover.mime_text_resources import ResourceScan, RootInclusion
from eml_attachment_remover.models import (
    CliError,
    DiscardedBodyResource,
    ReferenceIndex,
    SelectedPlainTextBody,
)


def _plain() -> EmailMessage:
    """Return one safe plain body.

    Returns:
        The configured plain-text leaf.

    """
    part = EmailMessage()
    part.set_content("PUBLIC PLAIN BODY")
    return part


def _binary() -> EmailMessage:
    """Return one anonymous binary leaf.

    Returns:
        The configured binary part.

    """
    part = EmailMessage()
    part.set_content(
        b"PUBLIC BINARY",
        maintype="application",
        subtype="octet-stream",
    )
    return part


def _seed_plan() -> mime_text_only.TextOnlyPlan:
    """Return one minimal immutable plan.

    Returns:
        The synthetic selected-body plan.

    """
    return mime_text_only.TextOnlyPlan(
        selected_body=SelectedPlainTextBody((9,), "text/plain"),
        selected_payload_sha256="0" * 64,
    )


def test_mixed_resource_scan_has_exact_boolean_and_base_contract() -> None:
    """Require a resource action and an exact non-optional scan configuration."""
    child = _binary()
    child["Content-Disposition"] = "inline"
    sources = (((3,), ReferenceIndex(frozenset(), frozenset())),)
    resource = DiscardedBodyResource(
        path=(3,),
        referenced_by=(),
        content_type="application/octet-stream",
        filename=None,
        disposition="inline",
    )
    seed = _seed_plan()
    with patch.object(
        mime_text_only,
        "_resource_leaves",
        return_value=(resource,),
    ) as leaves:
        plan = mime_text_only._plan_mixed_sibling(
            seed,
            child,
            (3,),
            "PUBLIC BASE",
            sources,
        )

    leaves.assert_called_once_with(
        child,
        (3,),
        ResourceScan(
            reference_sources=sources,
            inherited_base="PUBLIC BASE",
            include_text=True,
            root_inclusion=RootInclusion.INCLUDE,
        ),
    )
    assert plan.discarded_resources == (resource,)
    assert plan.discard_paths == ((3,),)


def test_mixed_sibling_attachment_and_identity_precedence_is_exact() -> None:
    """Distinguish disposition, filename, CID resource, and binary fallback."""
    disposed_text = _plain()
    disposed_text["Content-Disposition"] = "attachment"
    named_text = _plain()
    named_text.set_param("name", "public.txt", header="Content-Type")
    cid_binary = _binary()
    cid_binary["Content-ID"] = "<public@example.test>"
    anonymous_binary = _binary()
    seed = _seed_plan()

    disposed = mime_text_only._plan_mixed_sibling(
        seed,
        disposed_text,
        (0,),
        None,
        (),
    )
    named = mime_text_only._plan_mixed_sibling(
        seed,
        named_text,
        (1,),
        None,
        (),
    )
    cid = mime_text_only._plan_mixed_sibling(
        seed,
        cid_binary,
        (2,),
        None,
        (),
    )
    anonymous = mime_text_only._plan_mixed_sibling(
        seed,
        anonymous_binary,
        (3,),
        None,
        (),
    )

    assert [item.path for item in disposed.removed_attachments] == [(0,)]
    assert [item.path for item in named.removed_attachments] == [(1,)]
    assert [item.path for item in cid.discarded_resources] == [(2,)]
    assert not cid.removed_attachments
    assert [item.path for item in anonymous.removed_attachments] == [(3,)]


def test_ambiguous_message_multipart_and_protected_siblings_fail_exactly() -> None:
    """Never classify structured or protected unmarked siblings as binaries."""
    for content_type in (
        "message/rfc822",
        "multipart/mixed",
        "application/pkcs7-mime",
    ):
        child = EmailMessage()
        child.set_type(content_type)
        child.set_payload("PUBLIC AMBIGUOUS")

        with pytest.raises(CliError) as raised:
            mime_text_only._plan_mixed_sibling(
                _seed_plan(),
                child,
                (4,),
                None,
                (),
            )

        assert raised.value.message == (
            f"cannot produce a text-only EML: {content_type} has an ambiguous "
            "non-body role at MIME path 5"
        )
