"""Black-box MIME policy regression tests using public synthetic messages."""

from __future__ import annotations

import base64
import hashlib
import os
import tempfile
import unittest
from email import policy
from email.message import EmailMessage
from pathlib import Path

from tests.test_support import decoded_hash, parse, run_cli, simple_message


class SyntheticTests(unittest.TestCase):
    def test_image_attachment_is_removed_but_inline_signature_is_preserved(
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
            result = run_cli("-o", str(output), str(source))
            self.assertEqual(result.returncode, 0, result.stderr)
            parsed = parse(output)
            self.assertFalse(
                any(part.get_filename() == "damage photo.jpg" for part in parsed.walk())
            )
            signature = next(part for part in parsed.walk() if part.get("Content-ID"))
            self.assertEqual(signature.get_filename(), "signature.png")
            self.assertEqual(
                decoded_hash(signature),
                hashlib.sha256(b"PNG-SIGNATURE").hexdigest(),
            )

    def test_content_location_body_resource_is_preserved(self) -> None:
        message = EmailMessage()
        message["Subject"] = "Content-Location resource"
        message.set_content(
            '<html><body><img src="logo.png"></body></html>',
            subtype="html",
        )
        message.make_related()
        logo = EmailMessage()
        logo["Content-Type"] = 'image/png; name="logo.png"'
        logo["Content-Location"] = "logo.png"
        logo["Content-Transfer-Encoding"] = "base64"
        logo.set_payload(base64.b64encode(b"PNG-LOGO").decode("ascii"))
        message.attach(logo)
        message.make_mixed()
        message.add_attachment(
            b"FILE",
            maintype="application",
            subtype="octet-stream",
            filename="file.bin",
        )
        with tempfile.TemporaryDirectory() as directory:
            source = Path(directory) / "source.eml"
            output = Path(directory) / "output.eml"
            source.write_bytes(message.as_bytes(policy=policy.SMTP))
            result = run_cli("-o", str(output), str(source))
            self.assertEqual(result.returncode, 0, result.stderr)
            parsed = parse(output)
            self.assertTrue(
                any(
                    part.get("Content-Location") == "logo.png" for part in parsed.walk()
                )
            )
            self.assertFalse(
                any(part.get_filename() == "file.bin" for part in parsed.walk())
            )

    def test_unicode_spaces_quotes_and_emoji_in_paths(self) -> None:
        with tempfile.TemporaryDirectory() as directory:
            base = Path(directory)
            source = base / "- žą  'truck' 🚚.eml"
            destination = base / "результат žą 🚚.eml"
            source.write_bytes(simple_message().as_bytes(policy=policy.SMTP))
            result = run_cli("-o", str(destination), "--", str(source))
            self.assertEqual(result.returncode, 0, result.stderr)
            self.assertTrue(destination.is_file())
            output = parse(destination)
            self.assertEqual(
                [
                    part.get_filename()
                    for part in output.walk()
                    if part.get_content_disposition() == "attachment"
                ],
                [],
            )
            self.assertEqual(
                [
                    part.get_filename()
                    for part in output.walk()
                    if part.get("Content-ID")
                ],
                ["signature.png"],
            )

    def test_cid_image_is_preserved_even_if_disposition_says_attachment(self) -> None:
        message = simple_message()
        inline = next(part for part in message.walk() if part.get("Content-ID"))
        inline.replace_header(
            "Content-Disposition",
            'attachment; filename="signature.png"',
        )
        with tempfile.TemporaryDirectory() as directory:
            source = Path(directory) / "source.eml"
            output = Path(directory) / "output.eml"
            source.write_bytes(message.as_bytes(policy=policy.SMTP))
            result = run_cli("-o", str(output), str(source))
            self.assertEqual(result.returncode, 0, result.stderr)
            parsed = parse(output)
            kept = [part for part in parsed.walk() if part.get("Content-ID")]
            self.assertEqual(len(kept), 1)
            self.assertEqual(
                decoded_hash(kept[0]),
                hashlib.sha256(b"PNG-SIGNATURE").hexdigest(),
            )

    def test_unreferenced_cid_image_marked_attachment_is_removed(self) -> None:
        message = EmailMessage()
        message["Subject"] = "Unreferenced CID attachment"
        message.set_content("Body without an image reference")
        message.add_attachment(
            b"PNG-ATTACHMENT",
            maintype="image",
            subtype="png",
            filename="photo.png",
        )
        attachment = next(
            part
            for part in message.walk()
            if part.get_content_disposition() == "attachment"
        )
        attachment["Content-ID"] = "<unreferenced@example.test>"
        with tempfile.TemporaryDirectory() as directory:
            source = Path(directory) / "source.eml"
            output = Path(directory) / "output.eml"
            source.write_bytes(message.as_bytes(policy=policy.SMTP))
            result = run_cli("-o", str(output), str(source))
            self.assertEqual(result.returncode, 0, result.stderr)
            self.assertFalse(
                any(
                    part.get_filename() == "photo.png" for part in parse(output).walk()
                ),
            )

    def test_legacy_named_part_without_disposition_is_removed(self) -> None:
        message = EmailMessage()
        message["Subject"] = "Legacy attachment"
        message.set_content("Body")
        message.make_mixed()
        legacy = EmailMessage()
        legacy["Content-Type"] = 'application/octet-stream; name="legacy.bin"'
        legacy["Content-Transfer-Encoding"] = "base64"
        legacy.set_payload(base64.b64encode(b"legacy-binary").decode("ascii"))
        message.attach(legacy)
        with tempfile.TemporaryDirectory() as directory:
            source = Path(directory) / "source.eml"
            output = Path(directory) / "output.eml"
            source.write_bytes(message.as_bytes(policy=policy.SMTP))
            result = run_cli("-o", str(output), str(source))
            self.assertEqual(result.returncode, 0, result.stderr)
            self.assertNotIn(
                "legacy.bin",
                [part.get_filename() for part in parse(output).walk()],
            )

    def test_named_alternative_body_is_preserved(self) -> None:
        message = EmailMessage()
        message["Subject"] = "Named body"
        message.set_content("Body text")
        message.add_alternative("<p>Body HTML</p>", subtype="html")
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
            self.assertEqual(result.returncode, 0, result.stderr)
            parsed = parse(output)
            self.assertEqual(
                len([
                    part
                    for part in parsed.walk()
                    if part.get_content_type() == "text/plain"
                ]),
                1,
            )

    def test_no_attachment_produces_byte_exact_copy(self) -> None:
        message = EmailMessage()
        message["Subject"] = "No attachment"
        message.set_content("Body")
        raw = message.as_bytes(policy=policy.SMTP)
        with tempfile.TemporaryDirectory() as directory:
            source = Path(directory) / "source.eml"
            output = Path(directory) / "output.eml"
            source.write_bytes(raw)
            result = run_cli("-o", str(output), str(source))
            self.assertEqual(result.returncode, 0, result.stderr)
            self.assertEqual(output.read_bytes(), raw)

    def test_opaque_encrypted_root_produces_byte_exact_copy(self) -> None:
        message = EmailMessage()
        message["Subject"] = "Opaque S/MIME"
        message["Content-Type"] = "application/pkcs7-mime; smime-type=enveloped-data"
        message["Content-Transfer-Encoding"] = "base64"
        message.set_payload(base64.b64encode(b"opaque-ciphertext").decode("ascii"))
        raw = message.as_bytes(policy=policy.SMTP)
        with tempfile.TemporaryDirectory() as directory:
            source = Path(directory) / "source.eml"
            output = Path(directory) / "output.eml"
            source.write_bytes(raw)
            result = run_cli("-o", str(output), str(source))
            self.assertEqual(result.returncode, 0, result.stderr)
            self.assertEqual(output.read_bytes(), raw)
            self.assertIn("protected MIME entity left intact", result.stdout)

    def test_message_rfc822_attachment_is_removed_as_one_entity(self) -> None:
        inner = EmailMessage()
        inner["From"] = "inner@example.test"
        inner["To"] = "recipient@example.test"
        inner["Subject"] = "Forwarded"
        inner.set_content("Forwarded body")
        message = EmailMessage()
        message["Subject"] = "Outer"
        message.set_content("Outer body")
        message.add_attachment(inner, filename="forwarded.eml")
        with tempfile.TemporaryDirectory() as directory:
            source = Path(directory) / "source.eml"
            output = Path(directory) / "output.eml"
            source.write_bytes(message.as_bytes(policy=policy.SMTP))
            result = run_cli("-o", str(output), str(source))
            self.assertEqual(result.returncode, 0, result.stderr)
            self.assertFalse(
                any(
                    part.get_filename() == "forwarded.eml"
                    for part in parse(output).walk()
                )
            )

    def test_root_attachment_is_replaced_with_notice(self) -> None:
        message = EmailMessage()
        message["Subject"] = "Root attachment"
        message["Content-Type"] = 'application/pdf; name="root.pdf"'
        message["Content-Disposition"] = 'attachment; filename="root.pdf"'
        message["Content-Transfer-Encoding"] = "base64"
        message.set_payload(base64.b64encode(b"%PDF-root").decode("ascii"))
        with tempfile.TemporaryDirectory() as directory:
            source = Path(directory) / "source.eml"
            output = Path(directory) / "output.eml"
            source.write_bytes(message.as_bytes(policy=policy.SMTP))
            result = run_cli("-o", str(output), str(source))
            self.assertEqual(result.returncode, 0, result.stderr)
            parsed = parse(output)
            self.assertEqual(parsed.get_content_type(), "text/plain")
            self.assertIn("root attachment was removed", parsed.get_content())

    def test_attachment_inside_multipart_signed_returns_code_6(self) -> None:
        signed_content = EmailMessage()
        signed_content.set_content("Signed body")
        signed_content.add_attachment(
            b"protected",
            maintype="application",
            subtype="octet-stream",
            filename="protected.bin",
        )
        signature = EmailMessage()
        signature["Content-Type"] = 'application/pgp-signature; name="signature.asc"'
        signature["Content-Disposition"] = 'attachment; filename="signature.asc"'
        signature.set_payload("fake-signature")
        message = EmailMessage()
        message["Subject"] = "Signed"
        message["Content-Type"] = (
            'multipart/signed; protocol="application/pgp-signature"; '
            'boundary="signed-boundary"'
        )
        message.set_payload([signed_content, signature])
        with tempfile.TemporaryDirectory() as directory:
            source = Path(directory) / "signed.eml"
            output = Path(directory) / "output.eml"
            source.write_bytes(message.as_bytes(policy=policy.SMTP))
            result = run_cli("-o", str(output), str(source))
            self.assertEqual(result.returncode, 6)
            self.assertIn("multipart/signed", result.stderr)
            self.assertFalse(output.exists())

    def test_outer_attachment_is_removed_while_encrypted_body_is_preserved(
        self,
    ) -> None:
        encrypted = EmailMessage()
        encrypted["Content-Type"] = (
            'multipart/encrypted; protocol="application/pgp-encrypted"'
        )
        control = EmailMessage()
        control.set_type("application/pgp-encrypted")
        control.set_payload("Version: 1")
        payload = EmailMessage()
        payload.set_type("application/octet-stream")
        payload.set_payload("opaque-ciphertext")
        encrypted.set_payload([control, payload])
        message = EmailMessage()
        message["Subject"] = "Encrypted plus attachment"
        message.make_mixed()
        message.attach(encrypted)
        message.add_attachment(
            b"%PDF",
            maintype="application",
            subtype="pdf",
            filename="outer.pdf",
        )
        with tempfile.TemporaryDirectory() as directory:
            source = Path(directory) / "source.eml"
            output = Path(directory) / "output.eml"
            source.write_bytes(message.as_bytes(policy=policy.SMTP))
            result = run_cli("-o", str(output), str(source))
            self.assertEqual(result.returncode, 0, result.stderr)
            parsed = parse(output)
            self.assertTrue(
                any(
                    part.get_content_type() == "multipart/encrypted"
                    for part in parsed.walk()
                )
            )
            self.assertFalse(
                any(part.get_filename() == "outer.pdf" for part in parsed.walk())
            )

    def test_outer_attachment_with_signed_body_returns_code_6(self) -> None:
        signed_body = EmailMessage()
        signed_body.set_content("Signed body only")
        signature = EmailMessage()
        signature["Content-Type"] = "application/pgp-signature"
        signature.set_payload("fake-signature")
        signed = EmailMessage()
        signed["Content-Type"] = (
            'multipart/signed; protocol="application/pgp-signature"; '
            'boundary="signed-boundary"'
        )
        signed.set_payload([signed_body, signature])
        message = EmailMessage()
        message["Subject"] = "Signed body plus outer attachment"
        message.make_mixed()
        message.attach(signed)
        message.add_attachment(
            b"OUTER",
            maintype="application",
            subtype="pdf",
            filename="outer.pdf",
        )
        with tempfile.TemporaryDirectory() as directory:
            source = Path(directory) / "source.eml"
            output = Path(directory) / "output.eml"
            source.write_bytes(message.as_bytes(policy=policy.SMTP))
            result = run_cli("-o", str(output), str(source))
            self.assertEqual(result.returncode, 6)
            self.assertFalse(output.exists())
            self.assertIn("multipart/signed", result.stderr)

    @unittest.skipIf(
        os.name == "nt",
        "control characters are not portable Windows filenames",
    )
    def test_console_escapes_control_characters_in_paths(self) -> None:
        with tempfile.TemporaryDirectory() as directory:
            base = Path(directory)
            source = base / "source.eml"
            destination = base / "output\n\x1b[31m.eml"
            source.write_bytes(simple_message().as_bytes(policy=policy.SMTP))
            result = run_cli("-o", str(destination), str(source))
            self.assertEqual(result.returncode, 0, result.stderr)
            self.assertNotIn("\x1b", result.stdout)
            self.assertIn("\\x0a", result.stdout)
            self.assertIn("\\x1b", result.stdout)

    @unittest.skipIf(os.name == "nt", "POSIX mode bits differ on Windows")
    def test_source_permission_bits_are_copied(self) -> None:
        with tempfile.TemporaryDirectory() as directory:
            source = Path(directory) / "source.eml"
            output = Path(directory) / "output.eml"
            source.write_bytes(simple_message().as_bytes(policy=policy.SMTP))
            source.chmod(0o640)
            result = run_cli("-o", str(output), str(source))
            self.assertEqual(result.returncode, 0, result.stderr)
            self.assertEqual(output.stat().st_mode & 0o777, 0o640)

    def test_uppercase_eml_uses_non_destructive_default_name(self) -> None:
        with tempfile.TemporaryDirectory() as directory:
            source = Path(directory) / "MESSAGE.EML"
            source.write_bytes(simple_message().as_bytes(policy=policy.SMTP))
            result = run_cli(str(source))
            self.assertEqual(result.returncode, 0, result.stderr)
            self.assertTrue(
                (Path(directory) / "MESSAGE.attachments-removed.EML").is_file()
            )
