"""Mutation orchestration subprocess and finite-timeout contracts."""

from __future__ import annotations

import argparse
import os
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
                events.append("run")
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

        workers = str(mutation_task.mutation_worker_count())
        self.assertEqual(
            events,
            ["run", "cleanup", "coverage", "run", "capture", "run", "run"],
        )
        self.assertEqual(
            run_calls,
            [
                (
                    (
                        "python",
                        "tools/check_mutation_results.py",
                        "--manifest-only",
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
                (
                    ("mutmut", "run", "--max-children", workers),
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

    def test_mutation_worker_count_is_bounded_and_leaves_host_capacity(self) -> None:
        self.assertEqual(mutation_task.mutation_worker_count(1), 1)
        self.assertEqual(mutation_task.mutation_worker_count(2), 1)
        self.assertEqual(mutation_task.mutation_worker_count(10), 8)
        self.assertEqual(mutation_task.mutation_worker_count(64), 8)
        self.assertEqual(mutation_task.mutation_worker_count(4), 2)
        with patch.object(os, "process_cpu_count", return_value=10):
            self.assertEqual(mutation_task.mutation_worker_count(), 8)
        with patch.object(os, "process_cpu_count", return_value=None):
            self.assertEqual(mutation_task.mutation_worker_count(), 1)

    def test_worker_option_accepts_auto_or_a_bounded_count(self) -> None:
        self.assertIsNone(mutation_task.parse_workers("auto"))
        self.assertEqual(mutation_task.parse_workers("1"), 1)
        self.assertEqual(mutation_task.parse_workers("64"), 64)
        for value in ("0", "65"):
            with (
                self.subTest(value=value),
                self.assertRaises(argparse.ArgumentTypeError) as raised,
            ):
                mutation_task.parse_workers(value)
            self.assertEqual(str(raised.exception), "workers must be auto or 1-64")
        with self.assertRaises(ValueError):
            mutation_task.parse_workers("four")

    def test_explicit_workers_and_skipped_preflight_reach_mutmut(self) -> None:
        with tempfile.TemporaryDirectory() as directory:
            paths = _paths(Path(directory))
            commands: list[tuple[str, ...]] = []
            actions = mutation_task.mutation_actions(
                lambda: None,
                lambda: None,
                lambda: None,
                lambda command, **_options: commands.append(tuple(command)),
            )
            mutation_task.run_mutation(
                paths,
                actions,
                "python",
                workers=3,
                preflight=False,
            )
        self.assertEqual(commands[0], ("mutmut", "run", "--max-children", "3"))
        self.assertEqual(len(commands), 3)

    def test_failed_preflight_stops_before_cleanup_or_coverage(self) -> None:
        with tempfile.TemporaryDirectory() as directory:
            paths = _paths(Path(directory))
            events: list[str] = []

            def reject(*_args: object, **_kwargs: object) -> None:
                events.append("preflight")
                raise subprocess.CalledProcessError(1, "preflight")

            actions = mutation_task.mutation_actions(
                lambda: events.append("coverage"),
                lambda: events.append("cleanup"),
                lambda: events.append("capture"),
                reject,
            )
            with self.assertRaises(subprocess.CalledProcessError):
                mutation_task.run_mutation(paths, actions, "python")
            self.assertFalse(paths.build_directory.exists())
        self.assertEqual(events, ["preflight"])

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
                mutation_task.run_mutation(paths, actions, "python", preflight=False)
        self.assertEqual(
            str(raised.exception).split(" (3 sub-exceptions)", maxsplit=1)[0],
            "mutation gate failed",
        )
        self.assertEqual(len(raised.exception.exceptions), 3)


def _paths(root: Path) -> mutation_task.MutationPaths:
    """Create canonical mutation paths with a present coverage configuration.

    Returns:
        The path bundle rooted at ``root``.

    """
    tools = root / "tools"
    tools.mkdir()
    (tools / "mutmut.coveragerc").write_text("[run]\n", encoding="utf-8")
    return mutation_task.mutation_paths(
        root,
        root / "build",
        root / "mutants" / "stats.json",
        root / "build" / "results.txt",
        root / "equivalents.json",
    )


if __name__ == "__main__":
    unittest.main(verbosity=2)
