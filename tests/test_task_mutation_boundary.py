"""Finite exact contracts for mutation-task dispatch and timeout parsing."""

from __future__ import annotations

import argparse
import os
import sys
import unittest
from pathlib import Path
from unittest.mock import ANY, call, patch

from tools import tasks

from tests.mutmut_environment_support import selector_preserving_environment


class TaskMutationBoundaryTests(unittest.TestCase):
    """Keep mutation testing isolated, bounded, and directly testable."""

    def test_mutation_runner_closure_forwards_exact_isolation_inputs(self) -> None:
        storage = Path("/public/private-root/.hypothesis")
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
        storage_update = {
            "COVERAGE_FILE": str(storage.parent / ".mutmut-coverage"),
            tasks.hypothesis_runner.STORAGE_ENVIRONMENT_VARIABLE: str(storage),
        }
        common_removals = (
            *tasks.OBSERVABILITY_VARIABLES,
            "COVERAGE_Z",
            "COVERAGE_A",
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
