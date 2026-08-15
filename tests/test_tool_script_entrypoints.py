"""Exercise repository tool script entrypoints without performing release work."""

from __future__ import annotations

import runpy
import subprocess
import sys
import unittest
from contextlib import redirect_stdout
from io import StringIO
from pathlib import Path
from typing import Final
from unittest.mock import patch

PROJECT_ROOT: Final = Path(__file__).resolve().parents[1]
SUBPROCESS_TIMEOUT_SECONDS: Final = 120


def _run_tool(relative: str, *arguments: str) -> subprocess.CompletedProcess[str]:
    """Run one repository tool through its real warning-strict script boundary.

    Returns:
        Its completed process with captured text streams.

    """
    return subprocess.run(
        [sys.executable, "-X", "dev", "-W", "error", relative, *arguments],
        cwd=PROJECT_ROOT,
        text=True,
        capture_output=True,
        check=False,
        timeout=SUBPROCESS_TIMEOUT_SECONDS,
    )


class ToolScriptEntrypointTests(unittest.TestCase):
    """Verify help and missing-install command-line boundaries."""

    def test_argument_parsing_entrypoints_expose_help(self) -> None:
        for relative in (
            "tools/check_mutation_results.py",
            "tools/qualify_release.py",
            "tools/report_coverage.py",
            "tools/tasks.py",
        ):
            with self.subTest(script=relative):
                result = _run_tool(relative, "--help")
            self.assertEqual(result.returncode, 0, result.stderr)
            self.assertIn("usage:", result.stdout)
            self.assertFalse(result.stderr)

    def test_design_entrypoint_succeeds_for_an_empty_scan(self) -> None:
        path = PROJECT_ROOT / "tools" / "check_module_design.py"
        with (
            patch.object(Path, "rglob", return_value=iter(())),
            self.assertRaises(SystemExit) as raised,
        ):
            runpy.run_path(str(path), run_name="__main__")
        self.assertEqual(raised.exception.code, 0)

    def test_release_tag_entrypoint_exposes_help(self) -> None:
        path = PROJECT_ROOT / "tools" / "check_release_tag.py"
        stdout = StringIO()
        with (
            patch.object(sys, "argv", [str(path), "--help"]),
            redirect_stdout(stdout),
            self.assertRaises(SystemExit) as raised,
        ):
            runpy.run_path(str(path), run_name="__main__")
        self.assertEqual(raised.exception.code, 0)
        self.assertIn("release tag to validate", stdout.getvalue())

    def test_coverage_report_entrypoint_exposes_help(self) -> None:
        path = PROJECT_ROOT / "tools" / "report_coverage.py"
        stdout = StringIO()
        with (
            patch.object(sys, "argv", [str(path), "--help"]),
            redirect_stdout(stdout),
            self.assertRaises(SystemExit) as raised,
        ):
            runpy.run_path(str(path), run_name="__main__")
        self.assertEqual(raised.exception.code, 0)
        self.assertIn("Cobertura XML report", stdout.getvalue())

    def test_clean_hygiene_entrypoint_succeeds(self) -> None:
        result = _run_tool("tools/check_repository_hygiene.py")

        self.assertEqual(result.returncode, 0, result.stderr)
        self.assertFalse(result.stdout)
        self.assertFalse(result.stderr)

    def test_clean_hygiene_in_process_entrypoint_succeeds(self) -> None:
        path = PROJECT_ROOT / "tools" / "check_repository_hygiene.py"
        with (
            patch.object(sys, "path", [str(path.parent), *sys.path]),
            self.assertRaises(SystemExit) as raised,
        ):
            runpy.run_path(str(path), run_name="__main__")
        self.assertEqual(raised.exception.code, 0)

    def test_smoke_entrypoint_reports_a_missing_installed_command(self) -> None:
        path = PROJECT_ROOT / "tools" / "smoke_distribution.py"
        with (
            patch.object(sys, "argv", [str(path)]),
            patch("shutil.which", return_value=None),
            self.assertRaisesRegex(RuntimeError, "did not provide"),
        ):
            runpy.run_path(str(path), run_name="__main__")


if __name__ == "__main__":
    unittest.main(verbosity=2)
