"""Orchestrate mutation execution, evidence capture, and safe cleanup."""

from __future__ import annotations

import shutil
import subprocess
from dataclasses import dataclass
from typing import TYPE_CHECKING, Final

if TYPE_CHECKING:
    from collections.abc import Callable, Mapping, Sequence
    from pathlib import Path
    from typing import Protocol

    class CommandRunner(Protocol):
        """Describe the task runner used for mutation subprocesses."""

        def __call__(
            self,
            command: Sequence[str],
            *,
            profile: str | None = None,
            timeout_seconds: float | None = 600,
            environment_updates: Mapping[str, str] | None = None,
            environment_removals: Sequence[str] = (),
        ) -> None:
            """Run one bounded task command."""


UTF8: Final = "utf-8"
MUTATION_PROFILE: Final = "project-mutation"
MUTATION_TIMEOUT_SECONDS: Final = 7_200
EVIDENCE_TIMEOUT_SECONDS: Final = 120
FAILURE_GROUP_MESSAGE: Final = "mutation gate failed"


@dataclass(frozen=True, slots=True)
class MutationPaths:
    """Contain canonical mutation workspace and evidence paths."""

    project_root: Path
    build_directory: Path
    statistics: Path
    results: Path
    equivalents: Path
    coverage_config: Path


@dataclass(frozen=True, slots=True)
class MutationActions:
    """Contain injected project-task operations used by mutation orchestration."""

    coverage: Callable[[], None]
    cleanup: Callable[[], None]
    capture: Callable[[], None]
    run: CommandRunner


def mutation_paths(
    project_root: Path,
    build_directory: Path,
    statistics: Path,
    results: Path,
    equivalents: Path,
) -> MutationPaths:
    """Create a canonical mutation-path bundle.

    Returns:
        The immutable path bundle.

    """
    return MutationPaths(
        project_root,
        build_directory,
        statistics,
        results,
        equivalents,
        project_root / "tools" / "mutmut.coveragerc",
    )


def mutation_actions(
    coverage: Callable[[], None],
    cleanup: Callable[[], None],
    capture: Callable[[], None],
    run: CommandRunner,
) -> MutationActions:
    """Create an injected mutation-action bundle.

    Returns:
        The immutable action bundle.

    """
    return MutationActions(coverage, cleanup, capture, run)


def capture_results(
    paths: MutationPaths,
    executable: str,
    environment: Mapping[str, str],
) -> None:
    """Capture every named mutant and status in deterministic lexical order."""
    completed = subprocess.run(
        (executable, "-m", "mutmut", "results", "--all", "true"),
        check=True,
        cwd=paths.project_root,
        env=environment,
        timeout=EVIDENCE_TIMEOUT_SECONDS,
        capture_output=True,
        encoding=UTF8,
    )
    lines = sorted(
        line.strip() for line in completed.stdout.splitlines() if line.strip()
    )
    paths.results.write_text(
        "".join(f"{line}\n" for line in lines),
        encoding=UTF8,
    )


def remove_workspace(paths: MutationPaths) -> None:
    """Remove only the canonical generated Mutmut workspace.

    Raises:
        RuntimeError: If the configured statistics path is not canonical.

    """
    expected = paths.project_root / "mutants"
    if paths.statistics.parent != expected:
        message = f"refusing to remove noncanonical mutation workspace: {expected}"
        raise RuntimeError(message)
    if expected.is_symlink() or (expected.exists() and not expected.is_dir()):
        message = f"refusing to remove unsafe mutation workspace: {expected}"
        raise RuntimeError(message)
    if expected.exists():
        shutil.rmtree(expected)
    if expected.exists() or expected.is_symlink():
        message = f"mutation workspace removal was incomplete: {expected}"
        raise RuntimeError(message)


def _collect_failure(
    label: str,
    action: Callable[[], None],
    failures: list[Exception],
) -> None:
    """Run one evidence step and retain an actionable operational failure."""
    try:
        action()
    except (OSError, subprocess.SubprocessError) as error:
        failures.append(RuntimeError(f"{label} failed: {error}"))


def run_mutation(
    paths: MutationPaths, actions: MutationActions, executable: str
) -> None:
    """Establish coverage and require 100% of actionable mutants to be killed.

    Raises:
        RuntimeError: If the dedicated measurement configuration is unsafe.
        ExceptionGroup: If one or more mutation or evidence steps fail.

    """
    if paths.coverage_config.is_symlink() or not paths.coverage_config.is_file():
        message = f"mutation coverage configuration is unsafe: {paths.coverage_config}"
        raise RuntimeError(message)
    paths.build_directory.mkdir(exist_ok=True)
    actions.cleanup()
    paths.results.unlink(missing_ok=True)
    actions.coverage()
    failures: list[Exception] = []
    operations: tuple[tuple[str, Callable[[], None]], ...] = (
        (
            "run mutants",
            lambda: actions.run(
                ("mutmut", "run"),
                profile=MUTATION_PROFILE,
                timeout_seconds=MUTATION_TIMEOUT_SECONDS,
                environment_updates={
                    "COVERAGE_RCFILE": str(paths.coverage_config),
                },
            ),
        ),
        ("capture named results", actions.capture),
        (
            "export statistics",
            lambda: actions.run(
                ("mutmut", "export-cicd-stats"),
                timeout_seconds=EVIDENCE_TIMEOUT_SECONDS,
            ),
        ),
        (
            "validate statistics",
            lambda: actions.run(
                (
                    executable,
                    "tools/check_mutation_results.py",
                    str(paths.statistics),
                    "--results",
                    str(paths.results),
                    "--equivalents",
                    str(paths.equivalents),
                ),
                timeout_seconds=EVIDENCE_TIMEOUT_SECONDS,
            ),
        ),
    )
    for label, action in operations:
        _collect_failure(label, action, failures)
    if failures:
        raise ExceptionGroup(FAILURE_GROUP_MESSAGE, failures)
