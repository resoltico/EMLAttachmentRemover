"""Direct tests for safe filesystem and output error paths."""

from __future__ import annotations

import os
import tempfile
import unittest
from collections import Counter
from email.message import EmailMessage
from pathlib import Path
from unittest.mock import patch

from eml_attachment_remover import (
    atomic_publish,
    mime_serialization,
    models,
    output_commit,
    paths,
    processing,
    storage,
)


class PathValidationEdgeCaseTests(unittest.TestCase):
    """Exercise one path-validation contract per test."""

    def test_default_destination_marks_derived_eml(self) -> None:
        self.assertEqual(
            paths._default_destination(Path("public")),
            Path("public.attachments-removed.eml"),
        )

    def test_destination_inspection_error_is_write_error(self) -> None:
        with patch.object(Path, "lstat", side_effect=OSError("blocked")):
            with self.assertRaises(models.CliError) as raised:
                paths._destination_exists(Path("destination.eml"))

        self.assertEqual(raised.exception.code, models.ExitCode.WRITE_ERROR)

    def test_destination_resolution_error_is_write_error(self) -> None:
        with tempfile.TemporaryDirectory() as directory:
            source = Path(directory) / "source.eml"
            source.write_bytes(b"public")
            destination = Path(directory) / "destination.eml"
            with patch.object(
                paths,
                "_absolute_path",
                side_effect=[source.absolute(), OSError("blocked")],
            ):
                with self.assertRaises(models.CliError) as raised:
                    paths._validate_paths(source, destination, force=False)

        self.assertEqual(raised.exception.code, models.ExitCode.WRITE_ERROR)

    def test_source_inspection_error_is_input_error(self) -> None:
        with patch.object(Path, "stat", side_effect=OSError("blocked")):
            with self.assertRaises(models.CliError) as raised:
                paths._validate_source(Path("source.eml"))

        self.assertEqual(raised.exception.code, models.ExitCode.INPUT_ERROR)

    def test_source_mode_copy_error_returns_warning(self) -> None:
        with tempfile.TemporaryDirectory() as directory:
            source = Path(directory) / "source.eml"
            source.write_bytes(b"public")
            temporary = Path(directory) / "temporary.eml"
            with patch.object(Path, "chmod", side_effect=OSError("blocked")):
                warning = output_commit._apply_source_mode(source, temporary)

        self.assertIsNotNone(warning)

    def test_source_directory_is_rejected(self) -> None:
        with tempfile.TemporaryDirectory() as directory:
            with self.assertRaises(models.CliError) as raised:
                paths._validate_source(Path(directory))

        self.assertEqual(raised.exception.code, models.ExitCode.INPUT_ERROR)

    def test_source_cannot_be_its_own_destination(self) -> None:
        with tempfile.TemporaryDirectory() as directory:
            source = Path(directory) / "source.eml"
            source.write_bytes(b"public")
            with self.assertRaises(models.CliError) as raised:
                paths._validate_paths(source, source, force=False)

        self.assertEqual(raised.exception.code, models.ExitCode.OUTPUT_CONFLICT)

    def test_destination_directory_is_rejected_even_with_force(self) -> None:
        with tempfile.TemporaryDirectory() as directory:
            base = Path(directory)
            source = base / "source.eml"
            source.write_bytes(b"public")
            with self.assertRaises(models.CliError) as raised:
                paths._validate_paths(source, base, force=True)

        self.assertEqual(raised.exception.code, models.ExitCode.OUTPUT_CONFLICT)

    def test_nondirectory_destination_parent_is_write_error(self) -> None:
        with tempfile.TemporaryDirectory() as directory:
            base = Path(directory)
            source = base / "source.eml"
            source.write_bytes(b"public")
            parent = base / "not-a-directory"
            parent.write_bytes(b"public")
            with patch.object(paths, "_destination_exists", return_value=False):
                with self.assertRaises(models.CliError) as raised:
                    paths._validate_paths(source, parent / "output.eml", force=False)

        self.assertEqual(raised.exception.code, models.ExitCode.WRITE_ERROR)

    def test_destination_parent_inspection_error_is_write_error(self) -> None:
        with tempfile.TemporaryDirectory() as directory:
            base = Path(directory)
            source = base / "source.eml"
            source.write_bytes(b"public")
            with (
                patch.object(paths, "_validate_source", return_value=source),
                patch.object(Path, "stat", side_effect=OSError("blocked")),
            ):
                with self.assertRaises(models.CliError) as raised:
                    paths._validate_paths(
                        source,
                        base / "destination.eml",
                        force=False,
                    )

        self.assertEqual(raised.exception.code, models.ExitCode.WRITE_ERROR)

    def test_missing_output_directory_is_write_error(self) -> None:
        with tempfile.TemporaryDirectory() as directory:
            missing = Path(directory) / "missing"
            with self.assertRaises(models.CliError) as raised:
                paths._validate_output_directory(missing)

        self.assertEqual(raised.exception.code, models.ExitCode.WRITE_ERROR)

    def test_file_cannot_be_output_directory(self) -> None:
        with tempfile.TemporaryDirectory() as directory:
            candidate = Path(directory) / "not-a-directory"
            candidate.write_bytes(b"public")
            with self.assertRaises(models.CliError) as raised:
                paths._validate_output_directory(candidate)

        self.assertEqual(raised.exception.code, models.ExitCode.WRITE_ERROR)

    def test_output_directory_resolution_error_is_write_error(self) -> None:
        with patch.object(paths, "_absolute_path", side_effect=OSError("blocked")):
            with self.assertRaises(models.CliError) as raised:
                paths._validate_output_directory(Path("public"))

        self.assertEqual(raised.exception.code, models.ExitCode.WRITE_ERROR)


