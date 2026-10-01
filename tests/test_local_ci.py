"""Contracts keeping the local CI command in lockstep with the workflows."""

from __future__ import annotations

import io
import tempfile
import unittest
from contextlib import contextmanager, redirect_stdout
from dataclasses import replace
from pathlib import Path
from types import SimpleNamespace
from typing import TYPE_CHECKING, Final
from unittest.mock import MagicMock, call, patch

from tools import local_ci, tasks

from tests.local_ci_plan_support import EXPECTED_POSIX_PLAN, commands

if TYPE_CHECKING:
    from collections.abc import Iterator, Mapping, Sequence

PROJECT_ROOT: Final = Path(__file__).resolve().parents[1]
WORKFLOWS: Final = PROJECT_ROOT / ".github" / "workflows"
# Host paths render natively (``\\`` on Windows); container paths are always POSIX.
HOST_ROOT: Final = Path("/host-ci")
HOST_PROJECT: Final = Path("/project")


class PlanTests(unittest.TestCase):
    """Pin every local command, environment, and timeout."""

    def test_linux_plan_runs_every_reproducible_gate_in_cost_order(self) -> None:
        steps = _plan("linux")
        self.assertEqual(
            [(step.command, step.environment, step.timeout_seconds) for step in steps],
            commands(EXPECTED_POSIX_PLAN),
        )

    def test_windows_plan_omits_the_posix_only_steps_as_ci_does(self) -> None:
        steps = _plan("win32")
        expected = [
            entry
            for entry in commands(EXPECTED_POSIX_PLAN)
            if entry[0][:1] != ("test",) and "mutation" not in entry[0]
        ]
        self.assertEqual(
            [(step.command, step.environment, step.timeout_seconds) for step in steps],
            expected,
        )

    def test_other_posix_hosts_run_mutation_in_the_ci_runner_image(self) -> None:
        steps = _plan("darwin")
        linux = _plan("linux")
        mutation_index = next(
            index for index, step in enumerate(linux) if "mutation" in step.command
        )
        for native, portable in zip(
            steps[:mutation_index], linux[:mutation_index], strict=True
        ):
            expected = (
                portable.command[:-1]
                if native.mirrors in {local_ci.BUILD, local_ci.VERIFY}
                else portable.command
            )
            self.assertEqual(native, replace(portable, command=expected))
        self.assertEqual(steps[mutation_index + 2 :], linux[mutation_index + 1 :])
        build, campaign = steps[mutation_index : mutation_index + 2]
        self.assertEqual(
            (build.mirrors, build.environment, build.timeout_seconds),
            (local_ci.MUTATION, {}, 600),
        )
        self.assertEqual(
            build.command,
            (
                *("docker", "build", "--quiet"),
                *("--tag", "eml-attachment-remover-ci:uv-0.12.21"),
                str(HOST_ROOT / "image"),
            ),
        )
        self.assertEqual(
            (campaign.mirrors, campaign.environment, campaign.timeout_seconds),
            (local_ci.MUTATION, {}, 10_800),
        )
        self.assertEqual(
            campaign.command,
            (
                *("docker", "run", "--rm", "--init"),
                "--mount",
                (
                    "type=volume,source=eml-attachment-remover-ci-uv-cache,"
                    "target=/ci/.cache/uv"
                ),
                "--mount",
                f"type=bind,source={HOST_PROJECT},target=/src,readonly",
                "--mount",
                (
                    f"type=bind,source={HOST_PROJECT / 'build' / 'linux-mutation'},"
                    "target=/ci/work/build"
                ),
                *("--env", "PYTHONDEVMODE=1"),
                *("--env", "PYTHONDONTWRITEBYTECODE=1"),
                *("--env", "PYTHONNOUSERSITE=1"),
                *("--env", "PYTHONWARNINGS=error"),
                *("--env", "MUTATION_WORKERS=4"),
                *("--env", "UV_PROJECT_ENVIRONMENT=/ci/venv"),
                *("--env", "UV_PYTHON=3.14.7"),
                "eml-attachment-remover-ci:uv-0.12.21",
                *("/bin/sh", "-c"),
                (
                    "cd /src && tar --exclude=./.venv --exclude=./mutants "
                    "--exclude=./build --exclude=./.hypothesis -cf - . "
                    "| tar -xf - -C /ci/work && cd /ci/work "
                    "&& PYTHONWARNINGS=default uv sync --locked --group dev "
                    "--python 3.14.7 && exec uv run python tools/tasks.py mutation "
                    '--workers "$MUTATION_WORKERS"'
                ),
            ),
        )
        self.assertEqual(
            local_ci.LINUX_IMAGE_DEFINITION,
            "FROM ghcr.io/astral-sh/uv:0.12.21@sha256:"
            "a7aed3216253ee804de3e2d8afa5073baa1a177335345d43845cd4165e43b711 AS uv\n"
            "FROM ubuntu:24.04@sha256:"
            "008173c23f95b170204355c12626cb5a965d779a7e1283b09e9cffbb1bf33ca3\n"
            "RUN apt-get update && apt-get install -y --no-install-recommends "
            "ca-certificates && rm -rf /var/lib/apt/lists/* "
            "&& useradd --create-home --home-dir /ci --uid 1001 runner "
            "&& mkdir -p /ci/.cache/uv /ci/work "
            "&& chown -R runner:runner /ci\n"
            "COPY --from=uv /uv /usr/local/bin/uv\n"
            "USER runner\n"
            "WORKDIR /ci\n",
        )

    def test_project_tag_reads_the_static_version(self) -> None:
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            (root / "pyproject.toml").write_text(
                '[project]\nversion = "4.5.6"\n', encoding="utf-8"
            )
            self.assertEqual(local_ci.project_tag(root), "v4.5.6")

    def test_owner_grace_exceeds_the_leaf_shutdown_it_waits_for(self) -> None:
        self.assertEqual(local_ci.OWNER_GRACE_SECONDS, 30.0)
        self.assertGreater(
            local_ci.OWNER_GRACE_SECONDS,
            tasks.task_process.DEFAULT_GRACE_SECONDS,
        )


