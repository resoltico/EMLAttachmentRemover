"""Black-box regressions for MIME body-reference safety boundaries."""

from __future__ import annotations

import tempfile
import unittest
from email import policy
from email.message import EmailMessage
from pathlib import Path

from eml_attachment_remover import process_file
from tests.test_support import parse, run_cli


class ReferenceSafetyTests(unittest.TestCase):
    """Verify reference retention only protects genuine outer-message resources."""

    def test_iso_8859_1_body_location_reference_is_audited_and_discarded(self) -> None:
        message = EmailMessage()
        message["Subject"] = "Non-UTF-8 Content-Location resource"
        message.set_content("PUBLIC PLAIN BODY")
        message.add_alternative(
            '<html><body><img src="logo-café.png"></body></html>',
            subtype="html",
            charset="iso-8859-1",
        )
        related = EmailMessage()
        related.make_related()
        related.attach(message)
        logo = EmailMessage()
        logo.set_content(b"PNG-LOGO", maintype="image", subtype="png", cte="base64")
        logo.add_header("Content-Disposition", "attachment", filename="logo-cafe.png")
        logo["Content-Location"] = "logo-café.png"
        related.attach(logo)
        with tempfile.TemporaryDirectory() as directory:
            source = Path(directory) / "source.eml"
            output = Path(directory) / "output.eml"
            source.write_bytes(related.as_bytes(policy=policy.SMTP))
            result = process_file(source, output, force=False, dry_run=False)
            self.assertEqual(
                result.discarded_body_resources[0].referenced_by, ((0, 1),)
            )
            self.assertFalse(
                any(part.get("Content-Location") for part in parse(output).walk())
            )

    def test_related_container_removes_a_genuine_attachment(self) -> None:
        message = EmailMessage()
        message["Subject"] = "Attachment under related"
        message.set_content("Outer body")
        message.add_alternative("<p>Related body</p>", subtype="html")
        related = EmailMessage()
        related.make_related()
        related.attach(message)
        related_attachment = EmailMessage()
        related_attachment.set_content(
            b"RELATED-ATTACHMENT",
            maintype="application",
            subtype="octet-stream",
            cte="base64",
        )
        related_attachment.add_header(
            "Content-Disposition",
            "attachment",
            filename="related-file.bin",
        )
        related.attach(related_attachment)
        with tempfile.TemporaryDirectory() as directory:
            source = Path(directory) / "source.eml"
            output = Path(directory) / "output.eml"
            source.write_bytes(related.as_bytes(policy=policy.SMTP))
            result = run_cli("-o", str(output), str(source))
            self.assertEqual(result.returncode, 0, result.stderr)
            self.assertFalse(
                any(
                    part.get_filename() == "related-file.bin"
                    for part in parse(output).walk()
                )
            )

    def test_attached_message_reference_cannot_retain_an_outer_attachment(self) -> None:
        inner = EmailMessage()
        inner.set_content(
            '<html><body><img src="cid:outer-resource@example.test"></body></html>',
            subtype="html",
        )
        message = EmailMessage()
        message["Subject"] = "Attached-message reference isolation"
        message.set_content("Outer body")
        message.make_mixed()
        outer_resource = EmailMessage()
        outer_resource.set_content(
            b"OUTER", maintype="image", subtype="png", cte="base64"
        )
        outer_resource.add_header(
            "Content-Disposition",
            "attachment",
            filename="outer-resource.png",
        )
        outer_resource["Content-ID"] = "<outer-resource@example.test>"
        message.attach(outer_resource)
        message.add_attachment(inner, filename="forwarded.eml")
        with tempfile.TemporaryDirectory() as directory:
            source = Path(directory) / "source.eml"
            output = Path(directory) / "output.eml"
            source.write_bytes(message.as_bytes(policy=policy.SMTP))
            result = run_cli("-o", str(output), str(source))
            self.assertEqual(result.returncode, 0, result.stderr)
            output_names = [part.get_filename() for part in parse(output).walk()]
            self.assertNotIn("outer-resource.png", output_names)
            self.assertNotIn("forwarded.eml", output_names)
