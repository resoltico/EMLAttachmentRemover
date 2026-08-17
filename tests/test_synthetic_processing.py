"""Black-box synthetic contracts for the text-only EML transformation."""

from __future__ import annotations

import base64
import hashlib
import tempfile
import unittest
from email import policy
from email.message import EmailMessage
from pathlib import Path

import pytest

from eml_attachment_remover import process_file
from eml_attachment_remover.models import CliError, ExitCode
from tests.test_support import parse, run_cli, simple_message


def _plain_html_message() -> EmailMessage:
    """Return one conventional plain/HTML alternative message.

    Returns:
        The synthetic alternative body.

    """
    message = EmailMessage()
    message["Subject"] = "Synthetic text-only body"
    message.set_content("PUBLIC PLAIN BODY")
    message.add_alternative("<p>PUBLIC HTML BODY</p>", subtype="html")
    return message


class SyntheticTests(unittest.TestCase):
    """Exercise public processing and CLI boundaries with synthetic messages."""

    def test_html_resources_and_ordinary_attachments_are_separately_audited(
        self,
    ) -> None:
        message = simple_message()
        message.add_attachment(
            b"JPEG-PHOTO",
            maintype="image",
            subtype="jpeg",
            filename="damage photo.jpg",
        )
        with tempfile.TemporaryDirectory() as directory:
            source = Path(directory) / "source.eml"
            output = Path(directory) / "output.eml"
            source.write_bytes(message.as_bytes(policy=policy.SMTP))

            result = process_file(source, output, force=False, dry_run=False)
            parsed = parse(output)

        self.assertEqual(
            [part.filename for part in result.discarded_body_resources],
            ["signature.png"],
        )
        self.assertEqual(
            [part.filename for part in result.removed_attachments],
            ["invoice.pdf", "damage photo.jpg"],
        )
        leaves = [part for part in parsed.walk() if not part.is_multipart()]
        self.assertEqual([part.get_content_type() for part in leaves], ["text/plain"])

    def test_related_outer_shape_discards_content_location_resource(self) -> None:
        message = _plain_html_message()
        payload = message.get_payload()
        assert isinstance(payload, list)
        html = payload[1]
        assert isinstance(html, EmailMessage)
        html.set_content('<img src="logo.png">', subtype="html")
        related = EmailMessage()
        related.make_related()
        related.attach(message)
        logo = EmailMessage()
        logo["Content-Type"] = 'image/png; name="logo.png"'
        logo["Content-Location"] = "logo.png"
        logo["Content-Transfer-Encoding"] = "base64"
        logo.set_payload(base64.b64encode(b"PNG-LOGO").decode("ascii"))
        related.attach(logo)
        with tempfile.TemporaryDirectory() as directory:
            source = Path(directory) / "source.eml"
            output = Path(directory) / "output.eml"
            source.write_bytes(related.as_bytes(policy=policy.SMTP))

            result = process_file(source, output, force=False, dry_run=False)
            output_types = [part.get_content_type() for part in parse(output).walk()]

        self.assertEqual(result.discarded_body_resources[0].filename, "logo.png")
        self.assertEqual(result.discarded_body_resources[0].referenced_by, ((0, 1),))
        self.assertEqual(
            output_types,
            ["text/plain"],
        )

    def test_unicode_spaces_quotes_and_emoji_in_paths(self) -> None:
        with tempfile.TemporaryDirectory() as directory:
            base = Path(directory)
            source = base / "- žą  'truck' 🚚.eml"
            destination = base / "результат žą 🚚.eml"
            source.write_bytes(simple_message().as_bytes(policy=policy.SMTP))

            result = run_cli("-o", str(destination), "--", str(source))

            self.assertEqual(result.returncode, 0, result.stderr)
            self.assertEqual(
                [
                    part.get_content_type()
                    for part in parse(destination).walk()
                    if not part.is_multipart()
                ],
                ["text/plain"],
            )

    def test_named_plain_alternative_fails_closed_without_output(self) -> None:
        message = _plain_html_message()
        payload = message.get_payload()
        assert isinstance(payload, list)
        plain = payload[0]
        assert isinstance(plain, EmailMessage)
        plain.set_param("name", "body.txt", header="Content-Type")
        with tempfile.TemporaryDirectory() as directory:
            source = Path(directory) / "source.eml"
            output = Path(directory) / "output.eml"
            source.write_bytes(message.as_bytes(policy=policy.SMTP))

            result = run_cli("-o", str(output), str(source))

            self.assertEqual(result.returncode, ExitCode.TRANSFORMATION_UNAVAILABLE)
            self.assertFalse(output.exists())

    def test_plain_only_message_produces_a_byte_exact_copy(self) -> None:
        message = EmailMessage()
        message["Subject"] = "No transformation"
        message.set_content("PUBLIC BODY")
        raw = message.as_bytes(policy=policy.SMTP)
        with tempfile.TemporaryDirectory() as directory:
            source = Path(directory) / "source.eml"
            output = Path(directory) / "output.eml"
            source.write_bytes(raw)

            result = process_file(source, output, force=False, dry_run=False)

            self.assertEqual(output.read_bytes(), raw)
            self.assertFalse(result.discarded_body_representations)
            self.assertFalse(result.discarded_body_resources)
            self.assertFalse(result.removed_attachments)

    def test_protected_root_fails_closed_and_preserves_source(self) -> None:
        message = EmailMessage()
        message["Content-Type"] = "application/pkcs7-mime"
        message["Content-Transfer-Encoding"] = "base64"
        message.set_payload(base64.b64encode(b"opaque-ciphertext").decode("ascii"))
        raw = message.as_bytes(policy=policy.SMTP)
        with tempfile.TemporaryDirectory() as directory:
            source = Path(directory) / "source.eml"
            output = Path(directory) / "output.eml"
            source.write_bytes(raw)

            with pytest.raises(CliError) as raised:
                process_file(source, output, force=False, dry_run=False)

            self.assertEqual(raised.value.code, ExitCode.TRANSFORMATION_UNAVAILABLE)
            self.assertEqual(source.read_bytes(), raw)
            self.assertFalse(output.exists())

    def test_protected_attachment_sibling_is_removed_atomically(self) -> None:
        protected = EmailMessage()
        protected["Content-Type"] = "application/pkcs7-mime"
        protected["Content-Disposition"] = 'attachment; filename="opaque.p7m"'
        protected.set_payload("PUBLIC OPAQUE BYTES")
        message = EmailMessage()
        message.make_mixed()
        body = EmailMessage()
        body.set_content("PUBLIC BODY")
        message.attach(body)
        message.attach(protected)
        with tempfile.TemporaryDirectory() as directory:
            source = Path(directory) / "source.eml"
            output = Path(directory) / "output.eml"
            source.write_bytes(message.as_bytes(policy=policy.SMTP))

            result = process_file(source, output, force=False, dry_run=False)
            output_types = [part.get_content_type() for part in parse(output).walk()]

        self.assertEqual(
            [part.filename for part in result.removed_attachments],
            ["opaque.p7m"],
        )
        self.assertEqual(
            output_types,
            ["text/plain"],
        )

    def test_root_attachment_has_no_invented_body_and_fails_closed(self) -> None:
        message = EmailMessage()
        message["Content-Type"] = 'application/pdf; name="root.pdf"'
        message["Content-Disposition"] = 'attachment; filename="root.pdf"'
        message.set_payload(base64.b64encode(b"%PDF-root").decode("ascii"))
        with tempfile.TemporaryDirectory() as directory:
            source = Path(directory) / "source.eml"
            output = Path(directory) / "output.eml"
            source.write_bytes(message.as_bytes(policy=policy.SMTP))

            result = run_cli("-o", str(output), str(source))

        self.assertEqual(result.returncode, ExitCode.TRANSFORMATION_UNAVAILABLE)
        self.assertFalse(output.exists())

    def test_default_output_preserves_case_and_uses_text_only_suffix(self) -> None:
        with tempfile.TemporaryDirectory() as directory:
            source = Path(directory) / "MESSAGE.EML"
            source.write_bytes(_plain_html_message().as_bytes(policy=policy.SMTP))

            result = run_cli(str(source))

            self.assertEqual(result.returncode, 0, result.stderr)
            self.assertTrue((Path(directory) / "MESSAGE.text-only.EML").is_file())

    def test_original_hash_remains_stable_after_transformation(self) -> None:
        message = simple_message()
        raw = message.as_bytes(policy=policy.SMTP)
        expected = hashlib.sha256(raw).hexdigest()
        with tempfile.TemporaryDirectory() as directory:
            source = Path(directory) / "source.eml"
            output = Path(directory) / "output.eml"
            source.write_bytes(raw)

            process_file(source, output, force=False, dry_run=False)

            self.assertEqual(hashlib.sha256(source.read_bytes()).hexdigest(), expected)
