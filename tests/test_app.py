"""Black-box regression tests for a public Outlook-style EML fixture."""

from __future__ import annotations

import hashlib
import tempfile
import unittest
from email import policy
from email.message import EmailMessage
from pathlib import Path

from tests.test_support import leaf_rows, parse, run_cli


def public_outlook_style_message() -> EmailMessage:
    """Create a synthetic field-shape fixture with resources and an attachment.

    Returns:
        A message with a removable PDF and eight HTML-dependent image resources.

    """
    message = EmailMessage()
    message["From"] = "public.sender@example.test"
    message["To"] = "public.recipient@example.test"
    message["Subject"] = "Public Outlook-style fixture"
    message["X-MS-Has-Attach"] = "yes"
    message.set_content("Public plain-text body")
    references = "".join(
        f'<img src="cid:public-evidence-{index}@example.test">' for index in range(1, 9)
    )
    message.add_alternative(
        f"<html><body><p>Public HTML body</p>{references}</body></html>",
        subtype="html",
    )
    payload = message.get_payload()
    assert isinstance(payload, list)
    html = payload[1]
    assert isinstance(html, EmailMessage)
    for index in range(1, 9):
        html.add_related(
            b"\xff\xd8\xff\xe0" + f"PUBLIC-JPEG-{index}".encode() + b"\xff\xd9",
            maintype="application",
            subtype="octet-stream",
            cid=f"<public-evidence-{index}@example.test>",
            filename=f"public-evidence-{index}.jpg",
            disposition="inline",
        )
    message.add_attachment(
        b"%PDF-1.7\n" + b"PUBLIC-FIXTURE-PDF\n" * 4096,
        maintype="application",
        subtype="pdf",
        filename="public-attachment.pdf",
    )
    return message


class PublicEmlFixtureTests(unittest.TestCase):
    """Exercise a public fixture that resembles an Outlook MIME structure."""

    @staticmethod
    def _write_fixture(directory: str) -> Path:
        """Write and return a public fixture EML in the supplied directory.

        Returns:
            The generated public fixture path.

        """
        source = Path(directory) / "public-outlook-style.eml"
        source.write_bytes(public_outlook_style_message().as_bytes(policy=policy.SMTP))
        return source

    def test_public_fixture_creates_audited_text_only_copy(self) -> None:
        with tempfile.TemporaryDirectory() as directory:
            source_path = self._write_fixture(directory)
            destination = Path(directory) / "result.eml"
            source_hash = hashlib.sha256(source_path.read_bytes()).hexdigest()
            result = run_cli("-o", str(destination), "--", str(source_path))
            self.assertEqual(result.returncode, 0, result.stderr)
            self.assertTrue(destination.is_file())
            self.assertEqual(
                hashlib.sha256(source_path.read_bytes()).hexdigest(),
                source_hash,
            )
            source_rows = leaf_rows(parse(source_path))
            output = parse(destination)
            output_rows = leaf_rows(output)
            source_attachments = [
                (row[2], row[0]) for row in source_rows if row[1] == "attachment"
            ]
            self.assertEqual(len(source_attachments), 1)
            self.assertEqual(source_attachments[0][1], "application/pdf")
            self.assertEqual(
                [(row[2], row[0]) for row in output_rows if row[1] == "attachment"],
                [],
            )
            source_inline = [row for row in source_rows if row[1] == "inline"]
            self.assertEqual(len(source_inline), 8)
            self.assertEqual(len(output_rows), 1)
            self.assertEqual(output_rows[0][0], "text/plain")
            self.assertIsNone(output_rows[0][1])
            self.assertIsNone(output_rows[0][2])
            self.assertIsNone(output_rows[0][3])
            self.assertEqual(
                output_rows[0][4],
                next(row[4] for row in source_rows if row[0] == "text/plain"),
            )
            discarded_hashes = {row[4] for row in source_rows if row[0] != "text/plain"}
            self.assertTrue(
                discarded_hashes.isdisjoint({row[4] for row in output_rows})
            )
            self.assertEqual(parse(source_path)["Subject"], output["Subject"])
            self.assertEqual(parse(source_path)["From"], output["From"])
            self.assertEqual(parse(source_path)["To"], output["To"])
            self.assertIsNone(output["X-MS-Has-Attach"])
            self.assertIn("Discarded body representations: 1", result.stdout)
            self.assertIn("Discarded body resources: 8", result.stdout)
            self.assertIn("Removed ordinary attachments: 1", result.stdout)
            self.assertEqual(
                [defect for part in output.walk() for defect in part.defects],
                [],
            )
            self.assertLess(
                destination.stat().st_size, source_path.stat().st_size // 20
            )

    def test_public_fixture_dry_run_ignores_existing_default_output(self) -> None:
        with tempfile.TemporaryDirectory() as directory:
            source = self._write_fixture(directory)
            default_output = source.with_name(
                f"{source.stem}.text-only{source.suffix}",
            )
            default_output.write_bytes(b"existing")
            result = run_cli("--dry-run", "--", str(source))
            self.assertEqual(result.returncode, 0, result.stderr)
            self.assertEqual(default_output.read_bytes(), b"existing")
            self.assertIn("Text-only transformation plan.", result.stdout)
            self.assertIn("Discarded body resources: 8", result.stdout)
            self.assertIn("Removed ordinary attachments: 1", result.stdout)
