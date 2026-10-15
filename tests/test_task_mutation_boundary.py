"""Finite exact contracts for mutation-task dispatch and timeout parsing."""

from __future__ import annotations

import argparse
import os
import stat
import sys
import tempfile
import unittest
from pathlib import Path
from unittest.mock import ANY, call, patch

from tools import tasks

from tests.mutmut_environment_support import selector_preserving_environment


class TaskMutationBoundaryTests(unittest.TestCase):
    """Keep mutation testing isolated, bounded, and directly testable."""

    def test_hard_exit_child_tempfiles_remain_owned_by_campaign(self) -> None:
        project = tasks.PROJECT_ROOT
        retained: list[Path] = []

        def action(storage: Path) -> None:
            with patch.object(tasks, "_run", wraps=tasks._run) as run:
                runner = tasks.mutation_task.command_runner(
                    run,
                    storage,
                    tasks.hypothesis_runner.STORAGE_ENVIRONMENT_VARIABLE,
                    tasks.OBSERVABILITY_VARIABLES,
                    project,
                )
                # Simulate a worker that cannot run TemporaryDirectory finalizers.
                runner((
                    sys.executable,
                    "-c",
                    (
                        "import os, pathlib, tempfile; "
                        "root = pathlib.Path(tempfile.mkdtemp(prefix='worker-')); "
                        "(root / 'journal').write_bytes(b'child calls'); os._exit(0)"
                    ),
                ))
            retained.append(Path(run.call_args.kwargs["environment_updates"]["TMPDIR"]))
            self.assertEqual(retained[-1].parent, storage.parent)
            if os.name != "nt":
                self.assertEqual(stat.S_IMODE(retained[-1].stat().st_mode), 0o700)
            self.assertEqual(len(list(retained[-1].iterdir())), 1)
            self.assertEqual(
                next(retained[-1].iterdir()).joinpath("journal").read_bytes(),
                b"child calls",
            )

        previous_umask = os.umask(0)
        try:
            tasks.hypothesis_runner.run_isolated(action, project)
        finally:
            os.umask(previous_umask)
        self.assertFalse(retained[0].parent.exists())

    def test_mutation_runner_closure_forwards_exact_isolation_inputs(self) -> None:
        directory = self.enterContext(tempfile.TemporaryDirectory())
        storage = Path(directory) / ".hypothesis"
        path_bundle = object()
        controlled_environment = {
            "COVERAGE_Z": "z",
            "COVERAGE_A": "a",
            "PUBLIC": "value",
        }

        def run_action(action: object, *_args: object, **_kwargs: object) -> None:
            self.assertTrue(callable(action))
            action(storage)  # type: ignore[operator]

        with (
            patch.dict(
                os.environ,
                selector_preserving_environment(controlled_environment),
                clear=True,
            ),
            patch.object(sys, "platform", "linux"),
            patch.object(tasks, "_mutation_paths", return_value=path_bundle),
            patch.object(tasks, "_run") as run,
            patch.object(
                tasks.hypothesis_runner,
                "run_isolated",
                side_effect=run_action,
            ) as isolated,
            patch.object(tasks.mutation_task, "run_mutation") as run_mutation,
        ):
            tasks._mutation()
            self.assertEqual(isolated.call_args, call(ANY, tasks.PROJECT_ROOT))
            self.assertEqual(run_mutation.call_count, 1)
            self.assertIs(run_mutation.call_args.args[0], path_bundle)
            self.assertEqual(run_mutation.call_args.args[2], sys.executable)
            actions = run_mutation.call_args.args[1]
            actions.run(("default-command",))
            actions.run(
                ("public-command",),
                profile="project-public",
                timeout_seconds=29,
                environment_updates={"PUBLIC_UPDATE": "yes"},
                environment_removals=("PUBLIC_REMOVE",),
            )
        temporary = Path(run.call_args_list[0].kwargs["environment_updates"]["TMPDIR"])
        self.assertEqual(temporary.parent, storage.parent)
        self.assertTrue(temporary.is_dir())
        storage_update = {
            "COVERAGE_FILE": str(storage.parent / ".mutmut-coverage"),
            "EML_MUTATION_PYTEST_TEMPORARY_ROOT": str(storage),
            tasks.hypothesis_runner.STORAGE_ENVIRONMENT_VARIABLE: str(storage),
            "TMPDIR": str(temporary),
            "TEMP": str(temporary),
            "TMP": str(temporary),
        }
        common_removals = (
            *tasks.OBSERVABILITY_VARIABLES,
            "COVERAGE_Z",
            "COVERAGE_A",
            "PYTEST_ADDOPTS",
        )
        self.assertEqual(
            run.call_args_list,
            [
                call(
                    ("default-command",),
                    profile=None,
                    timeout_seconds=600,
                    environment_updates=storage_update,
                    environment_removals=common_removals,
                ),
                call(
                    ("public-command",),
                    profile="project-public",
                    timeout_seconds=29,
                    environment_updates={
                        "PUBLIC_UPDATE": "yes",
                        **storage_update,
                    },
                    environment_removals=(*common_removals, "PUBLIC_REMOVE"),
                ),
            ],
        )

    def test_mutation_platform_rejection_is_finite_and_exact(self) -> None:
        with (
            patch.object(sys, "platform", "win32"),
            patch.object(tasks.hypothesis_runner, "run_isolated") as isolated,
            self.assertRaises(RuntimeError) as raised,
        ):
            tasks._mutation()
        self.assertEqual(
            str(raised.exception),
            "mutation testing requires process fork support; run this task under "
            "WSL or on Linux/macOS",
        )
        isolated.assert_not_called()

    def test_positive_timeout_accepts_subsecond_values_and_has_exact_errors(
        self,
    ) -> None:
        self.assertEqual(tasks._positive_timeout("0.5"), 0.5)
        for value, message in (
            ("invalid", "timeout must be a positive number of seconds"),
            ("0", "timeout must be finite and greater than zero"),
        ):
            with self.subTest(value=value):
                with self.assertRaises(argparse.ArgumentTypeError) as raised:
                    tasks._positive_timeout(value)
                self.assertEqual(str(raised.exception), message)


if __name__ == "__main__":
    unittest.main(verbosity=2)
