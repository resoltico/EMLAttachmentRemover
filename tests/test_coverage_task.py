"""Contracts for reporting coverage evidence after unsuccessful test runs."""

from __future__ import annotations

import subprocess
import sys
import tempfile
import unittest
from pathlib import Path
from unittest.mock import patch

from tools import coverage_task, tasks


class MeasureTests(unittest.TestCase):
    """Keep the test verdict while always publishing the evidence behind it."""

    def test_passing_tests_report_once(self) -> None:
        events: list[str] = []
        coverage_task.measure(
            lambda: events.append("tests"),
            lambda: events.append("report"),
        )
        self.assertEqual(events, ["tests", "report"])

    def test_failed_tests_still_report_and_remain_the_failure(self) -> None:
        events: list[str] = []
        failure = subprocess.CalledProcessError(1, ("pytest",))

        def tests() -> None:
            raise failure

        with self.assertRaises(subprocess.CalledProcessError) as raised:
            coverage_task.measure(tests, lambda: events.append("report"))
        self.assertIs(raised.exception, failure)
        self.assertEqual(events, ["report"])

    def test_failed_tests_and_reporting_keep_both_failures(self) -> None:
        test_failure = subprocess.CalledProcessError(1, ("pytest",))
        report_failure = subprocess.CalledProcessError(2, ("report",))

        def tests() -> None:
            raise test_failure

        def report() -> None:
            raise report_failure

        with self.assertRaises(ExceptionGroup) as raised:
            coverage_task.measure(tests, report)
        self.assertEqual(
            raised.exception.message,
            "tests and coverage reporting failed",
        )
        self.assertEqual(
            list(raised.exception.exceptions),
            [test_failure, report_failure],
        )
        self.assertIsNone(raised.exception.__cause__)
        self.assertTrue(raised.exception.__suppress_context__)

    def test_timed_out_tests_do_not_report_untrustworthy_data(self) -> None:
        events: list[str] = []

        def tests() -> None:
            raise subprocess.TimeoutExpired(("pytest",), 1)

        with self.assertRaises(subprocess.TimeoutExpired):
            coverage_task.measure(tests, lambda: events.append("report"))
        self.assertEqual(events, [])

    def test_passing_tests_propagate_a_reporting_failure(self) -> None:
        def report() -> None:
            raise subprocess.CalledProcessError(2, ("report",))

        with self.assertRaises(subprocess.CalledProcessError):
            coverage_task.measure(lambda: None, report)


class CoverageTaskFailureTests(unittest.TestCase):
    """Run the real task orchestration with a failing test command."""

    def test_failed_suite_is_still_combined_and_reported(self) -> None:
        failure = subprocess.CalledProcessError(1, ("pytest",))
        commands: list[tuple[str, ...]] = []

        def run(command: tuple[str, ...], **_options: object) -> None:
            commands.append(command)
            if "pytest" in command:
                raise failure

        def isolate(action: object, *_args: object, **_kwargs: object) -> None:
            action(Path(directory) / "storage" / ".hypothesis")  # type: ignore[operator]

        with tempfile.TemporaryDirectory() as directory:
            with (
                patch.object(tasks, "BUILD_DIRECTORY", Path(directory) / "build"),
                patch.object(tasks, "_run", side_effect=run),
                patch.object(
                    tasks.hypothesis_runner, "run_isolated", side_effect=isolate
                ),
                self.assertRaises(subprocess.CalledProcessError) as raised,
            ):
                tasks._coverage()  # ruff: ignore[private-member-access] - task contract.
        self.assertIs(raised.exception, failure)
        self.assertEqual(
            [command[:4] for command in commands],
            [
                (sys.executable, "-m", "coverage", "erase"),
                (sys.executable, "-X", "dev", "-W"),
                (sys.executable, "-m", "coverage", "combine"),
                (sys.executable, "tools/report_coverage.py", "--data-file", ANY_PATH),
            ],
        )


class _AnyPath:
    """Compare equal to any string, standing in for private storage paths."""

    def __eq__(self, other: object) -> bool:
        return isinstance(other, str)

    def __hash__(self) -> int:
        return 0


ANY_PATH = _AnyPath()


if __name__ == "__main__":
    unittest.main(verbosity=2)
