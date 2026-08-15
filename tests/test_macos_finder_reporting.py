"""Adversarial tests for macOS Finder launcher report handling."""

from __future__ import annotations

import json
import os
import subprocess
import sys
import tempfile
import unittest
from pathlib import Path
from typing import TYPE_CHECKING, Final

from tests.test_support import SUBPROCESS_TIMEOUT_SECONDS, subprocess_environment

if TYPE_CHECKING:
    from collections.abc import Mapping

PROJECT_ROOT: Final = Path(__file__).resolve().parents[1]
RUNNER: Final = PROJECT_ROOT / "integrations/macos-shortcuts/run-from-finder.sh"
POSIX_SHELL: Final = Path("/bin/sh")
POSIX_AVAILABLE: Final = os.name == "posix" and POSIX_SHELL.is_file()
POSIX_REASON: Final = "requires the mandatory POSIX integration environment"


def _run(
    environment: Mapping[str, str],
    *arguments: str,
) -> subprocess.CompletedProcess[str]:
    """Run the Finder launcher in a bounded public test environment.

    Returns:
        The completed shell process.

    """
    return subprocess.run(
        [str(POSIX_SHELL), str(RUNNER), *arguments],
        env=environment,
        text=True,
        capture_output=True,
        check=False,
        timeout=SUBPROCESS_TIMEOUT_SECONDS,
    )


def _environment(home: Path, processor: Path, **updates: str) -> dict[str, str]:
    """Return an isolated launcher environment.

    Returns:
        A subprocess environment selecting the synthetic processor.

    """
    environment = subprocess_environment()
    environment.update({
        "EML_REMOVER_HOME": str(home / "public-installation"),
        "EML_REMOVER_PYTHON": sys.executable,
        "EML_REMOVER_REVEAL": "0",
        "EML_REMOVER_ZIPAPP": str(processor),
        "HOME": str(home),
    })
    environment.update(updates)
    return environment


def _temporary_mode_processor_source() -> str:
    """Return a processor that records launcher temporary-file modes.

    Returns:
        Synthetic Python source for the public test processor.

    """
    return (
        "import json, os, pathlib, stat\n"
        "entries = sorted(pathlib.Path(os.environ['TMPDIR']).iterdir())\n"
        "modes = [stat.S_IMODE(entry.stat().st_mode) for entry in entries]\n"
        "pathlib.Path(os.environ['PUBLIC_MODE_LOG']).write_text(json.dumps(modes))\n"
        "print(json.dumps({'ok': True, 'program': 'p', 'version': 'v', "
        "'results': [], 'skipped': [], 'errors': []}))\n"
    )


