# ruff: file-ignore[import-private-name, private-member-access]
"""Mutation-focused processing-result and source-version contracts."""

from __future__ import annotations

import os
import tempfile
import unittest
from email import policy
from email.message import EmailMessage
from pathlib import Path
from unittest.mock import patch

import eml_attachment_remover._version as version_module
from eml_attachment_remover import models, processing


def _root_attachment() -> EmailMessage:
    """Return a synthetic root attachment.

    Returns:
        A single removable public PDF entity.

    """
    message = EmailMessage()
    message.set_content("PUBLIC BODY")
    message.add_attachment(
        b"PUBLIC",
        maintype="application",
        subtype="pdf",
        filename="public.pdf",
    )
    return message


def _opaque_message() -> EmailMessage:
    """Return a synthetic opaque cryptographic MIME entity.

    Returns:
        A public stand-in for encrypted content.

    """
    message = EmailMessage()
    message["Content-Type"] = "application/pkcs7-mime; smime-type=enveloped-data"
    message["Content-Transfer-Encoding"] = "base64"
    message.set_payload("UFVCTElD")
    return message


class ProcessingDiagnosticContractTests(unittest.TestCase):
    """Require stable header cleanup and source-specific diagnostics."""

    def test_part_inventory_rejects_non_message_multipart_members(self) -> None:
        message = EmailMessage()
        message.set_payload(["PUBLIC INVALID MEMBER"])

        with self.assertRaises(TypeError):
            processing._located_parts(message)

    def test_every_stale_header_is_removed_including_duplicates(self) -> None:
        message = EmailMessage()
        for header in processing.STALE_ROOT_HEADERS:
            message[header] = "1"
            message[header] = "2"
        processing._remove_stale_root_headers(message)
        for header in processing.STALE_ROOT_HEADERS:
            with self.subTest(header=header):
                self.assertIsNone(message.get_all(header))

    def test_transport_signature_warning_is_exact_and_ordered(self) -> None:
        unsigned = EmailMessage()
        self.assertIsNone(processing._transport_signature_warning(unsigned))

        signed = EmailMessage()
        for header in reversed(processing.TRANSPORT_SIGNATURE_HEADERS):
            signed[header] = "PUBLIC"
        self.assertEqual(
            processing._transport_signature_warning(signed),
            "rewriting the message invalidates existing transport signatures "
            "(ARC-Message-Signature, ARC-Seal, DKIM-Signature, "
            "DomainKey-Signature); the original EML remains unchanged",
        )

    def test_read_error_names_source_and_cause(self) -> None:
        source = Path("synthetic-source.eml")
        with patch.object(os, "open", side_effect=OSError("read blocked")):
            with self.assertRaises(models.CliError) as raised:
                processing._read_source(source)
        self.assertEqual(raised.exception.code, models.ExitCode.INPUT_ERROR)
        self.assertEqual(
            raised.exception.message,
            f"could not read input file {source}: read blocked",
        )

    def test_parse_failure_identifies_the_validated_source_path(self) -> None:
        with tempfile.TemporaryDirectory() as directory:
            source = Path(directory) / "synthetic-source.eml"
            source.write_bytes(b"PUBLIC")
            with patch(
                "eml_attachment_remover.mime_serialization.BytesParser.parsebytes",
                side_effect=ValueError("synthetic parse failure"),
            ):
                with self.assertRaises(models.CliError) as raised:
                    processing.process_file(
                        source,
                        None,
                        force=False,
                        dry_run=True,
                    )
        self.assertEqual(raised.exception.code, models.ExitCode.PARSE_ERROR)
        self.assertEqual(
            raised.exception.message,
            f"could not parse {source.absolute()}: synthetic parse failure",
        )

    def test_opaque_content_fails_as_transformation_unavailable(self) -> None:
        with tempfile.TemporaryDirectory() as directory:
            source = Path(directory) / "opaque.eml"
            raw = _opaque_message().as_bytes(policy=policy.SMTP)
            source.write_bytes(raw)
            with self.assertRaises(models.CliError) as raised:
                processing.process_file(
                    source,
                    None,
                    force=False,
                    dry_run=True,
                )
        self.assertEqual(
            raised.exception.code,
            models.ExitCode.TRANSFORMATION_UNAVAILABLE,
        )


class ProcessResultContractTests(unittest.TestCase):
    """Require every dry-run and written-result field to be meaningful."""

    def test_dry_run_reports_exact_source_and_removal_metadata(self) -> None:
        with tempfile.TemporaryDirectory() as directory:
            source = Path(directory) / "root-attachment.eml"
            raw = _root_attachment().as_bytes(policy=policy.SMTP)
            source.write_bytes(raw)
            result = processing.process_file(
                source,
                None,
                force=False,
                dry_run=True,
            )
        self.assertEqual(result.source, source.absolute())
        self.assertIsNone(result.destination)
        self.assertEqual(result.source_size, len(raw))
        self.assertIsNone(result.output_size)
        self.assertTrue(result.dry_run)
        self.assertEqual(
            result.removed_attachments,
            (models.RemovedPart((1,), "application/pdf", "public.pdf", "attachment"),),
        )
        self.assertEqual(
            result.selected_plain_text_bodies,
            (models.SelectedPlainTextBody((0,), "text/plain"),),
        )
        self.assertEqual(result.discarded_body_representations, ())
        self.assertEqual(result.discarded_body_resources, ())
        self.assertEqual(result.warnings, ())

    def test_written_result_reports_exact_paths_sizes_and_mode(self) -> None:
        message = EmailMessage()
        message["Subject"] = "Public byte-exact copy"
        message.set_content("Public body")
        raw = message.as_bytes(policy=policy.SMTP)
        with tempfile.TemporaryDirectory() as directory:
            source = Path(directory) / "source.eml"
            destination = Path(directory) / "destination.eml"
            source.write_bytes(raw)
            result = processing.process_file(
                source,
                destination,
                force=False,
                dry_run=False,
            )
            self.assertEqual(destination.read_bytes(), raw)
        self.assertEqual(result.source, source.absolute())
        self.assertEqual(
            result.destination,
            destination.parent.resolve() / destination.name,
        )
        self.assertEqual(result.source_size, len(raw))
        self.assertEqual(result.output_size, len(raw))
        self.assertIsInstance(result.dry_run, bool)
        self.assertFalse(result.dry_run)
        self.assertEqual(result.removed_attachments, ())
        self.assertEqual(
            result.selected_plain_text_bodies,
            (models.SelectedPlainTextBody((), "text/plain"),),
        )
        self.assertEqual(result.discarded_body_representations, ())
        self.assertEqual(result.discarded_body_resources, ())
        self.assertEqual(result.warnings, ())


class SourceVersionContractTests(unittest.TestCase):
    """Require source-tree version lookup to read project metadata independently."""

    def test_project_version_reads_the_declared_value(self) -> None:
        with tempfile.TemporaryDirectory() as directory:
            project = Path(directory) / "pyproject.toml"
            project.write_text(
                '[project]\nname = "public-project"\nversion = "9.8.7"\n',
                encoding="utf-8",
            )
            with patch.object(version_module, "PROJECT_CONFIG", project):
                self.assertEqual(version_module._project_version(), "9.8.7")


if __name__ == "__main__":
    unittest.main(verbosity=2)
