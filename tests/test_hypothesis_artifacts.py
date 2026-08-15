"""Regression tests for public Hypothesis artifact finalization."""

from __future__ import annotations

import io
import json
import os
import shutil
import subprocess
import sys
import tempfile
import unittest
from pathlib import Path
from unittest.mock import patch

from tools import finalize_hypothesis_artifacts as artifacts

from tests.hypothesis_artifact_support import write_observations


class HypothesisArtifactOperationalTests(unittest.TestCase):
    """Verify filesystem failures, commands, and guaranteed finalization."""

    def test_operational_failures_are_contextual_and_never_published(self) -> None:
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory).resolve()
            constants = root / ".hypothesis" / "constants"
            constants.mkdir(parents=True)
            with (
                patch.object(shutil, "rmtree", side_effect=OSError("denied")),
                self.assertRaisesRegex(
                    artifacts.HypothesisArtifactError,
                    "cannot remove Hypothesis constants",
                ),
            ):
                artifacts.finalize(root)
            with (
                patch.object(shutil, "rmtree"),
                self.assertRaisesRegex(
                    artifacts.HypothesisArtifactError,
                    "removal was incomplete",
                ),
            ):
                artifacts.finalize(root)
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory).resolve()
            path = write_observations(root, [{"status": "passed"}])
            with (
                patch.object(
                    tempfile,
                    "NamedTemporaryFile",
                    side_effect=OSError("denied"),
                ),
                self.assertRaisesRegex(
                    artifacts.HypothesisArtifactError,
                    "cannot publish sanitized Hypothesis observations",
                ),
            ):
                artifacts.finalize(root, observations=True)
            self.assertEqual(
                json.loads(path.read_text(encoding="utf-8"))["status"], "passed"
            )

    def test_inspection_and_listing_failures_are_contextual(self) -> None:
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory).resolve()
            constants = root / ".hypothesis" / "constants"
            constants.mkdir(parents=True)
            original_lstat = Path.lstat

            def fail_constants(path: Path) -> os.stat_result:
                if path == constants:
                    message = "inspection denied"
                    raise OSError(message)
                return original_lstat(path)

            with (
                patch.object(Path, "lstat", fail_constants),
                self.assertRaisesRegex(
                    artifacts.HypothesisArtifactError,
                    "cannot inspect Hypothesis constants",
                ),
            ):
                artifacts.finalize(root)
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory).resolve()
            observation = write_observations(root, [{"status": "passed"}])
            observed = observation.parent
            original_lstat = Path.lstat

            def fail_observation(path: Path) -> os.stat_result:
                if path == observation:
                    message = "inspection denied"
                    raise OSError(message)
                return original_lstat(path)

            with (
                patch.object(Path, "lstat", fail_observation),
                self.assertRaisesRegex(
                    artifacts.HypothesisArtifactError,
                    "cannot inspect Hypothesis observation file",
                ),
            ):
                artifacts.finalize(root, observations=True)
            with (
                patch.object(Path, "iterdir", side_effect=OSError("listing denied")),
                self.assertRaisesRegex(
                    artifacts.HypothesisArtifactError,
                    "cannot list Hypothesis observations",
                ),
            ):
                artifacts.finalize(root, observations=True)
            self.assertTrue(observed.is_dir())

    def test_prefix_filter_and_serialized_content_backstop_are_enforced(self) -> None:
        replacements = artifacts._replacement_prefixes(  # ruff: ignore[private-member-access]
            Path(os.sep)
        )
        self.assertTrue(all(prefix != os.sep for prefix, _replacement in replacements))
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory).resolve()
            path = write_observations(root, [{"value": "PRIVATE_TOKEN"}])
            original = path.read_text(encoding="utf-8")
            with (
                patch.object(
                    artifacts,
                    "_replacement_prefixes",
                    return_value=(("PRIVATE_TOKEN", "<private>"),),
                ),
                patch.object(
                    artifacts,
                    "_public_value",
                    return_value={"value": "PRIVATE_TOKEN"},
                ),
                self.assertRaisesRegex(
                    artifacts.HypothesisArtifactError,
                    "private path remains",
                ),
            ):
                artifacts.finalize(root, observations=True)
            self.assertEqual(path.read_text(encoding="utf-8"), original)

    def test_main_reports_success_and_safe_failure(self) -> None:
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory).resolve()
            write_observations(root, [{"coverage": {str(root): [1]}}])
            stdout = io.StringIO()
            with (
                patch.object(artifacts, "PROJECT_ROOT", root),
                patch("sys.stdout", stdout),
            ):
                self.assertEqual(artifacts.main(["--observations"]), 0)
            self.assertIn("files=1, records=1", stdout.getvalue())
            constants = root / ".hypothesis" / "constants"
            constants.parent.mkdir(exist_ok=True)
            constants.write_text("unsafe", encoding="utf-8")
            stderr = io.StringIO()
            with (
                patch.object(artifacts, "PROJECT_ROOT", root),
                patch("sys.stderr", stderr),
            ):
                self.assertEqual(artifacts.main([]), 1)
            self.assertIn("must be a real directory", stderr.getvalue())

    def test_test_and_finalization_failures_are_both_preserved(self) -> None:
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory).resolve()
            constants = root / ".hypothesis" / "constants"
            constants.parent.mkdir(parents=True)
            constants.write_text("unsafe", encoding="utf-8")

            def fail_test() -> None:
                raise subprocess.TimeoutExpired(("pytest",), 1)

            with self.assertRaises(BaseExceptionGroup) as raised:
                artifacts.run_and_finalize(fail_test, root)
            self.assertEqual(len(raised.exception.exceptions), 2)
            self.assertIsInstance(
                raised.exception.exceptions[0], subprocess.TimeoutExpired
            )
            self.assertIsInstance(
                raised.exception.exceptions[1], artifacts.HypothesisArtifactError
            )

    def test_test_failure_is_reraised_after_successful_finalization(self) -> None:
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory).resolve()
            constants = root / ".hypothesis" / "constants"
            constants.mkdir(parents=True)

            def fail_test() -> None:
                raise subprocess.CalledProcessError(3, ("pytest",))

            with self.assertRaises(subprocess.CalledProcessError):
                artifacts.run_and_finalize(fail_test, root)
            self.assertFalse(constants.exists())

    def test_unexpected_action_failure_still_finalizes_then_is_reraised(self) -> None:
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory).resolve()
            constants = root / ".hypothesis" / "constants"
            constants.mkdir(parents=True)

            def fail_test() -> None:
                message = "unexpected test failure"
                raise RuntimeError(message)

            with self.assertRaisesRegex(RuntimeError, "unexpected test failure"):
                artifacts.run_and_finalize(fail_test, root)
            self.assertFalse(constants.exists())

    def test_successful_action_is_finalized(self) -> None:
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory).resolve()
            constants = root / ".hypothesis" / "constants"
            constants.mkdir(parents=True)
            actions: list[str] = []
            artifacts.run_and_finalize(lambda: actions.append("passed"), root)
            self.assertEqual(actions, ["passed"])
            self.assertFalse(constants.exists())

    def test_control_flow_exceptions_finalize_and_preserve_both_failures(
        self,
    ) -> None:
        for failure in (KeyboardInterrupt(), SystemExit(19)):
            with (
                self.subTest(exception=type(failure).__name__, finalization="passes"),
                tempfile.TemporaryDirectory() as directory,
            ):
                root = Path(directory).resolve()
                constants = root / ".hypothesis" / "constants"
                constants.mkdir(parents=True)

                def interrupt(error: BaseException = failure) -> None:
                    raise error

                with self.assertRaises(type(failure)) as original_raised:
                    artifacts.run_and_finalize(interrupt, root)
                self.assertIs(original_raised.exception, failure)
                self.assertFalse(constants.exists())
            with (
                self.subTest(exception=type(failure).__name__, finalization="fails"),
                tempfile.TemporaryDirectory() as directory,
            ):
                root = Path(directory).resolve()
                constants = root / ".hypothesis" / "constants"
                constants.parent.mkdir(parents=True)
                constants.write_text("unsafe", encoding="utf-8")
                with self.assertRaises(BaseExceptionGroup) as group_raised:
                    artifacts.run_and_finalize(interrupt, root)
                self.assertIs(group_raised.exception.exceptions[0], failure)
                self.assertIsInstance(
                    group_raised.exception.exceptions[1],
                    artifacts.HypothesisArtifactError,
                )

    def test_command_line_entrypoint_finalizes_repository_artifacts(self) -> None:
        result = subprocess.run(
            [
                sys.executable,
                "-X",
                "dev",
                "-W",
                "error",
                "tools/finalize_hypothesis_artifacts.py",
                "--observations",
            ],
            cwd=artifacts.PROJECT_ROOT,
            text=True,
            capture_output=True,
            check=False,
            timeout=30,
        )
        self.assertEqual(result.returncode, 0, result.stderr)
        self.assertIn("Hypothesis artifacts finalized", result.stdout)


if __name__ == "__main__":
    unittest.main(verbosity=2)
