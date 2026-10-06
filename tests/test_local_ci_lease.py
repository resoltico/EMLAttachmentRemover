"""Only the container campaign runs under the checkout's mutation lease."""

from __future__ import annotations

import io
import tempfile
import unittest
from contextlib import contextmanager, redirect_stdout
from pathlib import Path
from types import SimpleNamespace
from typing import TYPE_CHECKING
from unittest.mock import MagicMock, patch

from tools import local_ci

if TYPE_CHECKING:
    from collections.abc import Generator


HOST_ROOT = Path("/host-ci")
HOST_PROJECT = Path("/project")


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


class LeaseTests(unittest.TestCase):
    """Step leasing, the plan's leased steps, and lease hand-off."""

    def test_only_a_leased_step_runs_under_the_checkout_lease(self) -> None:
        events: list[str] = []

        @contextmanager
        def lease() -> Generator[None]:
            events.append("acquire")
            try:
                yield
            finally:
                events.append("release")

        steps = (
            local_ci.Step("one", ("plain",), {}, 5),
            local_ci.Step("two", ("guarded",), {}, 6, leased=True),
        )
        with redirect_stdout(io.StringIO()):
            local_ci.run(
                steps,
                lambda command, *_arguments: events.append(command[0]),
                clock=lambda: 0.0,
                lease=lease,
            )
        self.assertEqual(events, ["plain", "acquire", "guarded", "release"])

    def test_the_container_campaign_is_the_only_leased_planned_step(self) -> None:
        for host in ("linux", "darwin", "win32"):
            with self.subTest(host=host):
                leased = [step for step in _plan(host) if step.leased]
                self.assertEqual(
                    [step.command[:2] for step in leased],
                    [("docker", "run")] if host == "darwin" else [],
                )

    def test_run_local_ci_passes_its_lease_to_the_run(self) -> None:
        marker = MagicMock()
        with (
            tempfile.TemporaryDirectory() as directory,
            patch.object(local_ci, "run") as run,
            patch.object(local_ci, "sys", SimpleNamespace(platform="win32")),
            patch.object(local_ci, "os", SimpleNamespace(devnull="nul")),
        ):
            local_ci.run_local_ci(
                _project(Path(directory)),
                lambda *_arguments: None,
                release_tag=None,
                workers="1",
                lease=marker,
            )
        self.assertIs(run.call_args.kwargs["lease"], marker)
