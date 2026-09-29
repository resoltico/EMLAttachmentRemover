"""The platform-independent static checks run once; every lane keeps its own audit."""

from __future__ import annotations

import re
from pathlib import Path
from typing import Final
from unittest.mock import patch

from tools import tasks

WORKFLOW: Final = (
    Path(__file__).resolve().parents[1] / ".github" / "workflows" / "quality.yml"
)


def _calls(*, native: bool) -> list[str]:
    calls: list[str] = []
    with (
        patch.object(tasks, "_hygiene", lambda: calls.append("hygiene")),
        patch.object(tasks, "_static", lambda: calls.append("static")),
        patch.object(tasks, "_coverage", lambda: calls.append("coverage")),
    ):
        tasks._quality(native=native)  # ruff: ignore[private-member-access] - task contract.
    return calls


def test_the_full_quality_gate_audits_checks_and_measures_coverage() -> None:
    """Without the flag, every check runs, in the historic order."""
    assert _calls(native=False) == ["hygiene", "static", "coverage"]


def test_the_native_quality_gate_keeps_only_host_dependent_work() -> None:
    """The audit reads this checkout and coverage runs its tests; the rest is shared."""
    assert _calls(native=True) == ["hygiene", "coverage"]


def test_check_is_the_audit_then_the_static_checks() -> None:
    """The standalone ``check`` task still runs everything."""
    calls: list[str] = []
    with (
        patch.object(tasks, "_hygiene", lambda: calls.append("hygiene")),
        patch.object(tasks, "_static", lambda: calls.append("static")),
    ):
        tasks._check()  # ruff: ignore[private-member-access] - task contract.
    assert calls == ["hygiene", "static"]


def test_the_quality_command_line_selects_the_native_gate() -> None:
    """``quality --native`` reaches the task; the plain form does not set it."""
    seen: list[bool] = []
    with patch.object(tasks, "_quality", lambda *, native=False: seen.append(native)):
        assert tasks.main(["quality", "--native"]) == 0
        assert tasks.main(["quality"]) == 0
    assert seen == [True, False]


def test_static_checks_are_the_platform_independent_ones() -> None:
    """The repository audit is not among them: it depends on the host checkout."""
    commands: list[tuple[str, ...]] = []
    with (
        patch.object(tasks, "_run", lambda command, **_: commands.append(command)),
        patch.object(
            tasks.check_repository_hygiene,
            "audit_repository",
            lambda _root: type("Audit", (), {"issues": (), "public_files": ()})(),
        ),
    ):
        tasks._static()  # ruff: ignore[private-member-access] - task contract.
    names = [command[0] for command in commands]
    assert "ruff" in names
    assert "mypy" in names
    assert "detect-secrets-hook" in names
    assert all("check_repository_hygiene" not in " ".join(c) for c in commands)


def test_the_workflow_runs_the_static_checks_on_exactly_one_lane() -> None:
    """One matrix lane is marked static; all six lanes still exist and report."""
    text = WORKFLOW.read_text(encoding="utf-8")
    assert len(re.findall(r"^\s+static: true$", text, flags=re.MULTILINE)) == 1
    assert len(re.findall(r"^\s+- os: ", text, flags=re.MULTILINE)) == 6
    assert re.search(
        r"if: matrix\.static\n\s+run: uv run python tools/tasks\.py quality\n", text
    )
    assert re.search(
        r"if: \$\{\{ !matrix\.static \}\}\n"
        r"\s+run: uv run python tools/tasks\.py quality --native\n",
        text,
    )
    assert re.search(
        r"name: \$\{\{ matrix\.os \}\} / CPython \$\{\{ matrix\.python \}\}", text
    )
