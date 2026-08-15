"""Mutation orchestration subprocess and finite-timeout contracts."""

from __future__ import annotations

import subprocess
import tempfile
import unittest
from pathlib import Path
from typing import TYPE_CHECKING
from unittest.mock import patch

from tools import mutation_task

if TYPE_CHECKING:
    from collections.abc import Mapping, Sequence


class MutationTaskContracts(unittest.TestCase):
    """Pin direct subprocess and finite-timeout orchestration interfaces."""

    def test_capture_results_requests_decoded_utf8_and_writes_sorted_evidence(
        self,
    ) -> None:
        completed = subprocess.CompletedProcess(
            ("public",),
            0,
            stdout=" public.z: survived\npublic.a: killed\n",
        )
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            paths = mutation_task.mutation_paths(
                root,
                root / "build",
                root / "mutants" / "stats.json",
                root / "results.txt",
                root / "equivalents.json",
            )
            real_write_text = Path.write_text
            with (
                patch(
                    "tools.mutation_task.subprocess.run",
                    return_value=completed,
                ) as run,
                patch.object(
                    Path,
                    "write_text",
                    autospec=True,
                    side_effect=real_write_text,
                ) as write,
            ):
                mutation_task.capture_results(paths, "python", {"PUBLIC": "1"})
            self.assertEqual(
                paths.results.read_text(encoding="utf-8"),
                "public.a: killed\npublic.z: survived\n",
            )
        write.assert_called_once_with(
            paths.results,
            "public.a: killed\npublic.z: survived\n",
            encoding="utf-8",
        )
        run.assert_called_once_with(
            ("python", "-m", "mutmut", "results", "--all", "true"),
            check=True,
            cwd=root,
            env={"PUBLIC": "1"},
            timeout=120,
            capture_output=True,
            encoding="utf-8",
        )

    def test_mutation_operations_use_exact_profiles_and_finite_timeouts(self) -> None:
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            tools = root / "tools"
            tools.mkdir()
            (tools / "mutmut.coveragerc").write_text("[run]\n", encoding="utf-8")
            paths = mutation_task.mutation_paths(
                root,
                root / "build",
                root / "mutants" / "stats.json",
                root / "build" / "results.txt",
                root / "equivalents.json",
            )
            run_calls: list[tuple[tuple[str, ...], dict[str, object]]] = []

            def run(
                command: Sequence[str],
                *,
                profile: str | None = None,
                timeout_seconds: float | None = 600,
                environment_updates: Mapping[str, str] | None = None,
                environment_removals: Sequence[str] = (),
            ) -> None:
                run_calls.append((
                    tuple(command),
                    {
                        "profile": profile,
                        "timeout_seconds": timeout_seconds,
                        "environment_updates": environment_updates,
                        "environment_removals": tuple(environment_removals),
                    },
                ))

            events: list[str] = []
            actions = mutation_task.mutation_actions(
                lambda: events.append("coverage"),
                lambda: events.append("cleanup"),
                lambda: events.append("capture"),
                run,
            )

            mutation_task.run_mutation(paths, actions, "python")

        self.assertEqual(events, ["cleanup", "coverage", "capture"])
        self.assertEqual(
            run_calls,
            [
                (
                    ("mutmut", "run"),
                    {
                        "profile": "project-mutation",
                        "timeout_seconds": 7_200,
                        "environment_updates": {
                            "COVERAGE_RCFILE": str(paths.coverage_config),
                        },
                        "environment_removals": (),
                    },
                ),
                (
                    ("mutmut", "export-cicd-stats"),
                    {
                        "profile": None,
                        "timeout_seconds": 120,
                        "environment_updates": None,
                        "environment_removals": (),
                    },
                ),
                (
                    (
                        "python",
                        "tools/check_mutation_results.py",
                        str(paths.statistics),
                        "--results",
                        str(paths.results),
                        "--equivalents",
                        str(paths.equivalents),
                    ),
                    {
                        "profile": None,
                        "timeout_seconds": 120,
                        "environment_updates": None,
                        "environment_removals": (),
                    },
                ),
            ],
        )

    def test_mutation_failure_group_has_exact_public_message(self) -> None:
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            tools = root / "tools"
            tools.mkdir()
            (tools / "mutmut.coveragerc").write_text("[run]\n", encoding="utf-8")
            paths = mutation_task.mutation_paths(
                root,
                root / "build",
                root / "mutants" / "stats.json",
                root / "build" / "results.txt",
                root / "equivalents.json",
            )

            failure_message = "failed"

            def fail(*_args: object, **_kwargs: object) -> None:
                raise subprocess.SubprocessError(failure_message)

            actions = mutation_task.mutation_actions(
                lambda: None,
                lambda: None,
                lambda: None,
                fail,
            )
            with self.assertRaises(ExceptionGroup) as raised:
                mutation_task.run_mutation(paths, actions, "python")
        self.assertEqual(
            str(raised.exception).split(" (3 sub-exceptions)", maxsplit=1)[0],
            "mutation gate failed",
        )
        self.assertEqual(len(raised.exception.exceptions), 3)


if __name__ == "__main__":
    unittest.main(verbosity=2)