class RunTests(unittest.TestCase):
    """Report progress, stop at the first failure, and clean private state."""

    def test_steps_run_in_order_with_timed_headlines(self) -> None:
        steps = (
            local_ci.Step("one", ("first", "a b"), {"A": "1"}, 5),
            local_ci.Step("two", ("second",), {}, 6),
        )
        clock = iter((10.0, 11.25, 20.0, 23.0))
        calls: list[tuple[object, ...]] = []
        output = io.StringIO()
        with redirect_stdout(output):
            local_ci.run(
                steps,
                lambda *arguments: calls.append(arguments),
                clock=clock.__next__,
            )
        self.assertEqual(
            calls,
            [(("first", "a b"), {"A": "1"}, 5), (("second",), {}, 6)],
        )
        self.assertEqual(
            output.getvalue(),
            "==> [1/2] first 'a b'\n<== 1.2s\n==> [2/2] second\n<== 3.0s\n"
            f"{local_ci.CI_ONLY_NOTICE}\n",
        )

    def test_every_progress_line_is_flushed_before_child_output(self) -> None:
        # Children write straight to the inherited terminal, so buffered headlines
        # would appear after the output they introduce.
        stream = MagicMock()
        with redirect_stdout(stream):
            local_ci.run(
                (local_ci.Step("one", ("first",), {}, 5),),
                lambda *_arguments: None,
                clock=lambda: 0.0,
            )
        self.assertEqual(stream.flush.call_count, 3)

    def test_first_failure_stops_the_plan(self) -> None:
        steps = (
            local_ci.Step("one", ("first",), {}, 5),
            local_ci.Step("two", ("second",), {}, 6),
        )
        calls: list[tuple[str, ...]] = []

        def fail(command: Sequence[str], *_arguments: object) -> None:
            calls.append(tuple(command))
            raise RuntimeError(command[0])

        output = io.StringIO()
        with redirect_stdout(output), self.assertRaisesRegex(RuntimeError, "first"):
            local_ci.run(steps, fail, clock=lambda: 0.0)
        self.assertEqual(calls, [("first",)])
        self.assertEqual(output.getvalue(), "==> [1/2] first\n")

    def test_local_ci_uses_a_private_removed_root_and_the_project_tag(self) -> None:
        commands: list[tuple[str, ...]] = []
        roots: list[Path] = []

        def record(
            command: Sequence[str], environment: Mapping[str, str], _timeout: float
        ) -> None:
            commands.append(tuple(command))
            roots.append(Path(environment["UV_PROJECT_ENVIRONMENT"]).parent)

        with (
            tempfile.TemporaryDirectory() as directory,
            patch.object(local_ci, "sys", SimpleNamespace(platform="win32")),
            patch.object(local_ci, "os", SimpleNamespace(devnull="nul")),
            redirect_stdout(io.StringIO()),
        ):
            project = _project(Path(directory))
            local_ci.run_local_ci(project, record, release_tag=None, workers="auto")
            self.assertTrue((project / "build" / "linux-mutation").is_dir())
        self.assertEqual(len(set(roots)), 1)
        root = roots[0]
        self.assertTrue(root.name.startswith(local_ci.TEMPORARY_PREFIX))
        self.assertFalse(root.exists())
        self.assertIn(
            ("uv", "run", "python", "tools/check_release_tag.py", "v4.5.6"), commands
        )
        self.assertNotIn(("test", "-x", "/bin/sh"), commands)
        self.assertNotIn("mutation", {word for command in commands for word in command})
        self.assertIn(
            (
                *("uv", "run", "mypy", "--no-incremental", "--cache-dir", "nul"),
                *("--platform", "win32"),
            ),
            commands,
        )

    def test_private_root_is_resolved_like_ci_runner_paths(self) -> None:
        roots: list[Path] = []

        def record(
            _command: Sequence[str], environment: Mapping[str, str], _timeout: float
        ) -> None:
            roots.append(Path(environment["UV_PROJECT_ENVIRONMENT"]).parent)

        with tempfile.TemporaryDirectory() as directory:
            real = Path(directory).resolve() / "real"
            real.mkdir()
            alias = Path(directory).resolve() / "alias"
            alias.symlink_to(real, target_is_directory=True)

            @contextmanager
            def aliased(*_arguments: object, **_options: object) -> Iterator[str]:
                yield str(alias)

            with (
                patch("tools.local_ci.tempfile.TemporaryDirectory", aliased),
                patch.object(local_ci, "sys", SimpleNamespace(platform="win32")),
                redirect_stdout(io.StringIO()),
            ):
                local_ci.run_local_ci(
                    _project(Path(directory)), record, release_tag="v1", workers="1"
                )
        self.assertEqual(set(roots), {real})

    def test_container_hosts_get_the_image_definition_and_explicit_options(
        self,
    ) -> None:
        commands: list[tuple[str, ...]] = []
        definitions: list[str] = []

        def record(command: Sequence[str], *_rest: object) -> None:
            commands.append(tuple(command))
            if command[:2] == ("docker", "build"):
                image = Path(command[-1]) / "Dockerfile"
                definitions.append(image.read_text(encoding="utf-8"))

        with (
            tempfile.TemporaryDirectory() as directory,
            patch.object(local_ci, "sys", SimpleNamespace(platform="darwin")),
            redirect_stdout(io.StringIO()),
        ):
            project = _project(Path(directory))
            (project / "build" / "linux-mutation").mkdir(parents=True)
            local_ci.run_local_ci(project, record, release_tag="v9.9.9", workers="3")
        self.assertEqual(definitions, [local_ci.LINUX_IMAGE_DEFINITION])
        self.assertIn(
            ("uv", "run", "python", "tools/check_release_tag.py", "v9.9.9"), commands
        )
        self.assertIn(
            "MUTATION_WORKERS=3", {w for command in commands for w in command}
        )
        self.assertIn(("test", "-x", "/bin/sh"), commands)


