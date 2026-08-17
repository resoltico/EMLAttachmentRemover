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

from tests import macos_integration_support
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


@unittest.skipUnless(POSIX_AVAILABLE, POSIX_REASON)
class FinderReportSafetyTests(unittest.TestCase):
    """Treat processor diagnostics and JSON values as untrusted terminal text."""

    def test_v2_report_is_sanitized_summarized_and_status_preserved(self) -> None:
        with tempfile.TemporaryDirectory() as directory:
            base = Path(directory)
            processor = base / "public-processor.pyz"
            error: dict[str, object] = {
                "error": {
                    "code": 3,
                    "message": "bad\x1b[2Jž",
                    "name": "INPUT_ERROR",
                },
                "source": "public\nsource.eml",
                "status": "error",
            }
            success = macos_integration_support.valid_finder_result(
                "public-ž\x1b[32m\noutput.text-only.eml"
            )
            success["warnings"] = ["transport warning\x1b[33m"]
            report = macos_integration_support.valid_finder_report(
                results=[success],
                errors=[error],
            )
            macos_integration_support.write_report_processor(
                processor,
                report,
                status=9,
                diagnostics="diagnostic ž\x1b[31m\rline",
            )
            home = base / "public-home"
            home.mkdir()

            result = _run(_environment(home, processor), "public-source.eml")

            self.assertEqual(result.returncode, 9, result.stderr)
            self.assertNotIn("\x1b", result.stdout + result.stderr)
            self.assertNotIn("\r", result.stdout + result.stderr)
            self.assertIn("Created 1 verified text-only EML file:", result.stdout)
            self.assertIn(
                "public-ž\\x1b[32m\\x0aoutput.text-only.eml",
                result.stdout,
            )
            self.assertIn("Discarded body resources: 1", result.stdout)
            self.assertIn("Removed ordinary attachments: 1", result.stdout)
            self.assertIn("diagnostic ž\\x1b[31m\\x0dline\n", result.stderr)
            self.assertIn("1 processing warning:", result.stderr)
            self.assertIn(
                "public-source.eml: transport warning\\x1b[33m",
                result.stderr,
            )
            self.assertIn("public\\x0asource.eml", result.stderr)
            self.assertIn("bad\\x1b[2Jž", result.stderr)

    def test_created_and_skipped_outputs_are_reported_separately(self) -> None:
        with tempfile.TemporaryDirectory() as directory:
            base = Path(directory)
            processor = base / "public-processor.pyz"
            skipped: dict[str, object] = {
                "destination": "existing.text-only.eml",
                "source": "existing.eml",
                "status": "skipped",
            }
            macos_integration_support.write_report_processor(
                processor,
                macos_integration_support.valid_finder_report(
                    results=[macos_integration_support.valid_finder_result()],
                    skipped=[skipped],
                ),
            )
            home = base / "public-home"
            home.mkdir()

            result = _run(_environment(home, processor), "public-source.eml")

            self.assertEqual(result.returncode, 0, result.stderr)
            self.assertIn("Created 1 verified text-only EML file:", result.stdout)
            self.assertIn(
                "Skipped 1 existing output without changing or verifying it:",
                result.stdout,
            )
            self.assertNotIn("created or retained", result.stdout.casefold())

    def test_invalid_or_empty_reports_become_internal_errors(self) -> None:
        sources = {
            "invalid": "print('{not-json}')\n",
            "empty": "import sys\nprint('bad\\x1b[2Jž', file=sys.stderr)\n",
        }
        for name, source in sources.items():
            with self.subTest(name=name):
                with tempfile.TemporaryDirectory() as directory:
                    base = Path(directory)
                    processor = base / "public-invalid.pyz"
                    processor.write_text(source, encoding="utf-8")
                    home = base / "public-home"
                    home.mkdir()

                    result = _run(
                        _environment(home, processor),
                        "public-source.eml",
                    )

                    self.assertEqual(result.returncode, 70)
                    self.assertNotIn("\x1b", result.stderr)
                    self.assertNotIn("Traceback", result.stderr)
                    self.assertTrue(
                        "report was invalid" in result.stderr
                        or "produced no report" in result.stderr
                    )

    def test_report_and_processor_status_must_agree(self) -> None:
        failure: dict[str, object] = {
            "error": {"code": 3, "message": "bad", "name": "INPUT_ERROR"},
            "source": "public.eml",
            "status": "error",
        }
        cases = (
            (macos_integration_support.valid_finder_report(errors=[failure]), 0),
            (macos_integration_support.valid_finder_report(), 5),
        )
        for payload, processor_status in cases:
            with self.subTest(processor_status=processor_status):
                with tempfile.TemporaryDirectory() as directory:
                    base = Path(directory)
                    processor = base / "public-status-mismatch.pyz"
                    macos_integration_support.write_report_processor(
                        processor,
                        payload,
                        status=processor_status,
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

    def test_schema_scope_and_every_nested_shape_are_validated_before_display(
        self,
    ) -> None:
        base = macos_integration_support.valid_finder_report(
            results=[macos_integration_support.valid_finder_result()]
        )
        cases: dict[str, object] = {
            "outer-list": [],
            "v1-schema": {**base, "schema_version": 1},
            "wrong-scope": {**base, "scope": "attachment-preserving"},
            "wrong-program": {**base, "program": "public-decoy"},
            "results-mapping": {**base, "results": {}},
            "wrong-suffix": macos_integration_support.replace_result_field(
                base,
                "destination",
                "public-output.eml",
            ),
            "no-selected-body": macos_integration_support.replace_result_field(
                base,
                "selected_plain_text_bodies",
                [],
            ),
            "two-selected-bodies": macos_integration_support.replace_result_field(
                base,
                "selected_plain_text_bodies",
                [
                    {
                        "content_type": "text/plain",
                        "mime_path": "root",
                    },
                    {
                        "content_type": "text/plain",
                        "mime_path": "2",
                    },
                ],
            ),
            "zero-mime-path": macos_integration_support.replace_record_field(
                base,
                "selected_plain_text_bodies",
                "mime_path",
                "0.1",
            ),
            "empty-content-type": macos_integration_support.replace_record_field(
                base,
                "selected_plain_text_bodies",
                "content_type",
                "",
            ),
            "selected-html": macos_integration_support.replace_record_field(
                base,
                "selected_plain_text_bodies",
                "content_type",
                "text/html",
            ),
            "invalid-reference-path": macos_integration_support.replace_record_field(
                base,
                "discarded_body_resources",
                "referenced_by",
                ["2.0"],
            ),
        }
        cases["bad-destination"] = macos_integration_support.replace_result_field(
            base, "destination", 7
        )
        cases["bad-resource"] = macos_integration_support.replace_result_field(
            base,
            "discarded_body_resources",
            [{"content_type": "image/jpeg", "mime_path": "2.2"}],
        )
        extra_result_key = macos_integration_support.replace_result_field(
            base, "preserved", []
        )
        cases["v1-result-key"] = extra_result_key
        for name, payload in cases.items():
            with self.subTest(name=name):
                with tempfile.TemporaryDirectory() as directory:
                    path = Path(directory)
                    processor = path / "public-malformed.pyz"
                    macos_integration_support.write_report_processor(
                        processor,
                        payload,
                    )
                    home = path / "public-home"
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
            report = json.dumps(macos_integration_support.valid_finder_report())
            processor.write_text(
                "import json, os, sys\n"
                "argument_log = open("
                "os.environ['PUBLIC_ARGUMENT_LOG'], 'w', encoding='utf-8'"
                ")\n"
                "argument_log.write(json.dumps(sys.argv[1:]))\n"
                "argument_log.close()\n"
                f"print({report!r})\n",
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
                macos_integration_support.temporary_mode_processor_source(),
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
