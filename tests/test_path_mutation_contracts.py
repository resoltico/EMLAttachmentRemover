# ruff: file-ignore[private-member-access]
"""Mutation-focused contracts for path validation and atomic placement."""

from __future__ import annotations

import os
import tempfile
import unittest
from pathlib import Path
from unittest.mock import patch

from eml_attachment_remover import atomic_publish, models, output_commit, paths


class PathDiagnosticContractTests(unittest.TestCase):
    """Require actionable messages at every filesystem failure boundary."""

    def test_source_inspection_failure_names_path_and_cause(self) -> None:
        source = Path("synthetic-source.eml").absolute()
        with patch.object(Path, "stat", side_effect=OSError("inspection blocked")):
            with self.assertRaises(models.CliError) as raised:
                paths._validate_source(source)
        self.assertEqual(raised.exception.code, models.ExitCode.INPUT_ERROR)
        self.assertEqual(
            raised.exception.message,
            f"could not inspect input path {source}: inspection blocked",
        )

    def test_non_regular_source_message_names_the_source(self) -> None:
        with tempfile.TemporaryDirectory() as directory:
            source = Path(directory).absolute()
            with self.assertRaises(models.CliError) as raised:
                paths._validate_source(source)
        self.assertEqual(
            raised.exception.message,
            f"input path is not a regular file: {source}",
        )

    def test_destination_inspection_failure_names_path_and_cause(self) -> None:
        destination = Path("synthetic-output.eml")
        with patch.object(Path, "lstat", side_effect=OSError("inspection blocked")):
            with self.assertRaises(models.CliError) as raised:
                paths._destination_exists(destination)
        self.assertEqual(raised.exception.code, models.ExitCode.WRITE_ERROR)
        self.assertEqual(
            raised.exception.message,
            f"could not inspect output path {destination}: inspection blocked",
        )

    def test_destination_parent_errors_distinguish_failure_types(self) -> None:
        destination = Path("synthetic-parent") / "output.eml"
        with patch.object(Path, "stat", side_effect=OSError("inspection blocked")):
            with self.assertRaises(models.CliError) as inspection:
                paths._validate_destination_parent(destination)
        self.assertEqual(
            inspection.exception.message,
            "could not inspect output directory synthetic-parent: inspection blocked",
        )

        with tempfile.TemporaryDirectory() as directory:
            parent = Path(directory) / "regular-file"
            parent.write_bytes(b"PUBLIC")
            with self.assertRaises(models.CliError) as wrong_type:
                paths._validate_destination_parent(parent / "output.eml")
        self.assertEqual(
            wrong_type.exception.message,
            f"output parent is not a directory: {parent}",
        )

    def test_destination_resolution_failure_names_path_and_cause(self) -> None:
        source = Path("synthetic-source.eml")
        destination = Path("synthetic-output.eml")
        with (
            patch.object(paths, "_validate_source", return_value=source),
            patch.object(paths, "_absolute_path", side_effect=OSError("blocked")),
        ):
            with self.assertRaises(models.CliError) as raised:
                paths._validate_paths(source, destination, force=False)
        self.assertEqual(raised.exception.code, models.ExitCode.WRITE_ERROR)
        self.assertEqual(
            raised.exception.message,
            f"could not resolve output path {destination}: blocked",
        )

    def test_distinct_hard_link_alias_has_canonical_conflict_message(self) -> None:
        with tempfile.TemporaryDirectory() as directory:
            source = Path(directory) / "source.eml"
            alias = Path(directory) / "alias.eml"
            source.write_bytes(b"PUBLIC")
            os.link(source, alias)
            with self.assertRaises(models.CliError) as raised:
                paths._validate_paths(source, alias, force=True)
        self.assertEqual(raised.exception.code, models.ExitCode.OUTPUT_CONFLICT)
        self.assertEqual(
            raised.exception.message,
            "refusing to overwrite the source EML directly or through an alias",
        )

    def test_existing_destination_message_gives_remediation(self) -> None:
        with tempfile.TemporaryDirectory() as directory:
            source = Path(directory) / "source.eml"
            destination = Path(directory) / "output.eml"
            source.write_bytes(b"PUBLIC SOURCE")
            destination.write_bytes(b"PUBLIC OUTPUT")
            with self.assertRaises(models.CliError) as raised:
                paths._validate_paths(source, destination, force=False)
        self.assertEqual(
            raised.exception.message,
            "output already exists: "
            f"{destination.parent.resolve() / destination.name}; "
            "use --force to replace it",
        )

    def test_destination_directory_message_names_the_path(self) -> None:
        with tempfile.TemporaryDirectory() as directory:
            source = Path(directory) / "source.eml"
            destination = Path(directory) / "output.eml"
            source.write_bytes(b"PUBLIC")
            destination.mkdir()
            with self.assertRaises(models.CliError) as raised:
                paths._validate_paths(source, destination, force=True)
        self.assertEqual(
            raised.exception.message,
            f"output path is a directory: "
            f"{destination.parent.resolve() / destination.name}",
        )

    def test_output_directory_errors_are_actionable(self) -> None:
        with tempfile.TemporaryDirectory() as directory:
            missing = Path(directory) / "missing"
            with self.assertRaises(models.CliError) as missing_error:
                paths._validate_output_directory(missing)
            self.assertEqual(
                missing_error.exception.message,
                f"output directory does not exist: {missing}",
            )

            regular = Path(directory) / "regular"
            regular.write_bytes(b"PUBLIC")
            with self.assertRaises(models.CliError) as type_error:
                paths._validate_output_directory(regular)
            self.assertEqual(
                type_error.exception.message,
                f"output path is not a directory: {regular}",
            )

    def test_output_directory_inspection_error_preserves_context(self) -> None:
        candidate = Path("synthetic-output-directory")
        with (
            patch.object(paths, "_absolute_path", return_value=candidate),
            patch.object(Path, "stat", side_effect=OSError("inspection blocked")),
        ):
            with self.assertRaises(models.CliError) as raised:
                paths._validate_output_directory(candidate)
        self.assertEqual(
            raised.exception.message,
            "could not inspect output directory synthetic-output-directory: "
            "inspection blocked",
        )


