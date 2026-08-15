"""Exact direct contracts for MIME policy, reference, and verification behavior."""

from __future__ import annotations

import unittest
from email.message import EmailMessage
from unittest.mock import MagicMock, patch

import pytest

from eml_attachment_remover import (
    mime_policy,
    mime_references,
)
from eml_attachment_remover.models import (
    CliError,
    ExitCode,
    PartContext,
    ReferenceIndex,
    RemovalState,
    RemovedPart,
)
from tests.test_internal_support import attachment, child_context, empty_references


def _root_context() -> PartContext:
    """Return an empty-reference root context.

    Returns:
        A root traversal context without body references.

    """
    return PartContext(
        path=(),
        parent_type=None,
        under_related=False,
        references=empty_references(),
        is_root=True,
    )


def _mixed(*children: EmailMessage) -> EmailMessage:
    """Return a multipart/mixed message containing the supplied children.

    Returns:
        A new multipart message with the children in their supplied order.

    """
    message = EmailMessage()
    message.make_mixed()
    for child in children:
        message.attach(child)
    return message


class TestReferenceContracts(unittest.TestCase):
    """Specify normalization, decoding, extraction, and subtree isolation."""

    def test_content_id_normalization_preserves_raw_percent_syntax(self) -> None:
        assert (
            mime_references._normalize_content_id_header(
                "  <Public%40Example.test>  ",
            )
            == "Public%40Example.test"
        )

    def test_html_location_decodes_entities_but_preserves_uri_quotes(self) -> None:
        references = mime_references._references_from_html(
            "<img src=\"  'Public%20Logo&amp;x=1'  \">",
        )

        assert references.locations == frozenset({"'Public%20Logo&x=1'"})

    def test_text_decoder_honors_the_declared_charset(self) -> None:
        part = MagicMock()
        part.get_payload.return_value = b"caf\xe9"
        part.get_content_charset.return_value = "iso-8859-1"

        assert mime_references._decode_text_payload(part) == "café"

    def test_text_decoder_preserves_surrogate_bytes_from_string_payload(self) -> None:
        part = MagicMock()
        part.get_payload.side_effect = [None, "public\udcff"]
        part.get_content_charset.return_value = "utf-8"

        assert mime_references._decode_text_payload(part) == "public�"

    def test_unknown_charset_fallback_replaces_invalid_utf8(self) -> None:
        part = MagicMock()
        part.get_payload.return_value = b"public\xff"
        part.get_content_charset.return_value = "unknown-public-charset"

        assert mime_references._decode_text_payload(part) == "public�"

    def test_html_extraction_continues_after_a_cid_attribute(self) -> None:
        references = mime_references._references_from_html(
            '<img src="cid:public@example.test"><img src="Public%20Logo.png">',
        )

        assert references == ReferenceIndex(
            frozenset({"public@example.test"}),
            frozenset({"Public%20Logo.png"}),
        )

    def test_attachment_html_cannot_contribute_body_references(self) -> None:
        body = EmailMessage()
        body.set_content('<img src="body.png">', subtype="html")
        attached_html = EmailMessage()
        attached_html.set_content('<img src="private.png">', subtype="html")
        attached_html["Content-Disposition"] = 'attachment; filename="page.html"'

        assert mime_references._collect_references(
            _mixed(body, attached_html),
        ) == ReferenceIndex(frozenset(), frozenset({"body.png"}))

    def test_root_html_with_attachment_disposition_is_still_body_content(self) -> None:
        message = EmailMessage()
        message.set_content('<img src="root.png">', subtype="html")
        message["Content-Disposition"] = 'attachment; filename="root.html"'

        assert mime_references._collect_references(message) == ReferenceIndex(
            frozenset(),
            frozenset({"root.png"}),
        )


