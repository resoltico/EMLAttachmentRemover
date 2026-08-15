"""Behavioral tests for the release-tag validation tool."""

from __future__ import annotations

import contextlib
import io
import tempfile
import unittest
from pathlib import Path
from unittest.mock import patch

from tools import check_release_tag


class ReleaseTagToolTests(unittest.TestCase):
    """Exercise canonical-version lookup and exact tag validation."""

    def test_project_version_and_expected_tag_use_project_metadata(self) -> None:
        with tempfile.TemporaryDirectory() as directory:
            config = Path(directory) / "pyproject.toml"
            config.write_text('[project]\nversion = "9.8.7"\n', encoding="utf-8")
            with patch.object(check_release_tag, "PROJECT_CONFIG", config):
                version = check_release_tag._project_version()  # ruff: ignore[private-member-access]
                tag = check_release_tag._expected_tag()  # ruff: ignore[private-member-access]
        self.assertEqual(version, "9.8.7")
        self.assertEqual(tag, "v9.8.7")

    def test_main_accepts_and_prints_only_the_expected_tag(self) -> None:
        with (
            patch.object(check_release_tag, "_expected_tag", return_value="v9.8.7"),
            patch("sys.stdout", new_callable=io.StringIO) as stdout,
        ):
            status = check_release_tag.main(["v9.8.7"])
        self.assertEqual(status, 0)
        self.assertEqual(stdout.getvalue(), "v9.8.7\n")

    def test_main_rejects_a_mismatched_tag_with_the_expected_value(self) -> None:
        stderr = io.StringIO()
        with (
            patch.object(check_release_tag, "_expected_tag", return_value="v9.8.7"),
            contextlib.redirect_stderr(stderr),
            self.assertRaises(SystemExit) as raised,
        ):
            check_release_tag.main(["v9.8.6"])
        self.assertEqual(raised.exception.code, 2)
        self.assertIn("tag must be v9.8.7, not v9.8.6", stderr.getvalue())

    def test_help_describes_the_exact_release_tag_boundary(self) -> None:
        stdout = io.StringIO()
        with (
            contextlib.redirect_stdout(stdout),
            self.assertRaises(SystemExit) as raised,
        ):
            check_release_tag.main(["--help"])
        self.assertEqual(raised.exception.code, 0)
        normalized = " ".join(stdout.getvalue().split())
        self.assertIn(str(check_release_tag.__doc__), normalized)
        self.assertIn("tag release tag to validate", normalized)


if __name__ == "__main__":
    unittest.main(verbosity=2)