class AtomicPlacementContractTests(unittest.TestCase):
    """Require atomic no-clobber publication and safe commit failures."""

    def test_publication_uses_destination_hard_link_to_temporary(self) -> None:
        temporary = Path("synthetic-temporary.eml")
        destination = Path("synthetic-output.eml")
        with (
            patch.object(atomic_publish.os, "name", new="posix"),  # type: ignore[attr-defined]
            patch.object(atomic_publish, "_native_no_replace", return_value=False),
            patch.object(Path, "hardlink_to") as hardlink,
            patch.object(Path, "unlink") as unlink,
        ):
            output_commit._publish_without_clobber(temporary, destination)
        self.assertEqual(hardlink.call_args.args, (temporary,))
        unlink.assert_called_once_with()

    def test_publication_errors_preserve_conflict_or_write_context(self) -> None:
        temporary = Path("synthetic-temporary.eml")
        destination = Path("synthetic-output.eml")
        with (
            patch.object(atomic_publish.os, "name", new="posix"),  # type: ignore[attr-defined]
            patch.object(atomic_publish, "_native_no_replace", return_value=False),
            patch.object(
                Path,
                "hardlink_to",
                side_effect=FileExistsError("occupied"),
            ),
        ):
            with self.assertRaises(models.CliError) as conflict:
                output_commit._publish_without_clobber(temporary, destination)
        self.assertEqual(conflict.exception.code, models.ExitCode.OUTPUT_CONFLICT)
        self.assertEqual(
            conflict.exception.message,
            f"output was created by another process: {destination}",
        )

        with (
            patch.object(atomic_publish.os, "name", new="posix"),  # type: ignore[attr-defined]
            patch.object(atomic_publish, "_native_no_replace", return_value=False),
            patch.object(Path, "hardlink_to", side_effect=OSError("publish blocked")),
        ):
            with self.assertRaises(models.CliError) as write_error:
                output_commit._publish_without_clobber(temporary, destination)
        self.assertEqual(write_error.exception.code, models.ExitCode.WRITE_ERROR)
        self.assertEqual(
            write_error.exception.message,
            f"could not publish verified output at {destination}: publish blocked",
        )

    def test_platform_permission_error_is_conflict_when_entry_exists(self) -> None:
        """Map Windows-style existing-target errors without changing the target."""
        with tempfile.TemporaryDirectory() as directory:
            temporary = Path(directory) / "temporary.eml"
            destination = Path(directory) / "destination.eml"
            temporary.write_bytes(b"PUBLIC DERIVED")
            destination.write_bytes(b"PUBLIC RACE WINNER")
            with (
                patch.object(atomic_publish.os, "name", new="posix"),  # type: ignore[attr-defined]
                patch.object(
                    atomic_publish,
                    "_native_no_replace",
                    return_value=False,
                ),
                patch.object(
                    Path,
                    "hardlink_to",
                    side_effect=PermissionError("target exists"),
                ),
            ):
                with self.assertRaises(models.CliError) as raised:
                    output_commit._publish_without_clobber(temporary, destination)

            self.assertEqual(raised.exception.code, models.ExitCode.OUTPUT_CONFLICT)
            self.assertEqual(destination.read_bytes(), b"PUBLIC RACE WINNER")
            self.assertEqual(temporary.read_bytes(), b"PUBLIC DERIVED")

    def test_late_force_conflicts_have_canonical_messages(self) -> None:
        with tempfile.TemporaryDirectory() as directory:
            source = Path(directory) / "source.eml"
            alias = Path(directory) / "alias.eml"
            source.write_bytes(b"PUBLIC")
            os.link(source, alias)
            with self.assertRaises(models.CliError) as alias_error:
                output_commit._validate_forced_commit(source, alias)
            self.assertEqual(
                alias_error.exception.message,
                "refusing to overwrite the source EML through a late alias",
            )

            destination = Path(directory) / "destination.eml"
            destination.mkdir()
            with self.assertRaises(models.CliError) as directory_error:
                output_commit._validate_forced_commit(source, destination)
            self.assertEqual(
                directory_error.exception.message,
                f"output path became a directory: {destination}",
            )

    def test_unforced_commit_never_clobbers_a_race_winner(self) -> None:
        with tempfile.TemporaryDirectory() as directory:
            source = Path(directory) / "source.eml"
            temporary = Path(directory) / "temporary.eml"
            destination = Path(directory) / "destination.eml"
            source.write_bytes(b"PUBLIC SOURCE")
            temporary.write_bytes(b"PUBLIC DERIVED")
            destination.write_bytes(b"PUBLIC RACE WINNER")
            with self.assertRaises(models.CliError) as raised:
                output_commit._commit_output(
                    temporary,
                    destination,
                    source,
                    force=False,
                )
            self.assertEqual(destination.read_bytes(), b"PUBLIC RACE WINNER")
            self.assertEqual(temporary.read_bytes(), b"PUBLIC DERIVED")
        self.assertEqual(raised.exception.code, models.ExitCode.OUTPUT_CONFLICT)

    def test_injected_race_winner_is_never_overwritten_or_unlinked(self) -> None:
        with tempfile.TemporaryDirectory() as directory:
            source = Path(directory) / "source.eml"
            temporary = Path(directory) / "temporary.eml"
            destination = Path(directory) / "destination.eml"
            source.write_bytes(b"PUBLIC SOURCE")
            temporary.write_bytes(b"PUBLIC DERIVED")
            original_hardlink = Path.hardlink_to

            def race_then_link(link: Path, target: Path) -> None:
                destination.write_bytes(b"PUBLIC RACE WINNER")
                original_hardlink(link, target)

            with (
                patch.object(atomic_publish.os, "name", new="posix"),  # type: ignore[attr-defined]
                patch.object(
                    atomic_publish,
                    "_native_no_replace",
                    return_value=False,
                ),
                patch.object(
                    Path,
                    "hardlink_to",
                    autospec=True,
                    side_effect=race_then_link,
                ),
            ):
                with self.assertRaises(models.CliError) as raised:
                    output_commit._commit_output(
                        temporary,
                        destination,
                        source,
                        force=False,
                    )

            self.assertEqual(raised.exception.code, models.ExitCode.OUTPUT_CONFLICT)
            self.assertEqual(destination.read_bytes(), b"PUBLIC RACE WINNER")
            self.assertEqual(temporary.read_bytes(), b"PUBLIC DERIVED")

    def test_failed_forced_replace_preserves_existing_destination(self) -> None:
        with tempfile.TemporaryDirectory() as directory:
            source = Path(directory) / "source.eml"
            temporary = Path(directory) / "temporary.eml"
            destination = Path(directory) / "destination.eml"
            source.write_bytes(b"PUBLIC SOURCE")
            temporary.write_bytes(b"PUBLIC DERIVED")
            destination.write_bytes(b"PUBLIC ORIGINAL OUTPUT")
            with patch.object(Path, "replace", side_effect=OSError("move blocked")):
                with self.assertRaises(models.CliError) as raised:
                    output_commit._commit_output(
                        temporary,
                        destination,
                        source,
                        force=True,
                    )
            self.assertEqual(destination.read_bytes(), b"PUBLIC ORIGINAL OUTPUT")
        self.assertEqual(raised.exception.code, models.ExitCode.WRITE_ERROR)
        self.assertEqual(
            raised.exception.message,
            f"could not move verified output into place at {destination}: move blocked",
        )

    def test_publication_failure_does_not_attempt_destination_cleanup(self) -> None:
        with tempfile.TemporaryDirectory() as directory:
            source = Path(directory) / "source.eml"
            temporary = Path(directory) / "temporary.eml"
            destination = Path(directory) / "destination.eml"
            source.write_bytes(b"PUBLIC SOURCE")
            temporary.write_bytes(b"PUBLIC DERIVED")
            with (
                patch.object(atomic_publish.os, "name", new="posix"),  # type: ignore[attr-defined]
                patch.object(atomic_publish, "_native_no_replace", return_value=False),
                patch.object(Path, "hardlink_to", side_effect=OSError("link blocked")),
                patch.object(Path, "unlink") as unlink,
            ):
                with self.assertRaises(models.CliError) as raised:
                    output_commit._commit_output(
                        temporary,
                        destination,
                        source,
                        force=False,
                    )
            unlink.assert_not_called()
        self.assertEqual(raised.exception.code, models.ExitCode.WRITE_ERROR)
        self.assertIn("link blocked", raised.exception.message)


if __name__ == "__main__":
    unittest.main(verbosity=2)
