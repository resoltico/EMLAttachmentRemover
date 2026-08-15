"""Exact in-process contracts for the portable task runner."""

from __future__ import annotations

import sys
import tempfile
import unittest
from pathlib import Path
from typing import cast
from unittest.mock import ANY, call, patch

from tools import task_test_commands, tasks


class TaskCommandContractTests(unittest.TestCase):
    """Require every task boundary to preserve its exact public command contract."""

    def test_pytest_command_rejects_a_non_boolean_assurance_mode(self) -> None:
        """Do not silently turn an invalid coverage mode into a plain test run."""
        with self.assertRaises(TypeError) as raised:
            task_test_commands.pytest_command(
                sys.executable,
                Path("/public/test-results.xml"),
                coverage=cast("bool", None),
            )
        self.assertEqual(str(raised.exception), "coverage mode must be a boolean")

    def test_task_environment_is_exact_and_removals_are_absence_safe(self) -> None:
        ambient = {
            "HYPOTHESIS_STORAGE_DIRECTORY": "ambient-storage",
            "KEEP": "ambient",
            "PYTHONPATH": "ambient-imports",
            "REMOVE_PRESENT": "ambient",
        }
        with patch(
            "tools.tasks.os.environ.copy",
            side_effect=ambient.copy,
        ):
            environment = tasks._task_environment(
                profile="project-public",
                environment_updates={"KEEP": "updated", "NEW": "public"},
                environment_removals=("REMOVE_PRESENT", "REMOVE_ABSENT"),
            )
            default_environment = tasks._task_environment()

        self.assertEqual(
            environment,
            {
                "HYPOTHESIS_PROFILE": "project-public",
                "KEEP": "updated",
                "NEW": "public",
                "PYTHONDEVMODE": "1",
                "PYTHONDONTWRITEBYTECODE": "1",
                "PYTHONNOUSERSITE": "1",
                "PYTHONWARNINGS": "error",
            },
        )
        self.assertEqual(
            default_environment,
            {
                "KEEP": "ambient",
                "PYTHONDEVMODE": "1",
                "PYTHONDONTWRITEBYTECODE": "1",
                "PYTHONNOUSERSITE": "1",
                "PYTHONWARNINGS": "error",
                "REMOVE_PRESENT": "ambient",
            },
        )

    def test_run_forwards_every_subprocess_and_environment_argument(self) -> None:
        child_environment = {"PUBLIC": "value"}
        with (
            patch.object(
                tasks,
                "_task_environment",
                return_value=child_environment,
            ) as task_environment,
            patch("tools.tasks.subprocess.run") as run,
        ):
            tasks._run(
                ("public-command", "argument"),
                profile="project-public",
                timeout_seconds=17,
                environment_updates={"UPDATE": "public"},
                environment_removals=("REMOVE",),
            )
            tasks._run(("default-command",))

        self.assertEqual(
            task_environment.call_args_list,
            [
                call(
                    profile="project-public",
                    environment_updates={"UPDATE": "public"},
                    environment_removals=("REMOVE",),
                ),
                call(
                    profile=None,
                    environment_updates=None,
                    environment_removals=(),
                ),
            ],
        )
        self.assertEqual(
            run.call_args_list,
            [
                call(
                    ("public-command", "argument"),
                    check=True,
                    cwd=tasks.PROJECT_ROOT,
                    env=child_environment,
                    timeout=17,
                ),
                call(
                    ("default-command",),
                    check=True,
                    cwd=tasks.PROJECT_ROOT,
                    env=child_environment,
                    timeout=600,
                ),
            ],
        )

    def test_isolated_run_forwards_exact_storage_and_observability_policy(self) -> None:
        storage = Path("/public/isolated-hypothesis")
        with patch.object(tasks, "_run") as run:
            tasks._isolated_run(
                storage,
                ("public-command",),
                profile="project-public",
                timeout_seconds=19,
                environment_updates={"PUBLIC": "value"},
            )
            tasks._isolated_run(storage, ("default-command",))

        storage_update = {
            tasks.hypothesis_runner.STORAGE_ENVIRONMENT_VARIABLE: str(storage),
        }
        self.assertEqual(
            run.call_args_list,
            [
                call(
                    ("public-command",),
                    profile="project-public",
                    timeout_seconds=19,
                    environment_updates={"PUBLIC": "value", **storage_update},
                    environment_removals=tasks.OBSERVABILITY_VARIABLES,
                ),
                call(
                    ("default-command",),
                    profile=None,
                    timeout_seconds=600,
                    environment_updates=storage_update,
                    environment_removals=tasks.OBSERVABILITY_VARIABLES,
                ),
            ],
        )

    def test_test_task_has_an_exact_isolated_pytest_contract(self) -> None:
        storage = Path("/public/private-root/.hypothesis")

        def run_action(action: object, *_args: object, **_kwargs: object) -> None:
            self.assertTrue(callable(action))
            action(storage)  # type: ignore[operator]

        with tempfile.TemporaryDirectory() as directory:
            build = Path(directory) / "build"
            with (
                patch.object(tasks, "BUILD_DIRECTORY", build),
                patch.object(tasks, "_run") as run,
                patch.object(
                    tasks.hypothesis_runner,
                    "run_isolated",
                    side_effect=run_action,
                ) as isolated,
            ):
                tasks._test("project-development")
                build_created = build.is_dir()

        self.assertTrue(build_created)
        self.assertEqual(
            isolated.call_args,
            call(
                ANY,
                tasks.PROJECT_ROOT,
                observations=False,
                report_destination=build / "test-results-project-development.xml",
            ),
        )
        self.assertEqual(
            run.call_args,
            call(
                (
                    sys.executable,
                    "-X",
                    "dev",
                    "-W",
                    "error",
                    "-m",
                    "pytest",
                    "-vv",
                    "-p",
                    "no:cacheprovider",
                    "--hypothesis-show-statistics",
                    f"--junitxml={storage.parent / 'test-results.xml'}",
                ),
                profile="project-development",
                timeout_seconds=tasks.DEVELOPMENT_TEST_TIMEOUT_SECONDS,
                environment_updates={
                    tasks.hypothesis_runner.STORAGE_ENVIRONMENT_VARIABLE: str(storage),
                },
                environment_removals=tasks.OBSERVABILITY_VARIABLES,
            ),
        )

    def test_observable_thorough_task_has_an_exact_override_contract(self) -> None:
        storage = Path("/public/private-root/.hypothesis")

        def run_action(action: object, *_args: object, **_kwargs: object) -> None:
            self.assertTrue(callable(action))
            action(storage)  # type: ignore[operator]

        with tempfile.TemporaryDirectory() as directory:
            build = Path(directory) / "build"
            with (
                patch.object(tasks, "BUILD_DIRECTORY", build),
                patch.object(tasks, "_run") as run,
                patch.object(
                    tasks.hypothesis_runner,
                    "run_isolated",
                    side_effect=run_action,
                ) as isolated,
            ):
                tasks._test(
                    "project-thorough",
                    timeout_seconds=23,
                    observable=True,
                )

        self.assertEqual(
            isolated.call_args,
            call(
                ANY,
                tasks.PROJECT_ROOT,
                observations=True,
                report_destination=build / "test-results-project-thorough.xml",
            ),
        )
        self.assertEqual(run.call_args.kwargs["profile"], "project-thorough")
        self.assertEqual(run.call_args.kwargs["timeout_seconds"], 23)
        self.assertEqual(
            run.call_args.kwargs["environment_updates"],
            {
                "HYPOTHESIS_EXPERIMENTAL_OBSERVABILITY": "1",
                tasks.hypothesis_runner.STORAGE_ENVIRONMENT_VARIABLE: str(storage),
            },
        )

    def test_coverage_task_has_an_exact_command_and_report_contract(self) -> None:
        storage = Path("/public/private-root/.hypothesis")

        def run_action(action: object, *_args: object, **_kwargs: object) -> None:
            self.assertTrue(callable(action))
            action(storage)  # type: ignore[operator]

        with tempfile.TemporaryDirectory() as directory:
            build = Path(directory) / "build"
            coverage_data = storage.parent / ".coverage"
            coverage_environment = {"COVERAGE_FILE": str(coverage_data)}
            with (
                patch.object(tasks, "BUILD_DIRECTORY", build),
                patch.object(tasks, "_run") as run,
                patch.object(
                    tasks.hypothesis_runner,
                    "run_isolated",
                    side_effect=run_action,
                ) as isolated,
            ):
                tasks._coverage()
                build_created = build.is_dir()

        self.assertTrue(build_created)
        self.assertEqual(
            isolated.call_args,
            call(
                ANY,
                tasks.PROJECT_ROOT,
                report_destination=build / "test-results-project-ci.xml",
            ),
        )
        self.assertEqual(
            run.call_args_list,
            [
                call(
                    (sys.executable, "-m", "coverage", "erase"),
                    profile=None,
                    timeout_seconds=600,
                    environment_updates={
                        **coverage_environment,
                        tasks.hypothesis_runner.STORAGE_ENVIRONMENT_VARIABLE: str(
                            storage
                        ),
                    },
                    environment_removals=tasks.OBSERVABILITY_VARIABLES,
                ),
                call(
                    (
                        sys.executable,
                        "-X",
                        "dev",
                        "-W",
                        "error",
                        "-m",
                        "coverage",
                        "run",
                        "-m",
                        "pytest",
                        "-vv",
                        "-p",
                        "no:cacheprovider",
                        "--hypothesis-show-statistics",
                        f"--junitxml={storage.parent / 'test-results.xml'}",
                    ),
                    profile="project-ci",
                    timeout_seconds=600,
                    environment_updates={
                        **coverage_environment,
                        tasks.hypothesis_runner.STORAGE_ENVIRONMENT_VARIABLE: str(
                            storage
                        ),
                    },
                    environment_removals=tasks.OBSERVABILITY_VARIABLES,
                ),
                call(
                    (sys.executable, "-m", "coverage", "combine"),
                    profile=None,
                    timeout_seconds=600,
                    environment_updates={
                        **coverage_environment,
                        tasks.hypothesis_runner.STORAGE_ENVIRONMENT_VARIABLE: str(
                            storage
                        ),
                    },
                    environment_removals=tasks.OBSERVABILITY_VARIABLES,
                ),
                call(
                    (
                        sys.executable,
                        "tools/report_coverage.py",
                        "--data-file",
                        str(coverage_data),
                        "--xml-output",
                        str(build / "coverage.xml"),
                    ),
                    profile=None,
                    timeout_seconds=600,
                    environment_updates={
                        **coverage_environment,
                        tasks.hypothesis_runner.STORAGE_ENVIRONMENT_VARIABLE: str(
                            storage
                        ),
                    },
                    environment_removals=tasks.OBSERVABILITY_VARIABLES,
                ),
            ],
        )

    def test_build_and_release_tasks_use_exact_public_boundaries(self) -> None:
        with patch.object(tasks, "_run") as run:
            tasks._build_zipapp()
            tasks._qualify_release()

        self.assertEqual(
            run.call_args_list,
            [
                call((
                    sys.executable,
                    "tools/build_zipapp.py",
                    "--target",
                    str(tasks.BUILD_TARGET),
                )),
                call(
                    (
                        sys.executable,
                        "tools/qualify_release.py",
                        "--output-directory",
                        str(tasks.RELEASE_DIRECTORY),
                    ),
                ),
            ],
        )


if __name__ == "__main__":
    unittest.main(verbosity=2)