@unittest.skipUnless(POSIX_AVAILABLE, POSIX_REASON)
class FinderReportSafetyTests(unittest.TestCase):
    """Treat processor diagnostics and JSON values as untrusted terminal text."""

    def test_report_and_diagnostics_escape_controls_but_preserve_unicode(self) -> None:
        with tempfile.TemporaryDirectory() as directory:
            base = Path(directory)
            processor = base / "public-processor.pyz"
            processor.write_text(
                "import json, sys\n"
                "print('diagnostic ž\\x1b[31m\\rline', file=sys.stderr)\n"
                "print(json.dumps({\n"
                " 'ok': False, 'program': 'remove-eml-attachments', "
                "'version': '9.8.7',\n"
                " 'results': [{'source': 'public-source.eml', 'status': 'ok', "
                "'destination': 'public-ž\\x1b[32m\\noutput.eml'}],\n"
                " 'skipped': [],\n"
                " 'errors': [{'source': 'public\\nsource.eml', 'status': 'error', "
                "'error': "
                "{'name': 'INPUT', 'code': 3, 'message': 'bad\\x1b[2Jž'}}],\n"
                "}))\n"
                "sys.exit(9)\n",
                encoding="utf-8",
            )
            home = base / "public-home"
            home.mkdir()

            result = _run(_environment(home, processor), "public-source.eml")

            self.assertEqual(result.returncode, 9, result.stderr)
            self.assertNotIn("\x1b", result.stdout + result.stderr)
            self.assertNotIn("\r", result.stdout + result.stderr)
            self.assertIn("public-ž\\x1b[32m\\x0aoutput.eml", result.stdout)
            self.assertIn("diagnostic ž\\x1b[31m\\x0dline\n", result.stderr)
            self.assertIn("public\\x0asource.eml", result.stderr)
            self.assertIn("bad\\x1b[2Jž", result.stderr)

    def test_invalid_report_is_generic_and_changes_success_to_internal_error(
        self,
    ) -> None:
        with tempfile.TemporaryDirectory() as directory:
            base = Path(directory)
            processor = base / "public-invalid.pyz"
            processor.write_text("print('{not-json}')\n", encoding="utf-8")
            home = base / "public-home"
            home.mkdir()

            result = _run(_environment(home, processor), "public-source.eml")

            self.assertEqual(result.returncode, 70)
            self.assertIn("report was invalid", result.stderr)
            self.assertNotIn("Traceback", result.stderr)

    def test_invalid_report_diagnostics_are_sanitized_without_raw_controls(
        self,
    ) -> None:
        with tempfile.TemporaryDirectory() as directory:
            base = Path(directory)
            processor = base / "public-invalid-diagnostic.pyz"
            processor.write_text(
                "import sys\n"
                "print('public ž\\x1b[2J', file=sys.stderr)\n"
                "print('{invalid')\n",
                encoding="utf-8",
            )
            home = base / "public-home"
            home.mkdir()

            result = _run(_environment(home, processor), "public-source.eml")

            self.assertEqual(result.returncode, 70)
            self.assertNotIn("\x1b", result.stderr)
            self.assertIn("public ž\\x1b[2J\n", result.stderr)
            self.assertIn("report was invalid", result.stderr)

    def test_empty_report_preserves_processor_failure_status_and_sanitizes_stderr(
        self,
    ) -> None:
        with tempfile.TemporaryDirectory() as directory:
            base = Path(directory)
            processor = base / "public-empty.pyz"
            processor.write_text(
                "import sys\nprint('bad\\x1b[2Jž', file=sys.stderr)\nsys.exit(5)\n",
                encoding="utf-8",
            )
            home = base / "public-home"
            home.mkdir()

            result = _run(_environment(home, processor), "public-source.eml")

            self.assertEqual(result.returncode, 70)
            self.assertNotIn("\x1b", result.stderr)
            self.assertIn("bad\\x1b[2Jž\n", result.stderr)
            self.assertIn("produced no report", result.stderr)

    def test_report_and_processor_status_must_agree(self) -> None:
        cases = {
            "errors-with-success": (
                {
                    "errors": [
                        {
                            "error": {
                                "code": 3,
                                "message": "bad",
                                "name": "INPUT",
                            },
                            "source": "public.eml",
                            "status": "error",
                        }
                    ],
                    "ok": False,
                    "program": "p",
                    "results": [],
                    "skipped": [],
                    "version": "v",
                },
                0,
            ),
            "success-with-failure": (
                {
                    "errors": [],
                    "ok": True,
                    "program": "p",
                    "results": [],
                    "skipped": [],
                    "version": "v",
                },
                5,
            ),
        }
        for name, (payload, processor_status) in cases.items():
            with self.subTest(name=name):
                with tempfile.TemporaryDirectory() as directory:
                    base = Path(directory)
                    processor = base / "public-status-mismatch.pyz"
                    serialized = json.dumps(payload)
                    processor.write_text(
                        "import sys\n"
                        f"print({serialized!r})\n"
                        f"sys.exit({processor_status})\n",
                        encoding="utf-8",
                    )
                    home = base / "public-home"
                    home.mkdir()

                    result = _run(
                        _environment(home, processor),
                        "public-source.eml",
                    )

                    self.assertEqual(result.returncode, 70)
                    self.assertEqual(result.stdout, "")
                    self.assertIn("report was invalid", result.stderr)

    def test_malformed_item_schema_fails_before_any_result_is_displayed(self) -> None:
        with tempfile.TemporaryDirectory() as directory:
            base = Path(directory)
            processor = base / "public-malformed.pyz"
            processor.write_text(
                "import json\nprint(json.dumps({'ok': True, 'program': 'p', "
                "'version': 'v', 'results': [{'destination': 'unsafe'}], "
                "'skipped': [], 'errors': []}))\n",
                encoding="utf-8",
            )
            home = base / "public-home"
            home.mkdir()

            result = _run(_environment(home, processor), "public-source.eml")

            self.assertEqual(result.returncode, 70)
            self.assertEqual(result.stdout, "")
            self.assertIn("report was invalid", result.stderr)

    def test_all_malformed_report_shapes_fail_before_display(self) -> None:
        valid_error = {
            "error": {"code": 3, "message": "bad", "name": "INPUT"},
            "source": "public.eml",
            "status": "error",
        }
        cases: dict[str, object] = {
            "outer-list": [],
            "results-mapping": {
                "errors": [],
                "ok": True,
                "program": "p",
                "results": {},
                "skipped": [],
                "version": "v",
            },
            "bad-destination": {
                "errors": [],
                "ok": True,
                "program": "p",
                "results": [{"destination": 7, "source": "public.eml", "status": "ok"}],
                "skipped": [],
                "version": "v",
            },
            "unencodable-destination": {
                "errors": [],
                "ok": True,
                "program": "p",
                "results": [
                    {
                        "destination": "public-\ud800.eml",
                        "source": "public.eml",
                        "status": "ok",
                    }
                ],
                "skipped": [],
                "version": "v",
            },
            "bad-error": {
                "errors": [{**valid_error, "error": "invalid"}],
                "ok": False,
                "program": "p",
                "results": [],
                "skipped": [],
                "version": "v",
            },
            "bool-error-code": {
                "errors": [
                    {
                        **valid_error,
                        "error": {"code": True, "message": "bad", "name": "INPUT"},
                    }
                ],
                "ok": False,
                "program": "p",
                "results": [],
                "skipped": [],
                "version": "v",
            },
            "inconsistent-ok": {
                "errors": [valid_error],
                "ok": True,
                "program": "p",
                "results": [],
                "skipped": [],
                "version": "v",
            },
        }
        for name, payload in cases.items():
            with self.subTest(name=name):
                with tempfile.TemporaryDirectory() as directory:
                    base = Path(directory)
                    processor = base / "public-malformed.pyz"
                    serialized = json.dumps(payload)
                    processor.write_text(
                        f"print({serialized!r})\n",
                        encoding="utf-8",
                    )
                    home = base / "public-home"
                    home.mkdir()

                    result = _run(
                        _environment(home, processor),
                        "public-source.eml",
                    )

                    self.assertEqual(result.returncode, 70)
                    self.assertEqual(result.stdout, "")
                    self.assertIn("report was invalid", result.stderr)

    def test_existing_output_modes_are_forwarded_as_exact_arguments(self) -> None:
        with tempfile.TemporaryDirectory() as directory:
            base = Path(directory)
            processor = base / "public-arguments.pyz"
            argument_log = base / "public-arguments.json"
            processor.write_text(
                "import json, os, sys\n"
                "argument_log = open("
                "os.environ['PUBLIC_ARGUMENT_LOG'], 'w', encoding='utf-8'"
                ")\n"
                "argument_log.write(json.dumps(sys.argv[1:]))\n"
                "argument_log.close()\n"
                "print(json.dumps({'ok': True, "
                "'program': 'remove-eml-attachments', 'version': '9.8.7', "
                "'results': [], 'skipped': [], 'errors': []}))\n",
                encoding="utf-8",
            )
            home = base / "public-home"
            home.mkdir()
            environment = _environment(
                home,
                processor,
                PUBLIC_ARGUMENT_LOG=str(argument_log),
            )
            expected_options = {
                "error": ["--output-format", "json", "--"],
                "skip": ["--skip-existing", "--output-format", "json", "--"],
                "force": ["--force", "--output-format", "json", "--"],
            }

            for mode, options in expected_options.items():
                with self.subTest(mode=mode):
                    environment["EML_REMOVER_EXISTING"] = mode
                    result = _run(environment, "public-source.eml")
                    self.assertEqual(result.returncode, 0, result.stderr)
                    self.assertEqual(
                        json.loads(argument_log.read_text(encoding="utf-8")),
                        [*options, "public-source.eml"],
                    )

    def test_temporary_report_files_are_owner_only(self) -> None:
        with tempfile.TemporaryDirectory() as directory:
            base = Path(directory)
            processor = base / "public-mode.pyz"
            processor.write_text(
                _temporary_mode_processor_source(),
                encoding="utf-8",
            )
            home = base / "public-home"
            home.mkdir()
            temporary = base / "public-temporary"
            temporary.mkdir()
            mode_log = base / "public-modes.json"
            environment = _environment(
                home,
                processor,
                PUBLIC_MODE_LOG=str(mode_log),
                TMPDIR=str(temporary),
            )

            result = _run(environment, "public-source.eml")

            self.assertEqual(result.returncode, 0, result.stderr)
            self.assertEqual(
                json.loads(mode_log.read_text(encoding="utf-8")),
                [0o600, 0o600],
            )