class TestPolicyContracts(unittest.TestCase):
    """Specify exact removal decisions, traversal state, and notices."""

    def test_attachment_disposition_is_removable(self) -> None:
        assert mime_policy._should_remove(attachment(), child_context())

    def test_alternative_text_filename_is_body_content(self) -> None:
        part = EmailMessage()
        part.set_content("public body")
        part.add_header("Content-Disposition", "inline", filename="body.txt")
        context = PartContext(
            path=(1,),
            parent_type="multipart/alternative",
            under_related=False,
            references=empty_references(),
        )

        assert not mime_policy._should_remove(part, context)

    def test_policy_root_context_has_exact_root_semantics(self) -> None:
        references = ReferenceIndex(frozenset({"public"}), frozenset())

        assert mime_policy._root_context(references) == PartContext(
            path=(),
            parent_type=None,
            under_related=False,
            references=references,
            is_root=True,
        )

    def test_named_nontext_root_is_removable_but_named_text_is_not(self) -> None:
        binary = EmailMessage()
        binary["Content-Type"] = "application/octet-stream"
        binary["Content-Disposition"] = 'inline; filename="public.bin"'
        text = EmailMessage()
        text.set_content("body")
        text["Content-Disposition"] = 'inline; filename="body.txt"'

        assert not mime_policy._root_is_removable(binary, empty_references())
        del binary["Content-Disposition"]
        binary["Content-Disposition"] = 'filename="public.bin"'
        assert mime_policy._root_is_removable(binary, empty_references())
        assert not mime_policy._root_is_removable(text, empty_references())

    def test_first_nested_removal_reports_the_exact_path(self) -> None:
        nested = _mixed(EmailMessage(), attachment("nested.bin"))
        message = _mixed(EmailMessage(), nested)

        assert mime_policy._first_removable_in_subtree(
            message,
            _root_context(),
        ) == RemovedPart((1, 1), "text/plain", "nested.bin", "attachment")

    def test_encrypted_subtree_is_not_searched_for_removals(self) -> None:
        encrypted = EmailMessage()
        encrypted["Content-Type"] = "multipart/encrypted"
        encrypted.set_payload([attachment("protected.bin")])

        assert (
            mime_policy._first_removable_in_subtree(encrypted, child_context()) is None
        )

    def test_signed_guard_reports_exact_attachment_and_path(self) -> None:
        signed = EmailMessage()
        signed["Content-Type"] = "multipart/signed"
        signed.set_payload([_mixed(EmailMessage(), attachment("signed.bin"))])

        with pytest.raises(CliError) as raised:
            mime_policy._guard_signed_container(signed, child_context())

        assert raised.value.code is ExitCode.PROTECTED_MESSAGE
        assert raised.value.message == (
            "attachment 'signed.bin' at MIME path 1.1.2 is inside multipart/signed "
            "content; refusing to invalidate the MIME signature"
        )

    def test_digest_cleanup_removes_duplicate_headers(self) -> None:
        message = EmailMessage()
        message["Content-MD5"] = "first"
        message["Content-MD5"] = "second"

        mime_policy._clear_content_digest(message)

        assert message.get_all("Content-MD5") is None

    def test_protected_parent_records_exact_types_and_stops_traversal(self) -> None:
        state = RemovalState()
        encrypted = EmailMessage()
        encrypted["Content-Type"] = "multipart/encrypted"
        encrypted.set_payload([])

        assert mime_policy._handle_protected_parent(
            encrypted,
            child_context(),
            state,
        )
        assert state.protected_types == {"multipart/encrypted"}

    def test_nonmultipart_removal_never_calls_protected_container_handler(
        self,
    ) -> None:
        with patch.object(
            mime_policy,
            "_handle_protected_parent",
            side_effect=AssertionError("must not inspect a leaf as a container"),
        ):
            assert not mime_policy._remove_children(
                EmailMessage(),
                child_context(),
                RemovalState(),
            )

    def test_attachment_only_root_uses_the_exact_public_notice(self) -> None:
        message = _mixed(attachment())

        state = mime_policy._remove_attachments(message, empty_references())

        assert [part.path for part in state.removed] == [(0,)]
        assert message.get_content().rstrip("\n") == (
            "[All MIME body parts were attachments and were removed from this "
            "derived EML copy.]"
        )

    def test_opaque_security_root_is_preserved_without_traversal(self) -> None:
        message = EmailMessage()
        message["Content-Type"] = "application/pkcs7-mime"
        message.set_payload("public protected payload")

        state = mime_policy._remove_attachments(message, empty_references())

        assert state.removed == []
        assert state.protected_types == {"application/pkcs7-mime"}
