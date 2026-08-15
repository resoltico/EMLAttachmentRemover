"""Direct tests for reporting and batch-orchestration error paths."""

from __future__ import annotations

import io
import platform
import sys
import tempfile
import unittest
from pathlib import Path
from unittest.mock import call, patch

from eml_attachment_remover import cli, models, paths, reporting


def _process_result() -> models.ProcessResult:
    return models.ProcessResult(
        source=Path("public.eml"),
        destination=Path("output.eml"),
        source_size=1,
        output_size=1,
        removed=(),
        preserved_file_parts=(),
        warnings=(),
        dry_run=False,
    )


class ReportingAndCliEdgeCaseTests(unittest.TestCase):
    """Exercise one reporting or command-line contract per test."""

    def test_json_output_is_detected_in_equals_form(self) -> None:
        self.assertTrue(cli._json_output_requested(["--output-format=json"]))

    def test_incomplete_output_format_is_not_json(self) -> None:
        self.assertFalse(cli._json_output_requested(["--output-format"]))

    def test_dry_run_rejects_path_only_output(self) -> None:
        with self.assertRaises(models.CliError) as raised:
            cli._validate_cli_arguments(
                [Path("public.eml")],
                None,
                models.OutputFormat.PATHS,
                dry_run=True,
            )

        self.assertEqual(raised.exception.code, models.ExitCode.USAGE)

    def test_result_writer_emits_removal_summary(self) -> None:
        stream = io.StringIO()
        with patch.object(reporting, "_write_line") as write_line:
            reporting._write_result(stream, _process_result())

        self.assertIn(
            call(stream, "Removed 0 attachment(s)."),
            write_line.call_args_list,
        )

    def test_skip_data_has_skipped_status(self) -> None:
        skip = models.BatchSkip(Path("source.eml"), Path("destination.eml"))

        self.assertEqual(reporting._skip_data(skip)["status"], "skipped")

    def test_macos_collision_key_is_case_insensitive(self) -> None:
        with patch.object(platform, "system", return_value="Darwin"):
            key = paths._path_collision_key(Path("Public"))

        self.assertEqual(key, "public")

    def test_linux_collision_key_preserves_case(self) -> None:
        with patch.object(platform, "system", return_value="Linux"):
            key = paths._path_collision_key(Path("Public"))

        self.assertEqual(key, "Public")

    def test_human_batch_writes_single_result_without_source_header(self) -> None:
        result = _process_result()
        with patch.object(reporting, "_write_line") as write_line:
            reporting._write_human_batch([result], [], [], multiple=False)

        self.assertNotIn(
            call(sys.stdout, f"Source: {result.source}"),
            write_line.call_args_list,
        )

    def test_human_batch_separates_multiple_results(self) -> None:
        result = _process_result()
        with patch.object(reporting, "_write_line") as write_line:
            reporting._write_human_batch(
                [result, result],
                [],
                [],
                multiple=False,
            )

        self.assertIn(call(sys.stdout, ""), write_line.call_args_list)

    def test_human_batch_writes_skip_without_leading_separator(self) -> None:
        skip = models.BatchSkip(Path("source.eml"), Path("destination.eml"))
        with patch.object(reporting, "_write_line") as write_line:
            reporting._write_human_batch([], [skip], [], multiple=False)

        self.assertEqual(
            write_line.call_args_list[0],
            call(
                sys.stdout,
                "Skipped existing output for source.eml: destination.eml",
            ),
        )

    def test_non_fail_fast_batch_continues_after_directory_conflict(self) -> None:
        with tempfile.TemporaryDirectory() as directory:
            base = Path(directory)
            first_source = base / "first.eml"
            first_source.write_bytes(b"public")
            second_source = base / "second.eml"
            second_source.write_bytes(b"public")
            first_destination = base / "directory"
            first_destination.mkdir()
            second_destination = base / "second-output.eml"
            with patch.object(
                cli,
                "process_file",
                return_value=_process_result(),
            ) as process_file:
                outcome = cli._execute_plans(
                    [
                        (first_source, first_destination),
                        (second_source, second_destination),
                    ],
                    dry_run=False,
                    force=False,
                    skip_existing=True,
                    fail_fast=False,
                )

        self.assertEqual(len(outcome.failures), 1)
        self.assertEqual(len(outcome.results), 1)
        process_file.assert_called_once()

    def test_fail_fast_batch_stops_after_directory_conflict(self) -> None:
        with tempfile.TemporaryDirectory() as directory:
            base = Path(directory)
            first_source = base / "first.eml"
            first_source.write_bytes(b"public")
            second_source = base / "second.eml"
            second_source.write_bytes(b"public")
            first_destination = base / "directory"
            first_destination.mkdir()
            with patch.object(cli, "process_file") as process_file:
                outcome = cli._execute_plans(
                    [
                        (first_source, first_destination),
                        (second_source, base / "second-output.eml"),
                    ],
                    dry_run=False,
                    force=False,
                    skip_existing=True,
                    fail_fast=True,
                )

        self.assertEqual(len(outcome.failures), 1)
        self.assertFalse(outcome.results)
        process_file.assert_not_called()

    def test_failure_writer_delegates_to_error_renderer(self) -> None:
        failure = models.BatchFailure(
            Path("source.eml"),
            models.ExitCode.INPUT_ERROR,
            "public",
        )
        with patch.object(cli, "_write_error") as write_error:
            cli._write_failures((failure,))

        self.assertEqual(write_error.call_count, 1)

    def test_keyboard_interrupt_returns_interrupt_code(self) -> None:
        with (
            patch.object(cli, "_run_batch", side_effect=KeyboardInterrupt),
            patch.object(sys, "stderr", io.StringIO()),
        ):
            exit_code = cli.main(["public.eml"])

        self.assertEqual(exit_code, 130)

    def test_unexpected_exception_returns_internal_error(self) -> None:
        with (
            patch.object(cli, "_run_batch", side_effect=RuntimeError("public")),
            patch.object(sys, "stderr", io.StringIO()),
        ):
            exit_code = cli.main(["public.eml"])

        self.assertEqual(exit_code, models.ExitCode.INTERNAL_ERROR)
