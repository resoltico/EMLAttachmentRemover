"""Regression tests for mutation-task orchestration and cleanup."""

from __future__ import annotations

import os
import subprocess
import sys
import tempfile
import tomllib
import unittest
from configparser import ConfigParser
from pathlib import Path
from subprocess import CalledProcessError
from unittest.mock import patch

from tools import tasks


class MutationTaskTests(unittest.TestCase):
    """Verify mutation evidence capture, failure aggregation, and cleanup."""

    def test_mutation_aggregates_failures_after_exporting_evidence(self) -> None:
        failures = (
            CalledProcessError(1, ("mutmut", "run")),
            CalledProcessError(2, ("mutmut", "export-cicd-stats")),
            CalledProcessError(
                3,
                (sys.executable, "tools/check_mutation_results.py"),
            ),
        )
        with (
            patch.dict(
                os.environ,
                {
                    "COVERAGE_PROCESS_START": "ambient startup override",
                    "COVERAGE_UNRECOGNIZED_INPUT": "ambient future override",
                },
            ),
            patch.object(sys, "platform", "linux"),
            patch.object(tasks, "_coverage") as coverage,
            patch.object(tasks, "_remove_mutation_workspace"),
            patch.object(
                tasks,
                "_capture_mutation_results",
                side_effect=OSError("results unavailable"),
            ),
            patch.object(tasks, "_run", side_effect=failures) as run,
            self.assertRaises(ExceptionGroup) as raised,
        ):
            tasks._mutation()
        coverage.assert_called_once_with()
        self.assertEqual(
            [entry.args[0] for entry in run.call_args_list],
            [
                ("mutmut", "run"),
                ("mutmut", "export-cicd-stats"),
                (
                    sys.executable,
                    "tools/check_mutation_results.py",
                    str(tasks.MUTATION_STATISTICS),
                    "--results",
                    str(tasks.MUTATION_RESULTS),
                    "--equivalents",
                    str(tasks.MUTATION_EQUIVALENTS),
                ),
            ],
        )
        self.assertEqual(
            run.call_args_list[0].kwargs["environment_updates"]["COVERAGE_RCFILE"],
            str(tasks.PROJECT_ROOT / "tools" / "mutmut.coveragerc"),
        )
        mutation_coverage = Path(
            run.call_args_list[0].kwargs["environment_updates"]["COVERAGE_FILE"]
        )
        self.assertEqual(mutation_coverage.name, ".mutmut-coverage")
        self.assertNotIn(tasks.PROJECT_ROOT, mutation_coverage.parents)
        removals = run.call_args_list[0].kwargs["environment_removals"]
        self.assertIn("COVERAGE_PROCESS_START", removals)
        self.assertIn("COVERAGE_UNRECOGNIZED_INPUT", removals)
        self.assertEqual(len(raised.exception.exceptions), 4)
        self.assertTrue(
            all(
                isinstance(error, RuntimeError) for error in raised.exception.exceptions
            ),
        )
        messages = [str(error) for error in raised.exception.exceptions]
        self.assertTrue(any("run mutants failed" in message for message in messages))
        self.assertTrue(
            any("capture named results failed" in message for message in messages)
        )
        self.assertTrue(
            any("export statistics failed" in message for message in messages)
        )
        self.assertTrue(
            any("validate statistics failed" in message for message in messages)
        )

    def test_mutation_establishes_coverage_and_removes_stale_statistics(self) -> None:
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            mutation_root = root / "mutants"
            statistics = mutation_root / "mutmut-cicd-stats.json"
            results = Path(directory) / "mutmut-results.txt"
            tools = root / "tools"
            tools.mkdir()
            (tools / "mutmut.coveragerc").write_text(
                "[run]\nrelative_files = false\n",
                encoding="utf-8",
            )
            mutation_root.mkdir()
            statistics.write_text("stale public data", encoding="utf-8")
            results.write_text("stale public data", encoding="utf-8")
            events: list[str] = []

            def record_coverage() -> None:
                events.append("coverage")

            def record_cleanup() -> None:
                events.append("cleanup")
                tasks.mutation_task.remove_workspace(
                    tasks.mutation_task.mutation_paths(
                        root,
                        root / "build",
                        statistics,
                        results,
                        root / "equivalents.json",
                    )
                )

            def record_run(*_args: object, **_kwargs: object) -> None:
                events.append("mutation")

            with (
                patch.object(tasks, "PROJECT_ROOT", root),
                patch.object(tasks, "MUTATION_STATISTICS", statistics),
                patch.object(tasks, "MUTATION_RESULTS", results),
                patch.object(sys, "platform", "linux"),
                patch.object(tasks, "_coverage", side_effect=record_coverage),
                patch.object(
                    tasks,
                    "_remove_mutation_workspace",
                    side_effect=record_cleanup,
                ),
                patch.object(
                    tasks,
                    "_capture_mutation_results",
                    side_effect=lambda: events.append("capture"),
                ),
                patch.object(tasks, "_run", side_effect=record_run) as run,
            ):
                tasks._mutation()
        self.assertEqual(events[:2], ["cleanup", "coverage"])
        self.assertEqual(run.call_count, 3)
        self.assertFalse(statistics.exists())
        self.assertFalse(results.exists())

    def test_mutation_coverage_measurement_uses_absolute_paths(self) -> None:
        configuration = ConfigParser()
        self.assertEqual(
            configuration.read(
                tasks.PROJECT_ROOT / "tools" / "mutmut.coveragerc",
                encoding="utf-8",
            ),
            [str(tasks.PROJECT_ROOT / "tools" / "mutmut.coveragerc")],
        )
        self.assertFalse(configuration.getboolean("run", "relative_files"))

    def test_mutmut_pytest_disables_repository_cacheprovider(self) -> None:
        """Prevent Mutmut's internal pytest runs from creating copied caches."""
        configuration = tomllib.loads(
            (tasks.PROJECT_ROOT / "pyproject.toml").read_text(encoding="utf-8")
        )
        self.assertEqual(
            configuration["tool"]["mutmut"]["pytest_add_cli_args"],
            ["-q", "-p", "no:cacheprovider"],
        )

    def test_mutation_rejects_missing_or_symlinked_coverage_configuration(self) -> None:
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            coverage_events: list[str] = []
            paths = tasks.mutation_task.mutation_paths(
                root,
                root / "build",
                root / "mutants" / "stats.json",
                root / "build" / "results.txt",
                root / "equivalents.json",
            )
            actions = tasks.mutation_task.mutation_actions(
                lambda: coverage_events.append("coverage"),
                lambda: None,
                lambda: None,
                lambda *_args, **_kwargs: None,
            )
            with self.assertRaisesRegex(RuntimeError, "configuration is unsafe"):
                tasks.mutation_task.run_mutation(paths, actions, sys.executable)
            self.assertEqual(coverage_events, [])
            target = root / "target.coveragerc"
            target.write_text("[run]\nrelative_files = false\n", encoding="utf-8")
            tools = root / "tools"
            tools.mkdir()
            link = tools / "mutmut.coveragerc"
            link.symlink_to(target)
            with self.assertRaisesRegex(RuntimeError, "configuration is unsafe"):
                tasks.mutation_task.run_mutation(paths, actions, sys.executable)
            self.assertEqual(coverage_events, [])

    def test_mutation_workspace_cleanup_rejects_noncanonical_target(self) -> None:
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            outside = root / "outside" / "stats.json"
            with (
                patch.object(tasks, "PROJECT_ROOT", root),
                patch.object(tasks, "MUTATION_STATISTICS", outside),
                self.assertRaisesRegex(RuntimeError, "noncanonical"),
            ):
                tasks._remove_mutation_workspace()

    def test_mutation_workspace_cleanup_rejects_unsafe_objects(self) -> None:
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            workspace = root / "mutants"
            statistics = workspace / "mutmut-cicd-stats.json"
            workspace.write_text("not a directory", encoding="utf-8")
            with (
                patch.object(tasks, "PROJECT_ROOT", root),
                patch.object(tasks, "MUTATION_STATISTICS", statistics),
                self.assertRaisesRegex(RuntimeError, "unsafe mutation workspace"),
            ):
                tasks._remove_mutation_workspace()

    def test_mutation_workspace_cleanup_reports_operational_and_incomplete_removal(
        self,
    ) -> None:
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            workspace = root / "mutants"
            workspace.mkdir()
            statistics = workspace / "mutmut-cicd-stats.json"
            with (
                patch.object(tasks, "PROJECT_ROOT", root),
                patch.object(tasks, "MUTATION_STATISTICS", statistics),
                patch(
                    "tools.mutation_task.shutil.rmtree",
                    side_effect=OSError("denied"),
                ),
                self.assertRaisesRegex(OSError, "denied"),
            ):
                tasks._remove_mutation_workspace()
            with (
                patch.object(tasks, "PROJECT_ROOT", root),
                patch.object(tasks, "MUTATION_STATISTICS", statistics),
                patch("tools.mutation_task.shutil.rmtree"),
                self.assertRaisesRegex(RuntimeError, "removal was incomplete"),
            ):
                tasks._remove_mutation_workspace()

    def test_mutation_workspace_cleanup_accepts_an_absent_workspace(self) -> None:
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            statistics = root / "mutants" / "mutmut-cicd-stats.json"
            with (
                patch.object(tasks, "PROJECT_ROOT", root),
                patch.object(tasks, "MUTATION_STATISTICS", statistics),
            ):
                tasks._remove_mutation_workspace()
            self.assertFalse(statistics.parent.exists())

    def test_mutation_rejects_native_windows_before_running_coverage(self) -> None:
        with (
            patch.object(sys, "platform", "win32"),
            patch.object(tasks, "_coverage") as coverage,
            patch.object(tasks.hypothesis_runner, "run_isolated") as isolated,
            self.assertRaisesRegex(RuntimeError, "WSL or on Linux/macOS"),
        ):
            tasks._mutation()
        coverage.assert_not_called()
        isolated.assert_not_called()

    def test_named_mutation_results_are_captured_sorted_and_bounded(self) -> None:
        completed = subprocess.CompletedProcess(
            ("public",),
            0,
            stdout="  public.z: survived\n\npublic.a: killed\n",
        )
        environment = {"PUBLIC": "value"}
        with tempfile.TemporaryDirectory() as directory:
            output = Path(directory) / "mutmut-results.txt"
            with (
                patch.object(tasks, "MUTATION_RESULTS", output),
                patch.object(
                    tasks,
                    "_task_environment",
                    return_value=environment,
                ),
                patch(
                    "tools.mutation_task.subprocess.run",
                    return_value=completed,
                ) as run,
            ):
                tasks._capture_mutation_results()
            self.assertEqual(
                output.read_text(encoding="utf-8"),
                "public.a: killed\npublic.z: survived\n",
            )
        run.assert_called_once_with(
            (sys.executable, "-m", "mutmut", "results", "--all", "true"),
            check=True,
            cwd=tasks.PROJECT_ROOT,
            env=environment,
            timeout=120,
            capture_output=True,
            encoding="utf-8",
        )


if __name__ == "__main__":
    unittest.main(verbosity=2)
