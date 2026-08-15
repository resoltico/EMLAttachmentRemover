"""Direct tests for defensive MIME processing branches."""

from __future__ import annotations

import tempfile
import unittest
from collections import Counter
from email import errors
from email.message import EmailMessage
from email.parser import BytesParser
from pathlib import Path
from unittest.mock import MagicMock, patch

from eml_attachment_remover import (
    mime_locations,
    mime_policy,
    mime_references,
    mime_serialization,
    models,
    storage,
)
from tests.test_internal_support import attachment, child_context, empty_references


class MimeReferenceEdgeCaseTests(unittest.TestCase):
    """Exercise one normalization or payload-decoding contract per test."""

    def test_content_id_header_normalization_handles_brackets(self) -> None:
        self.assertEqual(
            mime_references._normalize_content_id_header(
                "<PUBLIC@EXAMPLE.TEST>",
            ),
            "PUBLIC@EXAMPLE.TEST",
        )

    def test_empty_location_normalizes_to_none(self) -> None:
        self.assertIsNone(
            mime_locations._normalize_content_location_header("  "),
        )

    def test_empty_html_uri_is_not_indexed(self) -> None:
        self.assertEqual(
            mime_references._references_from_html(
                '<img src=""><a href="public.css">',
            ),
            models.ReferenceIndex(frozenset(), frozenset()),
        )

    def test_unknown_charset_falls_back_to_utf8(self) -> None:
        part = MagicMock()
        part.get_payload.side_effect = [None, b"bytes"]
        part.get_content_charset.return_value = "unknown-charset"

        self.assertEqual(mime_references._decode_text_payload(part), "bytes")

    def test_string_payload_is_decoded(self) -> None:
        part = MagicMock()
        part.get_payload.side_effect = [None, "public text"]
        part.get_content_charset.return_value = "utf-8"

        self.assertEqual(mime_references._decode_text_payload(part), "public text")

    def test_nontext_payload_decodes_to_empty_string(self) -> None:
        part = MagicMock()
        part.get_payload.side_effect = [None, []]
        part.get_content_charset.return_value = "utf-8"

        self.assertEqual(mime_references._decode_text_payload(part), "")


class MimePolicyEdgeCaseTests(unittest.TestCase):
    """Exercise one MIME retention or traversal contract per test."""

    def test_inline_disposition_is_a_retention_reason(self) -> None:
        part = EmailMessage()
        part["Content-Disposition"] = "inline"

        self.assertIs(
            mime_policy._keep_reason(part, child_context()),
            models.KeepReason.INLINE_DISPOSITION,
        )

    def test_content_id_is_a_retention_reason(self) -> None:
        part = EmailMessage()
        part["Content-ID"] = "<public@example.test>"

        self.assertIs(
            mime_policy._keep_reason(part, child_context()),
            models.KeepReason.CONTENT_ID,
        )

    def test_content_location_is_a_retention_reason(self) -> None:
        part = EmailMessage()
        part["Content-Location"] = "public.png"

        self.assertIs(
            mime_policy._keep_reason(part, child_context()),
            models.KeepReason.CONTENT_LOCATION,
        )

    def test_inline_root_is_not_removable(self) -> None:
        part = EmailMessage()
        part["Content-Disposition"] = "inline"

        self.assertFalse(mime_policy._root_is_removable(part, empty_references()))

    def test_preserved_security_entity_records_protected_type(self) -> None:
        part = EmailMessage()
        part["Content-Type"] = "application/pgp-encrypted; name=public.asc"
        state = models.RemovalState()

        mime_policy._record_preserved_file_part(part, child_context(), state)

        self.assertEqual(state.protected_types, {"application/pgp-encrypted"})

    def test_malformed_multipart_has_no_removable_descendant(self) -> None:
        malformed = MagicMock()
        malformed.is_multipart.return_value = True
        malformed.get_payload.return_value = "not a child list"

        self.assertIsNone(
            mime_policy._first_removable_in_subtree(malformed, child_context()),
        )

    def test_malformed_multipart_is_not_modified(self) -> None:
        malformed = MagicMock()
        malformed.is_multipart.return_value = True
        malformed.get_payload.return_value = "not a child list"

        self.assertFalse(
            mime_policy._remove_children(
                malformed,
                child_context(),
                models.RemovalState(),
            ),
        )

    def test_signature_part_is_not_scanned_for_removals(self) -> None:
        signed = EmailMessage()
        signed["Content-Type"] = "multipart/signed"
        signed.set_payload([EmailMessage(), attachment()])

        self.assertIsNone(
            mime_policy._first_removable_in_subtree(signed, child_context()),
        )

    def test_empty_signed_container_guard_is_a_noop(self) -> None:
        signed = EmailMessage()
        signed["Content-Type"] = "multipart/signed"
        signed.set_payload([])

        mime_policy._guard_signed_container(signed, child_context())

        self.assertEqual(signed.get_payload(), [])

    def test_empty_signed_root_is_recorded_as_protected(self) -> None:
        signed = EmailMessage()
        signed["Content-Type"] = "multipart/signed"
        signed.set_payload([])

        state = mime_policy._remove_attachments(signed, empty_references())

        self.assertIn("multipart/signed", state.protected_types)

    def test_content_digest_is_removed_after_payload_change(self) -> None:
        message = EmailMessage()
        message["Content-MD5"] = "obsolete"

        mime_policy._clear_content_digest(message)

        self.assertNotIn("Content-MD5", message)

    def test_empty_nested_multipart_is_pruned(self) -> None:
        root = EmailMessage()
        root.make_mixed()
        nested = EmailMessage()
        nested.make_mixed()
        nested.attach(attachment())
        root.attach(nested)
        state = models.RemovalState()

        modified = mime_policy._remove_children(root, child_context(), state)

        self.assertTrue(modified)
        self.assertEqual(root.get_payload(), [])

    def test_attachment_only_root_becomes_plain_text_notice(self) -> None:
        message = EmailMessage()
        message.make_mixed()
        message.attach(attachment())

        mime_policy._remove_attachments(message, empty_references())

        self.assertEqual(message.get_content_type(), "text/plain")


