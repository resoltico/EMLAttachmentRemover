"""Exact command-line contracts for the portable task runner."""

from __future__ import annotations

import io
import os
import runpy
import subprocess
import sys
import tempfile
import unittest
from contextlib import redirect_stderr, redirect_stdout
from pathlib import Path
from unittest.mock import call, patch

from tools import tasks


class TaskCliContractTests(unittest.TestCase):
    """Keep launcher safety and public argument behavior exact."""

    def test_launcher_disables_bytecode_before_dynamic_repository_imports(self) -> None:
        """Protect the launcher itself before child-process policy is available."""
        source = Path(tasks.__file__).read_text(encoding="utf-8")
        assignment = source.index("sys.dont_write_bytecode = True")
        first_repository_import = source.index("check_repository_hygiene = cast(")
        self.assertLess(assignment, first_repository_import)
        self.assertTrue(sys.dont_write_bytecode)

    def test_launcher_prevents_repository_bytecode_without_an_ambient_guard(
        self,
    ) -> None:
        """Field-test imports when a direct caller did not request ``-B``."""
        environment = os.environ.copy()
        environment.pop("PYTHONDONTWRITEBYTECODE", None)
        with tempfile.TemporaryDirectory() as directory:
            environment["PYTHONPYCACHEPREFIX"] = directory
            result = subprocess.run(
                [sys.executable, str(tasks.__file__), "--help"],
                cwd=tasks.PROJECT_ROOT,
                env=environment,
                text=True,
                capture_output=True,
                check=False,
                timeout=30,
            )
            generated_names = {path.name for path in Path(directory).rglob("*.pyc")}

        self.assertEqual(result.returncode, 0, result.stderr)
        self.assertIn("usage: tasks.py", result.stdout)
        for stem in (
            "check_repository_hygiene",
            "repository_hygiene_types",
            "task_test_commands",
        ):
            self.assertFalse(
                any(name.startswith(f"{stem}.") for name in generated_names),
                generated_names,
            )

    def test_main_help_and_invalid_option_diagnostics_are_exact(self) -> None:
        missing_errors = io.StringIO()
        with (
            patch.object(sys, "argv", ["tasks.py"]),
            redirect_stderr(missing_errors),
            self.assertRaises(SystemExit) as missing_exit,
        ):
            tasks.main([])
        self.assertEqual(missing_exit.exception.code, 2)
        self.assertIn(
            "error: the following arguments are required: task\n",
            missing_errors.getvalue(),
        )

        output = io.StringIO()
        with (
            patch.object(sys, "argv", ["tasks.py"]),
            redirect_stdout(output),
            self.assertRaises(SystemExit) as help_exit,
        ):
            tasks.main(["--help"])
        self.assertEqual(help_exit.exception.code, 0)
        self.assertEqual(
            " ".join(output.getvalue().split()),
            "usage: tasks.py [-h] "
            "{build,check,ci,coverage,mutation,quality,release,test,thorough} ... "
            "Run portable local development and release-preparation tasks. "
            "positional arguments: "
            "{build,check,ci,coverage,mutation,quality,release,test,thorough} "
            "options: -h, --help show this help message and exit",
        )

        thorough_output = io.StringIO()
        with (
            patch.object(sys, "argv", ["tasks.py"]),
            redirect_stdout(thorough_output),
            self.assertRaises(SystemExit) as thorough_help_exit,
        ):
            tasks.main(["thorough", "--help"])
        self.assertEqual(thorough_help_exit.exception.code, 0)
        self.assertEqual(
            " ".join(thorough_output.getvalue().split()),
            "usage: tasks.py thorough [-h] "
            "[--timeout-seconds TIMEOUT_SECONDS] [--observable] "
            "options: -h, --help show this help message and exit "
            "--timeout-seconds TIMEOUT_SECONDS whole pytest timeout "
            "--observable write public Hypothesis observations",
        )

        for task, option_help in (
            (
                "mutation",
                (
                    "usage: tasks.py mutation [-h] [--allow-stale-manifest] "
                    "[--workers WORKERS] options: -h, --help show this help message "
                    "and exit --allow-stale-manifest skip only the equivalence "
                    "preflight, to gather rebind evidence --workers WORKERS Mutmut "
                    "workers: auto or 1-64 (default: auto)"
                ),
            ),
            (
                "ci",
                (
                    "usage: tasks.py ci [-h] [--release-tag RELEASE_TAG] "
                    "[--workers WORKERS] options: -h, --help show this help message "
                    "and exit --release-tag RELEASE_TAG tag to validate like the "
                    "release workflow (default: v<version>) --workers WORKERS Mutmut "
                    "workers: auto or 1-64 (default: auto)"
                ),
            ),
        ):
            task_output = io.StringIO()
            with (
                self.subTest(task=task),
                patch.object(sys, "argv", ["tasks.py"]),
                redirect_stdout(task_output),
                self.assertRaises(SystemExit) as task_help_exit,
            ):
                tasks.main([task, "--help"])
            self.assertEqual(task_help_exit.exception.code, 0)
            self.assertEqual(" ".join(task_output.getvalue().split()), option_help)

        for arguments in (
            ["test", "--observable"],
            ["test", "--timeout-seconds", "1"],
            ["test", "--workers", "2"],
            ["quality", "--allow-stale-manifest"],
            ["build", "--release-tag", "v1"],
        ):
            with self.subTest(arguments=arguments):
                errors = io.StringIO()
                with (
                    patch.object(tasks, "_test") as test,
                    redirect_stderr(errors),
                    self.assertRaises(SystemExit) as invalid_exit,
                ):
                    tasks.main(arguments)
                self.assertEqual(invalid_exit.exception.code, 2)
                self.assertIn(
                    f"error: unrecognized arguments: {' '.join(arguments[1:])}\n",
                    errors.getvalue(),
                )
                test.assert_not_called()

    def test_mutation_and_ci_options_reach_their_tasks(self) -> None:
        with patch.object(tasks, "_mutation") as mutation:
            self.assertEqual(tasks.main(["mutation"]), 0)
            self.assertEqual(
                tasks.main(["mutation", "--workers", "3", "--allow-stale-manifest"]),
                0,
            )
        self.assertEqual(
            mutation.call_args_list,
            [call(workers=None, preflight=True), call(workers=3, preflight=False)],
        )
        with patch.object(tasks, "_ci") as local_ci:
            self.assertEqual(tasks.main(["ci"]), 0)
            self.assertEqual(
                tasks.main(["ci", "--release-tag", "v9.9.9", "--workers", "2"]),
                0,
            )
        self.assertEqual(
            local_ci.call_args_list,
            [call(None, None), call("v9.9.9", 2)],
        )

    def test_tools_import_as_package_modules_or_script_siblings(self) -> None:
        self.assertIs(tasks._tool("task_timeout"), tasks.task_timeout)  # ruff: ignore[private-member-access] - import contract.
        tools_directory = str(tasks.PROJECT_ROOT / "tools")
        with (
            patch.object(tasks, "__package__", ""),
            patch.object(sys, "path", [tools_directory, *sys.path]),
            patch.dict(sys.modules),
        ):
            sibling = tasks._tool("task_timeout")  # ruff: ignore[private-member-access] - import contract.
            self.assertEqual(getattr(sibling, "__name__", None), "task_timeout")
            self.assertIsNot(sibling, tasks.task_timeout)

    def test_script_entrypoint_exposes_help_without_running_a_task(self) -> None:
        """Exercise the real ``__main__`` branch through a harmless request."""
        path = Path(tasks.__file__)
        output = io.StringIO()
        with (
            patch.object(sys, "argv", [str(path), "--help"]),
            patch.object(sys, "path", [str(path.parent), *sys.path]),
            redirect_stdout(output),
            self.assertRaises(SystemExit) as raised,
        ):
            runpy.run_path(str(path), run_name="__main__")

        self.assertEqual(raised.exception.code, 0)
        self.assertIn("usage: tasks.py", output.getvalue())


if __name__ == "__main__":
    unittest.main(verbosity=2)
