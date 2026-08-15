"""Regression tests for repository-owned quality and build tooling."""

from __future__ import annotations

import argparse
import io
import os
import sys
import tempfile
import unittest
from pathlib import Path
from unittest.mock import MagicMock, call, patch

from tools import tasks

from tests.mutmut_environment_support import selector_preserving_environment


class TaskRunnerTests(unittest.TestCase):
    """Verify that documented tasks dispatch exact, bounded commands."""

    def test_run_propagates_strict_environment_profile_and_timeout(self) -> None:
        controlled_environment = {
            "COVERAGE_PROCESS_START": "ambient startup override",
            "COVERAGE_RCFILE": "ambient configuration override",
            "COVERAGE_UNRECOGNIZED_INPUT": "ambient future override",
            "HYPOTHESIS_STORAGE_DIRECTORY": "ambient storage override",
            "KEEP": "public",
            "PYTHONPATH": "ambient source override",
            "REMOVE": "not inherited",
        }
        with (
            patch(
                "tools.tasks.os.environ.copy",
                side_effect=lambda: selector_preserving_environment(
                    controlled_environment
                ),
            ),
            patch("tools.tasks.subprocess.run") as run,
        ):
            tasks._run(
                ("public-command",),
                profile="public-profile",
                timeout_seconds=7,
                environment_updates={
                    "COVERAGE_RCFILE": "explicit configuration",
                    "UPDATE": "public",
                },
                environment_removals=("REMOVE",),
            )
            tasks._run(("default-command",))
        keyword_arguments = run.call_args_list[0].kwargs
        self.assertEqual(run.call_args_list[0].args[0], ("public-command",))
        self.assertEqual(keyword_arguments["cwd"], tasks.PROJECT_ROOT)
        self.assertEqual(keyword_arguments["timeout"], 7)
        self.assertNotIn("REMOVE", keyword_arguments["env"])
        self.assertNotIn("HYPOTHESIS_STORAGE_DIRECTORY", keyword_arguments["env"])
        self.assertNotIn("PYTHONPATH", keyword_arguments["env"])
        self.assertEqual(
            keyword_arguments["env"]["COVERAGE_PROCESS_START"],
            "ambient startup override",
        )
        self.assertEqual(
            keyword_arguments["env"]["COVERAGE_UNRECOGNIZED_INPUT"],
            "ambient future override",
        )
        self.assertEqual(
            keyword_arguments["env"]["COVERAGE_RCFILE"],
            "explicit configuration",
        )
        self.assertEqual(keyword_arguments["env"]["KEEP"], "public")
        self.assertEqual(keyword_arguments["env"]["UPDATE"], "public")
        self.assertEqual(keyword_arguments["env"]["PYTHONDEVMODE"], "1")
        self.assertEqual(keyword_arguments["env"]["PYTHONDONTWRITEBYTECODE"], "1")
        self.assertEqual(keyword_arguments["env"]["PYTHONNOUSERSITE"], "1")
        self.assertEqual(keyword_arguments["env"]["PYTHONWARNINGS"], "error")
        self.assertEqual(
            keyword_arguments["env"]["HYPOTHESIS_PROFILE"],
            "public-profile",
        )
        default_environment = run.call_args_list[1].kwargs["env"]
        self.assertNotIn("HYPOTHESIS_PROFILE", default_environment)
        self.assertNotIn("UPDATE", default_environment)
        self.assertEqual(
            default_environment["COVERAGE_RCFILE"],
            "ambient configuration override",
        )

    def test_check_runs_every_static_gate(self) -> None:
        public_files = (Path("/public/README.md"), Path("/public/src/public.py"))
        audit = MagicMock(issues=(), public_files=public_files)
        with (
            patch.object(tasks, "_run") as run,
            patch.object(
                tasks.check_repository_hygiene,
                "audit_repository",
                return_value=audit,
            ),
        ):
            tasks._check()
        self.assertEqual(run.call_count, 8)
        self.assertEqual(
            run.call_args_list[0],
            call((sys.executable, "tools/check_repository_hygiene.py")),
        )
        self.assertEqual(
            run.call_args_list[1],
            call(("ruff", "format", "--check", "--no-cache", ".")),
        )
        self.assertEqual(
            run.call_args_list[2], call(("ruff", "check", "--no-cache", "."))
        )
        self.assertEqual(
            run.call_args_list[3],
            call(("mypy", "--no-incremental", "--cache-dir", os.devnull)),
        )
        self.assertEqual(
            run.call_args_list[4],
            call((sys.executable, "tools/check_module_design.py")),
        )
        actionlint_command = run.call_args_list[5].args[0]
        self.assertEqual(
            actionlint_command,
            (
                "actionlint",
                "-shellcheck",
                "shellcheck",
                "-pyflakes",
                "pyflakes",
                *tasks.WORKFLOW_FILES,
            ),
        )
        self.assertEqual(
            run.call_args_list[6],
            call(("shellcheck", "--shell=sh", *tasks.SHELL_SCRIPTS)),
        )
        secret_command = run.call_args_list[7].args[0]
        self.assertEqual(secret_command[:2], ("detect-secrets-hook", "--no-verify"))
        self.assertTrue(
            all(str(path) in secret_command for path in public_files),
        )

    def test_check_preserves_multiline_dirty_workspace_diagnostics(self) -> None:
        audit = MagicMock(issues=(object(),), public_files=())
        audit.diagnostics.return_value = (
            "alpha.txt: first issue",
            "beta.txt: second issue",
        )
        with (
            patch.object(tasks, "_run"),
            patch.object(
                tasks.check_repository_hygiene,
                "audit_repository",
                return_value=audit,
            ),
            self.assertRaises(RuntimeError) as raised,
        ):
            tasks._check()
        self.assertEqual(
            str(raised.exception),
            "repository changed before secret scanning:\n"
            "alpha.txt: first issue\n"
            "beta.txt: second issue",
        )
        audit.diagnostics.assert_called_once_with(tasks.PROJECT_ROOT)

    def test_check_rejects_a_workspace_dirtied_after_initial_hygiene(self) -> None:
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory).resolve()

            def dirty_after_first_gate(*_args: object, **_kwargs: object) -> None:
                (root / "late-public.txt").write_text("PUBLIC\n", encoding="utf-8")

            with (
                patch.object(tasks, "PROJECT_ROOT", root),
                patch.object(tasks, "_run", side_effect=dirty_after_first_gate) as run,
                self.assertRaisesRegex(
                    RuntimeError,
                    r"repository changed before secret scanning:\n"
                    r"late-public\.txt: unexpected top-level regular file",
                ),
            ):
                tasks._check()
        self.assertEqual(run.call_count, 7)
        self.assertFalse(
            any(
                call_args.args[0][0] == "detect-secrets-hook"
                for call_args in run.call_args_list
            ),
        )

    def test_test_coverage_mutation_and_build_commands(self) -> None:
        with tempfile.TemporaryDirectory() as directory:
            mutation_root = Path(directory) / "mutants"
            statistics = mutation_root / "mutmut-cicd-stats.json"
            results = Path(directory) / "mutmut-results.txt"
            tools = Path(directory) / "tools"
            tools.mkdir()
            (tools / "mutmut.coveragerc").write_text(
                "[run]\nrelative_files = false\n",
                encoding="utf-8",
            )
            mutation_root.mkdir()
            with (
                patch.object(tasks, "PROJECT_ROOT", Path(directory)),
                patch.object(tasks, "MUTATION_STATISTICS", statistics),
                patch.object(tasks, "MUTATION_RESULTS", results),
                patch.object(tasks, "_capture_mutation_results"),
                patch.object(
                    tasks.hypothesis_runner,
                    "run_isolated",
                    side_effect=lambda action, *_args, **_kwargs: action(
                        Path("/public/private-hypothesis")
                    ),
                ),
                patch.object(tasks, "_run") as run,
            ):
                tasks._test("project-development")
                tasks._test("project-thorough")
                tasks._coverage()
                tasks._mutation()
                tasks._build_zipapp()
        self.assertEqual(run.call_args_list[0].kwargs["timeout_seconds"], 600)
        self.assertEqual(run.call_args_list[1].kwargs["timeout_seconds"], 1_800)
        commands = [entry.args[0] for entry in run.call_args_list]
        self.assertTrue(any("pytest" in command for command in commands))
        self.assertTrue(any("coverage" in command for command in commands))
        coverage_file = str(Path("/public/.coverage"))
        coverage_calls = [
            entry for entry in run.call_args_list if "coverage" in entry.args[0]
        ]
        self.assertTrue(coverage_calls)
        self.assertTrue(
            all(
                entry.kwargs.get("environment_updates", {}).get("COVERAGE_FILE")
                == coverage_file
                for entry in coverage_calls
            ),
        )
        self.assertTrue(
            any(
                "report_coverage.py" in argument
                for command in commands
                for argument in command
            )
        )
        self.assertIn(("mutmut", "run"), commands)
        self.assertTrue(
            any(
                "check_mutation_results.py" in argument
                for command in commands
                for argument in command
            ),
        )
        self.assertTrue(
            any(
                any("build_zipapp.py" in argument for argument in command)
                for command in commands
            ),
        )
        self.assertEqual(
            tasks._test_result_path("public").name,
            "test-results-public.xml",
        )

    def test_test_can_enable_observability_with_a_custom_timeout(self) -> None:
        with (
            patch.object(tasks, "_run") as run,
            patch.object(tasks.hypothesis_runner, "run_isolated") as isolate,
        ):
            tasks._test("project-thorough", timeout_seconds=17, observable=True)
            isolate.assert_called_once()
            self.assertEqual(isolate.call_args.args[1], tasks.PROJECT_ROOT)
            self.assertTrue(isolate.call_args.kwargs["observations"])
            storage = Path("/public/private-hypothesis")
            isolate.call_args.args[0](storage)
            keyword_arguments = run.call_args.kwargs
        self.assertEqual(keyword_arguments["timeout_seconds"], 17)
        self.assertEqual(
            keyword_arguments["environment_updates"],
            {
                "HYPOTHESIS_EXPERIMENTAL_OBSERVABILITY": "1",
                "HYPOTHESIS_STORAGE_DIRECTORY": str(storage),
            },
        )
        self.assertEqual(
            keyword_arguments["environment_removals"],
            tasks.OBSERVABILITY_VARIABLES,
        )

    def test_release_task_runs_portable_qualification_tool(self) -> None:
        with patch.object(tasks, "_run") as run:
            tasks._qualify_release()
        self.assertEqual(run.call_count, 1)
        run.assert_called_once_with(
            (
                sys.executable,
                "tools/qualify_release.py",
                "--output-directory",
                str(tasks.RELEASE_DIRECTORY),
            ),
        )

    def test_mutation_result_capture_delegates_canonical_inputs(self) -> None:
        paths = object()
        environment = {"PUBLIC": "value"}
        with (
            patch.object(tasks, "_mutation_paths", return_value=paths),
            patch.object(tasks, "_task_environment", return_value=environment),
            patch.object(tasks.mutation_task, "capture_results") as capture,
        ):
            tasks._capture_mutation_results()
        self.assertEqual(capture.call_args, call(paths, sys.executable, environment))

    def test_mutation_rejects_native_windows_before_orchestration(self) -> None:
        with (
            patch.object(sys, "platform", "win32"),
            patch.object(tasks.hypothesis_runner, "run_isolated") as isolate,
            self.assertRaisesRegex(RuntimeError, "requires process fork support"),
        ):
            tasks._mutation()
        isolate.assert_not_called()

    def test_quality_and_main_dispatch_every_task(self) -> None:
        with (
            patch.object(tasks, "_check") as check,
            patch.object(tasks, "_coverage") as coverage,
        ):
            tasks._quality()
        check.assert_called_once_with()
        coverage.assert_called_once_with()

        task_names = (
            "build",
            "check",
            "coverage",
            "mutation",
            "quality",
            "release",
        )
        function_names = (
            "_build_zipapp",
            "_check",
            "_coverage",
            "_mutation",
            "_quality",
            "_qualify_release",
        )
        for task_name, function_name in zip(task_names, function_names, strict=True):
            with self.subTest(task=task_name):
                with patch.object(tasks, function_name) as action:
                    self.assertEqual(tasks.main([task_name]), 0)
                action.assert_called_once_with()
        with patch.object(tasks, "_test") as test:
            self.assertEqual(tasks.main(["test"]), 0)
            self.assertEqual(tasks.main(["thorough"]), 0)
            self.assertEqual(
                tasks.main(
                    ["thorough", "--observable", "--timeout-seconds", "17"],
                ),
                0,
            )
        self.assertEqual(
            test.call_args_list,
            [
                call("project-development"),
                call(
                    "project-thorough",
                    timeout_seconds=None,
                    observable=False,
                ),
                call(
                    "project-thorough",
                    timeout_seconds=17,
                    observable=True,
                ),
            ],
        )

    def test_timeout_parser_and_task_option_validation(self) -> None:
        self.assertEqual(tasks._positive_timeout("1.5"), 1.5)
        for value, message in (
            ("invalid", "positive number"),
            ("0", "finite and greater than zero"),
            ("nan", "finite and greater than zero"),
            ("inf", "finite and greater than zero"),
            ("-inf", "finite and greater than zero"),
        ):
            with self.subTest(value=value):
                with self.assertRaisesRegex(argparse.ArgumentTypeError, message):
                    tasks._positive_timeout(value)
        with (
            patch("sys.stderr", new_callable=io.StringIO),
            self.assertRaises(SystemExit),
        ):
            tasks.main(["test", "--observable"])


if __name__ == "__main__":
    unittest.main(verbosity=2)