def _project(root: Path) -> Path:
    (root / "pyproject.toml").write_text(
        '[project]\nversion = "4.5.6"\n', encoding="utf-8"
    )
    return root


def _plan(host: str) -> tuple[local_ci.Step, ...]:
    return local_ci.plan(
        HOST_ROOT,
        host=host,
        project_root=HOST_PROJECT,
        release_tag="v1.2.3",
        workers="4",
    )


class TaskIntegrationTests(unittest.TestCase):
    """Run each planned step as an owned process group with strict settings."""

    def test_ci_task_runs_steps_with_owner_grace_and_task_environment(self) -> None:
        with (
            patch.object(tasks.local_ci, "run_local_ci") as run_local_ci,
            patch.object(tasks.task_process, "run") as run,
            patch.object(
                tasks, "_task_environment", return_value={"STRICT": "1"}
            ) as environment,
        ):
            tasks._ci("v1.0.0", None)  # ruff: ignore[private-member-access] - task contract.
            tasks._ci(None, 3)  # ruff: ignore[private-member-access] - task contract.
            run_step = run_local_ci.call_args_list[0].args[1]
            run_step(("uv", "sync"), {"UV_PYTHON": "3.14.7"}, 600)
        self.assertEqual(
            [entry.args[0] for entry in run_local_ci.call_args_list],
            [tasks.PROJECT_ROOT, tasks.PROJECT_ROOT],
        )
        self.assertEqual(
            [
                {key: value for key, value in entry.kwargs.items() if key != "lease"}
                for entry in run_local_ci.call_args_list
            ],
            [
                {"release_tag": "v1.0.0", "workers": "auto"},
                {"release_tag": None, "workers": "3"},
            ],
        )
        lease = run_local_ci.call_args_list[0].kwargs["lease"]
        with patch.object(tasks.mutation_lease, "lease") as leased:
            lease()
        leased.assert_called_once_with(tasks.BUILD_DIRECTORY)
        environment.assert_called_once_with(environment_updates={"UV_PYTHON": "3.14.7"})
        self.assertEqual(
            run.call_args_list,
            [
                call(
                    ("uv", "sync"),
                    cwd=tasks.PROJECT_ROOT,
                    env={"STRICT": "1"},
                    timeout=600,
                    grace_seconds=30.0,
                )
            ],
        )


if __name__ == "__main__":
    unittest.main(verbosity=2)