class MimeSerializationEdgeCaseTests(unittest.TestCase):
    """Exercise one serialization or verification failure contract per test."""

    def test_payload_bytes_accepts_decoded_bytes(self) -> None:
        part = MagicMock()
        part.get_payload.side_effect = [None, b"public"]

        self.assertEqual(mime_serialization._payload_bytes(part), b"public")

    def test_payload_bytes_encodes_string_payload(self) -> None:
        part = MagicMock()
        part.get_payload.side_effect = [None, "public"]

        self.assertEqual(mime_serialization._payload_bytes(part), b"public")

    def test_payload_bytes_uses_empty_fallback(self) -> None:
        part = MagicMock()
        part.get_payload.side_effect = [None, []]

        self.assertEqual(mime_serialization._payload_bytes(part), b"")

    def test_safe_parser_defect_becomes_warning(self) -> None:
        message = EmailMessage()
        message.defects.append(errors.InvalidHeaderDefect("public warning"))

        warnings = mime_serialization._validate_defects(message)

        self.assertEqual(len(warnings), 1)

    def test_parse_value_error_becomes_parse_error(self) -> None:
        with patch.object(BytesParser, "parsebytes", side_effect=ValueError("bad")):
            with self.assertRaises(models.CliError) as raised:
                mime_serialization._parse_message(b"public", "fixture")

        self.assertEqual(raised.exception.code, models.ExitCode.PARSE_ERROR)

    def test_every_unsafe_defect_type_is_rejected(self) -> None:
        for defect_type in mime_serialization.UNSAFE_DEFECT_TYPES:
            with self.subTest(defect_type=defect_type.__name__):
                message = EmailMessage()
                message.defects.append(defect_type("public malformed MIME"))
                with self.assertRaises(models.CliError) as raised:
                    mime_serialization._validate_defects(message)
                self.assertEqual(raised.exception.code, models.ExitCode.PARSE_ERROR)

    def test_temporary_creation_error_is_write_error(self) -> None:
        with tempfile.TemporaryDirectory() as directory:
            destination = Path(directory) / "output.eml"
            with patch.object(tempfile, "mkstemp", side_effect=OSError("blocked")):
                with self.assertRaises(models.CliError) as raised:
                    storage._write_temporary(
                        destination,
                        EmailMessage(),
                        b"public",
                        modified=False,
                    )

        self.assertEqual(raised.exception.code, models.ExitCode.WRITE_ERROR)

    def test_temporary_write_error_is_write_error(self) -> None:
        with tempfile.TemporaryDirectory() as directory:
            destination = Path(directory) / "output.eml"
            with patch.object(
                storage,
                "_write_open_temporary",
                side_effect=OSError("blocked"),
            ):
                with self.assertRaises(models.CliError) as raised:
                    storage._write_temporary(
                        destination,
                        EmailMessage(),
                        b"public",
                        modified=False,
                    )

        self.assertEqual(raised.exception.code, models.ExitCode.WRITE_ERROR)

    def test_serialized_parse_error_becomes_verification_error(self) -> None:
        with tempfile.TemporaryDirectory() as directory:
            temporary = Path(directory) / "temporary.eml"
            temporary.write_bytes(b"public")
            parse_error = models.CliError(models.ExitCode.PARSE_ERROR, "bad fixture")
            with patch.object(
                mime_serialization,
                "_parse_message",
                side_effect=parse_error,
            ):
                with self.assertRaises(models.CliError) as raised:
                    mime_serialization._verify_serialized_message(
                        temporary,
                        Counter(),
                    )

        self.assertEqual(raised.exception.code, models.ExitCode.VERIFICATION_ERROR)

    def test_payload_fingerprint_mismatch_is_verification_error(self) -> None:
        with tempfile.TemporaryDirectory() as directory:
            temporary = Path(directory) / "temporary.eml"
            temporary.write_bytes(b"public")
            with (
                patch.object(
                    mime_serialization,
                    "_parse_message",
                    return_value=(EmailMessage(), ()),
                ),
                patch.object(
                    mime_serialization,
                    "_leaf_fingerprints",
                    return_value=Counter({("x",): 1}),
                ),
            ):
                with self.assertRaises(models.CliError) as raised:
                    mime_serialization._verify_serialized_message(
                        temporary,
                        Counter(),
                    )

        self.assertEqual(raised.exception.code, models.ExitCode.VERIFICATION_ERROR)

    def test_remaining_attachment_is_verification_error(self) -> None:
        with tempfile.TemporaryDirectory() as directory:
            temporary = Path(directory) / "temporary.eml"
            temporary.write_bytes(b"public")
            with (
                patch.object(
                    mime_serialization,
                    "_parse_message",
                    return_value=(attachment(), ()),
                ),
                patch.object(
                    mime_serialization,
                    "_leaf_fingerprints",
                    return_value=Counter(),
                ),
                patch.object(
                    mime_serialization,
                    "_collect_references",
                    return_value=empty_references(),
                ),
            ):
                with self.assertRaises(models.CliError) as raised:
                    mime_serialization._verify_serialized_message(
                        temporary,
                        Counter(),
                    )

        self.assertEqual(raised.exception.code, models.ExitCode.VERIFICATION_ERROR)
