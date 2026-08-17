"""Black-box tests for multi-file command-line behavior."""

from __future__ import annotations

import json
import os
import tempfile
import unittest
from pathlib import Path

from tests.test_support import run_cli, run_cli_bytes, simple_message


class BatchTests(unittest.TestCase):
    def test_two_sources_are_processed_in_one_invocation(self) -> None:
        with tempfile.TemporaryDirectory() as directory:
            base = Path(directory)
            first = base / "first.eml"
            second = base / "second.eml"
            first.write_bytes(simple_message().as_bytes())
            second.write_bytes(simple_message().as_bytes())
            result = run_cli("--output-format", "paths", str(first), str(second))
            self.assertEqual(result.returncode, 0, result.stderr)
            paths = [Path(line) for line in result.stdout.splitlines()]
            self.assertEqual(
                paths,
                [
                    base.resolve() / "first.text-only.eml",
                    base.resolve() / "second.text-only.eml",
                ],
            )
            self.assertTrue(all(path.is_file() for path in paths))

    def test_partial_batch_failure_returns_code_9(self) -> None:
        with tempfile.TemporaryDirectory() as directory:
            base = Path(directory)
            valid = base / "valid.eml"
            missing = base / "missing.eml"
            valid.write_bytes(simple_message().as_bytes())
            result = run_cli(str(valid), str(missing))
            self.assertEqual(result.returncode, 9)
            self.assertTrue((base / "valid.text-only.eml").is_file())
            self.assertIn("error[INPUT_ERROR:3]", result.stderr)

    def test_fail_fast_returns_specific_code_and_stops(self) -> None:
        with tempfile.TemporaryDirectory() as directory:
            base = Path(directory)
            missing = base / "missing.eml"
            valid = base / "valid.eml"
            valid.write_bytes(simple_message().as_bytes())
            result = run_cli("--fail-fast", str(missing), str(valid))
            self.assertEqual(result.returncode, 3)
            self.assertFalse((base / "valid.text-only.eml").exists())

    def test_explicit_output_with_multiple_sources_is_usage_error(self) -> None:
        result = run_cli("-o", "out.eml", "one.eml", "two.eml")
        self.assertEqual(result.returncode, 2)
        self.assertIn("--output may be used only", result.stderr)

    def test_batch_level_json_error_remains_valid_json(self) -> None:
        result = run_cli(
            "--output-format",
            "json",
            "-o",
            "out.eml",
            "one.eml",
            "two.eml",
        )
        self.assertEqual(result.returncode, 2)
        self.assertEqual(result.stderr, "")
        report = json.loads(result.stdout)
        self.assertFalse(report["ok"])
        self.assertEqual(report["schema_version"], 2)
        self.assertEqual(report["scope"], "text-only")
        self.assertEqual(report["errors"][0]["error"]["code"], 2)

    def test_output_directory_collision_is_rejected_before_writing(self) -> None:
        with tempfile.TemporaryDirectory() as directory:
            base = Path(directory)
            first_directory = base / "one"
            second_directory = base / "two"
            output_directory = base / "out"
            first_directory.mkdir()
            second_directory.mkdir()
            output_directory.mkdir()
            first = first_directory / "same.eml"
            second = second_directory / "same.eml"
            first.write_bytes(simple_message().as_bytes())
            second.write_bytes(simple_message().as_bytes())
            result = run_cli(
                "--output-dir",
                str(output_directory),
                str(first),
                str(second),
            )
            self.assertEqual(result.returncode, 4)
            self.assertEqual(list(output_directory.iterdir()), [])

    def test_output_directory_writes_each_derived_file_with_its_source_name(
        self,
    ) -> None:
        with tempfile.TemporaryDirectory() as directory:
            base = Path(directory)
            output_directory = base / "out"
            output_directory.mkdir()
            first = base / "first.eml"
            second = base / "second.eml"
            first.write_bytes(simple_message().as_bytes())
            second.write_bytes(simple_message().as_bytes())
            result = run_cli(
                "--output-dir",
                str(output_directory),
                str(first),
                str(second),
            )
            self.assertEqual(result.returncode, 0, result.stderr)
            self.assertEqual(
                sorted(path.name for path in output_directory.iterdir()),
                ["first.text-only.eml", "second.text-only.eml"],
            )

    def test_json_report_contains_successes_and_errors(self) -> None:
        with tempfile.TemporaryDirectory() as directory:
            base = Path(directory)
            valid = base / "valid.eml"
            missing = base / "missing.eml"
            valid.write_bytes(simple_message().as_bytes())
            result = run_cli("--output-format", "json", str(valid), str(missing))
            self.assertEqual(result.returncode, 9)
            report = json.loads(result.stdout)
            self.assertFalse(report["ok"])
            self.assertEqual(report["schema_version"], 2)
            self.assertEqual(report["scope"], "text-only")
            self.assertEqual(len(report["results"]), 1)
            self.assertEqual(len(report["errors"]), 1)
            self.assertEqual(report["errors"][0]["error"]["code"], 3)

    def test_skip_existing_reports_the_existing_destination(self) -> None:
        with tempfile.TemporaryDirectory() as directory:
            base = Path(directory)
            source = base / "source.eml"
            output = base / "source.text-only.eml"
            source.write_bytes(simple_message().as_bytes())
            output.write_bytes(b"existing")
            result = run_cli(
                "--skip-existing",
                "--output-format",
                "paths",
                str(source),
            )
            self.assertEqual(result.returncode, 0, result.stderr)
            self.assertEqual(result.stdout.strip(), str(output.resolve()))
            self.assertEqual(output.read_bytes(), b"existing")

    @unittest.skipIf(os.name == "nt", "newline filenames are not portable to Windows")
    def test_paths0_preserves_newlines_in_filenames(self) -> None:
        with tempfile.TemporaryDirectory() as directory:
            base = Path(directory)
            source = base / "line one\nline two.eml"
            source.write_bytes(simple_message().as_bytes())
            result = run_cli_bytes("--output-format", "paths0", str(source))
            self.assertEqual(result.returncode, 0, result.stderr.decode())
            self.assertEqual(
                result.stdout,
                os.fsencode(base.resolve() / "line one\nline two.text-only.eml")
                + b"\0",
            )
