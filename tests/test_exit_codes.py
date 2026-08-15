# ruff: file-ignore[private-member-access]
"""Black-box tests for documented CLI failure codes."""

from __future__ import annotations

import os
import tempfile
import unittest
from pathlib import Path
from unittest.mock import patch

from eml_attachment_remover import models, paths
from tests.test_support import parse, run_cli, simple_message


def _assert_mocked_alias_rejected(
    test_case: unittest.TestCase,
    source: Path,
    alias: Path,
) -> None:
    """Exercise the portable alias guard when link creation is unavailable."""
    with (
        patch.object(paths, "_paths_alias", return_value=True),
        test_case.assertRaises(models.CliError) as raised,
    ):
        paths._validate_paths(source, alias, force=True)
    test_case.assertEqual(raised.exception.code, models.ExitCode.OUTPUT_CONFLICT)


class ExitCodeTests(unittest.TestCase):
    def test_missing_input_returns_code_3(self) -> None:
        result = run_cli("/definitely/missing/message.eml")
        self.assertEqual(result.returncode, 3)
        self.assertIn("error[INPUT_ERROR:3]", result.stderr)

    def test_existing_output_returns_code_4(self) -> None:
        with tempfile.TemporaryDirectory() as directory:
            source = Path(directory) / "source.eml"
            output = Path(directory) / "output.eml"
            source.write_bytes(simple_message().as_bytes())
            output.write_text("occupied")
            result = run_cli("-o", str(output), str(source))
            self.assertEqual(result.returncode, 4)
            self.assertEqual(output.read_text(), "occupied")

    def test_same_input_and_output_returns_code_4(self) -> None:
        with tempfile.TemporaryDirectory() as directory:
            source = Path(directory) / "source.eml"
            source.write_bytes(simple_message().as_bytes())
            result = run_cli("-o", str(source), str(source))
            self.assertEqual(result.returncode, 4)

    def test_hard_link_alias_returns_code_4(self) -> None:
        with tempfile.TemporaryDirectory() as directory:
            source = Path(directory) / "source.eml"
            alias = Path(directory) / "alias.eml"
            source.write_bytes(simple_message().as_bytes())
            try:
                os.link(source, alias)
            except OSError:
                _assert_mocked_alias_rejected(self, source, alias)
                return
            result = run_cli("--force", "-o", str(alias), str(source))
            self.assertEqual(result.returncode, 4)

    def test_malformed_boundary_returns_code_5(self) -> None:
        raw = (
            b"From: a@example.test\r\n"
            b"MIME-Version: 1.0\r\n"
            b'Content-Type: multipart/mixed; boundary="missing"\r\n\r\n'
            b"This body has no start boundary.\r\n"
        )
        with tempfile.TemporaryDirectory() as directory:
            source = Path(directory) / "broken.eml"
            source.write_bytes(raw)
            result = run_cli(str(source))
            self.assertEqual(result.returncode, 5)
            self.assertIn("unsafe MIME structure", result.stderr)

    def test_invalid_base64_returns_code_5(self) -> None:
        raw = (
            b"From: a@example.test\r\n"
            b"MIME-Version: 1.0\r\n"
            b'Content-Type: multipart/mixed; boundary="b"\r\n\r\n'
            b"--b\r\nContent-Type: text/plain\r\n\r\nBody\r\n"
            b'--b\r\nContent-Type: application/octet-stream; name="bad.bin"\r\n'
            b'Content-Disposition: attachment; filename="bad.bin"\r\n'
            b"Content-Transfer-Encoding: base64\r\n\r\n@@not-base64@@\r\n"
            b"--b--\r\n"
        )
        with tempfile.TemporaryDirectory() as directory:
            source = Path(directory) / "bad-base64.eml"
            source.write_bytes(raw)
            result = run_cli(str(source))
            self.assertEqual(result.returncode, 5)
            self.assertIn("unsafe MIME structure or transfer encoding", result.stderr)

    def test_missing_output_directory_returns_code_7(self) -> None:
        with tempfile.TemporaryDirectory() as directory:
            source = Path(directory) / "source.eml"
            source.write_bytes(simple_message().as_bytes())
            output = Path(directory) / "missing-dir" / "output.eml"
            result = run_cli("-o", str(output), str(source))
            self.assertEqual(result.returncode, 7)

    def test_force_replaces_existing_output(self) -> None:
        with tempfile.TemporaryDirectory() as directory:
            source = Path(directory) / "source.eml"
            output = Path(directory) / "output.eml"
            source.write_bytes(simple_message().as_bytes())
            output.write_text("occupied")
            result = run_cli("--force", "-o", str(output), str(source))
            self.assertEqual(result.returncode, 0, result.stderr)
            self.assertNotEqual(output.read_bytes(), b"occupied")

    def test_force_rejects_symlink_alias_to_source(self) -> None:
        with tempfile.TemporaryDirectory() as directory:
            source = Path(directory) / "source.eml"
            alias = Path(directory) / "alias.eml"
            source.write_bytes(simple_message().as_bytes())
            try:
                alias.symlink_to(source)
            except OSError:
                _assert_mocked_alias_rejected(self, source, alias)
                return
            result = run_cli("--force", "-o", str(alias), str(source))
            self.assertEqual(result.returncode, 4)
            self.assertTrue(alias.is_symlink())

    def test_force_replaces_output_symlink_without_changing_its_target(self) -> None:
        with tempfile.TemporaryDirectory() as directory:
            source = Path(directory) / "source.eml"
            target = Path(directory) / "unrelated.txt"
            output = Path(directory) / "output.eml"
            source.write_bytes(simple_message().as_bytes())
            target.write_text("keep me")
            try:
                output.symlink_to(target)
            except OSError:
                output.write_text("replace me")
            result = run_cli("--force", "-o", str(output), str(source))
            self.assertEqual(result.returncode, 0, result.stderr)
            self.assertEqual(target.read_text(), "keep me")
            self.assertFalse(output.is_symlink())
            self.assertEqual(parse(output)["Subject"], "Synthetic message")

    def test_bad_option_returns_usage_code_2(self) -> None:
        result = run_cli("--not-a-real-option")
        self.assertEqual(result.returncode, 2)
        self.assertIn("error[USAGE:2]", result.stderr)
