"""Fast semantic contracts for previously surviving MIME mutations."""

from __future__ import annotations

import hashlib
import tempfile
import unittest
from collections import Counter
from email.message import EmailMessage
from pathlib import Path
from unittest.mock import call, patch

from eml_attachment_remover import mime_locations, mime_references, mime_serialization
from eml_attachment_remover.models import CliError, ExitCode, ReferenceIndex


def _mixed(*children: EmailMessage) -> EmailMessage:
    """Return a multipart message containing the supplied children.

    Returns:
        A fresh multipart/mixed message.

    """
    message = EmailMessage()
    message.make_mixed()
    for child in children:
        message.attach(child)
    return message


class ReferenceMutationContracts(unittest.TestCase):
    """Distinguish normalization and body-content extraction mutations."""

    def test_content_id_header_preserves_literal_scheme_text(self) -> None:
        self.assertEqual(
            mime_references._normalize_content_id_header(
                "cid:public@example.test",
            ),
            "cid:public@example.test",
        )

    def test_normalizers_do_not_discard_unrelated_x_characters(self) -> None:
        self.assertEqual(
            mime_references._normalize_content_id_header("Xpublic-idX"),
            "Xpublic-idX",
        )
        self.assertEqual(
            mime_locations._normalize_content_location_header("Xpublic.pngX"),
            "Xpublic.pngX",
        )

    def test_plain_text_markup_does_not_create_html_references(self) -> None:
        plain = EmailMessage()
        plain.set_content('<img src="not-html.png">')

        self.assertEqual(
            mime_references._collect_references(plain),
            ReferenceIndex(frozenset(), frozenset()),
        )


class SerializationMutationContracts(unittest.TestCase):
    """Distinguish fingerprint, parse, traversal, and diagnostic mutations."""

    def test_fingerprint_preserves_metadata_and_normalizes_text_newlines(self) -> None:
        text = EmailMessage()
        text["Content-Type"] = "text/plain"
        text["Content-ID"] = "<public@example.test>"
        text["Content-Location"] = "public.txt"
        text.set_payload("one\r\ntwo\rthree\n")
        digest = hashlib.sha256(b"one\ntwo\nthree\n").hexdigest()

        self.assertEqual(
            mime_serialization._leaf_fingerprints(_mixed(text)),
            Counter({
                (
                    (0,),
                    "text/plain",
                    None,
                    "<public@example.test>",
                    "public.txt",
                    digest,
                ): 1,
            }),
        )

    def test_parse_forces_transfer_decoding_on_every_leaf(self) -> None:
        raw = (
            b"MIME-Version: 1.0\r\n"
            b"Content-Type: application/octet-stream\r\n"
            b"Content-Transfer-Encoding: base64\r\n\r\n"
            b"@@not-base64@@\r\n"
        )

        with self.assertRaises(CliError) as raised:
            mime_serialization._parse_message(raw, "public.eml")

        self.assertIs(raised.exception.code, ExitCode.PARSE_ERROR)

    def test_multipart_root_is_never_reported_as_a_root_attachment(self) -> None:
        body = EmailMessage()
        body.set_content("public body")

        self.assertIsNone(
            mime_serialization._remaining_removable_part(_mixed(body)),
        )

    def test_verification_passes_the_actual_temporary_path_to_parser(self) -> None:
        with tempfile.TemporaryDirectory() as directory:
            temporary = Path(directory) / "public-output.eml"
            temporary.write_bytes(b"public")
            with (
                patch.object(
                    mime_serialization,
                    "_parse_message",
                    return_value=(EmailMessage(), ()),
                ) as parse_message,
                patch.object(
                    mime_serialization,
                    "_leaf_fingerprints",
                    return_value=Counter(),
                ),
                patch.object(
                    mime_serialization,
                    "_remaining_removable_part",
                    return_value=None,
                ),
            ):
                mime_serialization._verify_serialized_message(
                    temporary,
                    Counter(),
                )

        self.assertEqual(
            parse_message.call_args_list,
            [call(b"public", str(temporary))],
        )