class FilesystemWriteEdgeCaseTests(unittest.TestCase):
    """Exercise one atomic publication, commit, or processing contract per test."""

    def test_atomic_publication_rejects_existing_destination(self) -> None:
        with tempfile.TemporaryDirectory() as directory:
            temporary = Path(directory) / "temporary.eml"
            destination = Path(directory) / "destination.eml"
            temporary.write_bytes(b"derived")
            destination.write_bytes(b"existing")
            with self.assertRaises(models.CliError) as raised:
                output_commit._publish_without_clobber(temporary, destination)

        self.assertEqual(raised.exception.code, models.ExitCode.OUTPUT_CONFLICT)

    def test_atomic_publication_error_is_write_error(self) -> None:
        with patch.object(Path, "hardlink_to", side_effect=OSError("blocked")):
            with self.assertRaises(models.CliError) as raised:
                output_commit._publish_without_clobber(
                    Path("temporary.eml"),
                    Path("destination.eml"),
                )

        self.assertEqual(raised.exception.code, models.ExitCode.WRITE_ERROR)

    def test_atomic_publication_links_the_verified_temporary(self) -> None:
        with tempfile.TemporaryDirectory() as directory:
            temporary = Path(directory) / "temporary.eml"
            destination = Path(directory) / "destination.eml"
            temporary.write_bytes(b"verified")
            output_commit._publish_without_clobber(temporary, destination)

            self.assertEqual(destination.read_bytes(), b"verified")
            self.assertFalse(temporary.exists())

    def test_forced_commit_cannot_target_source_alias(self) -> None:
        with tempfile.TemporaryDirectory() as directory:
            source = Path(directory) / "source.eml"
            source.write_bytes(b"public")
            with self.assertRaises(models.CliError) as raised:
                output_commit._validate_forced_commit(source, source)

        self.assertEqual(raised.exception.code, models.ExitCode.OUTPUT_CONFLICT)

    def test_forced_commit_cannot_target_directory(self) -> None:
        with tempfile.TemporaryDirectory() as directory:
            base = Path(directory)
            source = base / "source.eml"
            source.write_bytes(b"public")
            destination = base / "destination.eml"
            destination.mkdir()
            with self.assertRaises(models.CliError) as raised:
                output_commit._validate_forced_commit(source, destination)

        self.assertEqual(raised.exception.code, models.ExitCode.OUTPUT_CONFLICT)

    def test_forced_commit_replace_error_is_write_error(self) -> None:
        with tempfile.TemporaryDirectory() as directory:
            base = Path(directory)
            source = base / "source.eml"
            source.write_bytes(b"source")
            temporary = base / "temporary.eml"
            temporary.write_bytes(b"public")
            destination = base / "destination.eml"
            destination.write_bytes(b"existing")
            with patch.object(Path, "replace", side_effect=OSError("blocked")):
                with self.assertRaises(models.CliError) as raised:
                    output_commit._commit_output(
                        temporary,
                        destination,
                        source,
                        force=True,
                    )

        self.assertEqual(raised.exception.code, models.ExitCode.WRITE_ERROR)

    def test_unforced_commit_failure_leaves_paths_unchanged(self) -> None:
        with tempfile.TemporaryDirectory() as directory:
            base = Path(directory)
            source = base / "source.eml"
            source.write_bytes(b"source")
            destination = base / "destination.eml"
            temporary = base / "temporary.eml"
            temporary.write_bytes(b"temporary")
            with (
                patch.object(
                    atomic_publish,
                    "_native_no_replace",
                    return_value=False,
                ),
                patch.object(Path, "hardlink_to", side_effect=OSError("blocked")),
            ):
                with self.assertRaises(models.CliError) as raised:
                    output_commit._commit_output(
                        temporary,
                        destination,
                        source,
                        force=False,
                    )

            self.assertFalse(destination.exists())
            self.assertEqual(temporary.read_bytes(), b"temporary")
        self.assertEqual(raised.exception.code, models.ExitCode.WRITE_ERROR)

    def test_serialized_output_read_error_is_verification_error(self) -> None:
        with patch.object(Path, "read_bytes", side_effect=OSError("blocked")):
            with self.assertRaises(models.CliError) as raised:
                mime_serialization._verify_serialized_message(
                    Path("unreadable.eml"),
                    Counter(),
                )

        self.assertEqual(raised.exception.code, models.ExitCode.VERIFICATION_ERROR)

    def test_temporary_size_error_is_write_error(self) -> None:
        with tempfile.TemporaryDirectory() as directory:
            base = Path(directory)
            source = base / "source.eml"
            source.write_bytes(b"source")
            temporary = base / "temporary.eml"
            temporary.write_bytes(b"public")
            plan = models.OutputPlan(
                source=source,
                destination=base / "destination.eml",
                message=EmailMessage(),
                raw=b"public",
                modified=False,
                force=False,
            )
            with (
                patch.object(storage, "_write_temporary", return_value=temporary),
                patch.object(storage, "_apply_source_mode", return_value=None),
                patch.object(Path, "stat", side_effect=OSError("blocked")),
            ):
                with self.assertRaises(models.CliError) as raised:
                    storage._produce_output(plan)

        self.assertEqual(raised.exception.code, models.ExitCode.WRITE_ERROR)

    def test_process_file_read_error_is_input_error(self) -> None:
        with tempfile.TemporaryDirectory() as directory:
            source = Path(directory) / "source.eml"
            source.write_bytes(b"public")
            with patch.object(os, "open", side_effect=OSError("blocked")):
                with self.assertRaises(models.CliError) as raised:
                    processing.process_file(
                        source,
                        Path(directory) / "destination.eml",
                        force=False,
                        dry_run=True,
                    )

        self.assertEqual(raised.exception.code, models.ExitCode.INPUT_ERROR)

    def test_removed_attachment_warns_about_transport_signature(self) -> None:
        with tempfile.TemporaryDirectory() as directory:
            source = Path(directory) / "source.eml"
            message = EmailMessage()
            message["DKIM-Signature"] = "public"
            message.set_content("Public body")
            message.add_attachment(
                b"PUBLIC",
                maintype="application",
                subtype="octet-stream",
                filename="public.bin",
            )
            source.write_bytes(message.as_bytes())

            result = processing.process_file(source, None, force=False, dry_run=True)

        self.assertTrue(
            any("transport signatures" in warning for warning in result.warnings),
        )

    def test_output_mode_warning_is_reported(self) -> None:
        with tempfile.TemporaryDirectory() as directory:
            base = Path(directory)
            source = base / "source.eml"
            message = EmailMessage()
            message.set_content("Public body")
            source.write_bytes(message.as_bytes())
            with patch.object(
                processing,
                "_produce_output",
                return_value=(1, "mode warning"),
            ):
                result = processing.process_file(
                    source,
                    base / "output.eml",
                    force=False,
                    dry_run=False,
                )

        self.assertIn("mode warning", result.warnings)
