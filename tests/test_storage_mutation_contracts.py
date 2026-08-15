# ruff: file-ignore[private-member-access]
"""Mutation-focused contracts for temporary output storage."""

from __future__ import annotations

import tempfile
import unittest
from email.message import EmailMessage
from pathlib import Path
from unittest.mock import patch

from eml_attachment_remover import models, storage


def _output_plan(base: Path, *, raw: bytes = b"PUBLIC") -> models.OutputPlan:
    """Return an unmodified synthetic output plan.

    Returns:
        A plan whose source and destination share the supplied directory.

    """
    source = base / "source.eml"
    source.write_bytes(raw)
    message = EmailMessage()
    message.set_content("Public body")
    return models.OutputPlan(
        source=source,
        destination=base / "destination.eml",
        message=message,
        raw=raw,
        modified=False,
        force=False,
    )


class TemporaryStorageContractTests(unittest.TestCase):
    """Require same-directory temporary files and actionable failures."""

    def test_temporary_name_location_and_unmodified_bytes_are_stable(self) -> None:
        with tempfile.TemporaryDirectory() as directory:
            destination = Path(directory) / "destination.eml"
            temporary = storage._write_temporary(
                destination,
                EmailMessage(),
                b"PUBLIC WIRE BYTES",
                modified=False,
            )
            try:
                self.assertEqual(temporary.parent, destination.parent)
                self.assertTrue(temporary.name.startswith(".eml-remove-"))
                self.assertTrue(temporary.name.endswith(".tmp"))
                self.assertEqual(temporary.read_bytes(), b"PUBLIC WIRE BYTES")
            finally:
                temporary.unlink(missing_ok=True)

    def test_creation_failure_names_destination_directory_and_cause(self) -> None:
        destination = Path("synthetic-directory") / "destination.eml"
        with patch.object(tempfile, "mkstemp", side_effect=OSError("create blocked")):
            with self.assertRaises(models.CliError) as raised:
                storage._write_temporary(
                    destination,
                    EmailMessage(),
                    b"PUBLIC",
                    modified=False,
                )
        self.assertEqual(raised.exception.code, models.ExitCode.WRITE_ERROR)
        self.assertEqual(
            raised.exception.message,
            "could not create a temporary file in synthetic-directory: create blocked",
        )

    def test_write_failure_names_temporary_and_removes_it(self) -> None:
        with tempfile.TemporaryDirectory() as directory:
            destination = Path(directory) / "destination.eml"
            with patch.object(
                storage,
                "_write_open_temporary",
                side_effect=OSError("write blocked"),
            ):
                with self.assertRaises(models.CliError) as raised:
                    storage._write_temporary(
                        destination,
                        EmailMessage(),
                        b"PUBLIC",
                        modified=False,
                    )
            temporary_text = raised.exception.message.removeprefix(
                "could not write temporary output ",
            ).removesuffix(": write blocked")
            temporary = Path(temporary_text)
            self.assertEqual(temporary.parent, destination.parent)
            self.assertFalse(temporary.exists())
        self.assertEqual(raised.exception.code, models.ExitCode.WRITE_ERROR)
        self.assertEqual(
            raised.exception.message,
            f"could not write temporary output {temporary}: write blocked",
        )

    def test_cleanup_failure_reports_the_sensitive_temporary_leak(self) -> None:
        with tempfile.TemporaryDirectory() as directory:
            destination = Path(directory) / "destination.eml"
            with (
                patch.object(
                    storage,
                    "_write_open_temporary",
                    side_effect=OSError("write blocked"),
                ),
                patch.object(Path, "unlink", side_effect=OSError("cleanup blocked")),
            ):
                with self.assertRaises(models.CliError) as raised:
                    storage._write_temporary(
                        destination,
                        EmailMessage(),
                        b"PUBLIC",
                        modified=False,
                    )
        self.assertEqual(raised.exception.code, models.ExitCode.WRITE_ERROR)
        self.assertIn("write blocked", raised.exception.message)
        self.assertIn(
            "could not remove sensitive temporary file", raised.exception.message
        )

    def test_size_failure_names_temporary_and_cause(self) -> None:
        temporary = Path("synthetic-temporary.eml")
        with patch.object(Path, "stat", side_effect=OSError("inspection blocked")):
            with self.assertRaises(models.CliError) as raised:
                storage._temporary_size(temporary)
        self.assertEqual(raised.exception.code, models.ExitCode.WRITE_ERROR)
        self.assertEqual(
            raised.exception.message,
            f"could not inspect temporary output {temporary}: inspection blocked",
        )


class OutputProductionContractTests(unittest.TestCase):
    """Require warning propagation, atomic commit, and best-effort cleanup."""

    def test_permission_warning_and_final_size_are_returned(self) -> None:
        raw = b"PUBLIC BYTE-EXACT MESSAGE"
        with tempfile.TemporaryDirectory() as directory:
            base = Path(directory)
            plan = _output_plan(base, raw=raw)
            with patch.object(
                storage,
                "_apply_source_mode",
                return_value="permission copy blocked",
            ):
                output_size, warning = storage._produce_output(plan)
            self.assertEqual(plan.destination.read_bytes(), raw)
        self.assertEqual(output_size, len(raw))
        self.assertEqual(warning, "permission copy blocked")

    def test_cleanup_failure_after_publication_is_an_actionable_error(self) -> None:
        with tempfile.TemporaryDirectory() as directory:
            base = Path(directory)
            plan = _output_plan(base)
            temporary = base / "temporary.eml"
            temporary.write_bytes(b"PUBLIC")
            with (
                patch.object(storage, "_write_temporary", return_value=temporary),
                patch.object(storage, "_apply_source_mode", return_value=None),
                patch.object(storage, "_temporary_size", return_value=6),
                patch.object(storage, "_commit_output"),
                patch.object(Path, "unlink", side_effect=OSError("cleanup blocked")),
            ):
                with self.assertRaises(models.CliError) as raised:
                    storage._produce_output(plan)
        self.assertEqual(raised.exception.code, models.ExitCode.WRITE_ERROR)
        self.assertEqual(
            raised.exception.message,
            f"could not remove sensitive temporary file {temporary} "
            "(published output is valid): cleanup blocked",
        )

    def test_unchanged_output_corruption_is_rejected_before_commit(self) -> None:
        with tempfile.TemporaryDirectory() as directory:
            base = Path(directory)
            plan = _output_plan(base, raw=b"PUBLIC ORIGINAL")
            temporary = base / "temporary.eml"
            temporary.write_bytes(b"PUBLIC CORRUPTION")
            with (
                patch.object(storage, "_write_temporary", return_value=temporary),
                patch.object(storage, "_commit_output") as commit,
            ):
                with self.assertRaises(models.CliError) as raised:
                    storage._produce_output(plan)

            self.assertFalse(plan.destination.exists())
            commit.assert_not_called()
        self.assertEqual(raised.exception.code, models.ExitCode.VERIFICATION_ERROR)


if __name__ == "__main__":
    unittest.main(verbosity=2)
