"""Run portable local development and release-preparation tasks."""

from __future__ import annotations

import argparse
import importlib
import os
import subprocess
import sys
from pathlib import Path
from typing import TYPE_CHECKING, Final, cast

if TYPE_CHECKING:
    from collections.abc import Callable, Mapping, Sequence

    from tools.task_interfaces import (
        HypothesisRunner,
        MutationTaskModule,
        RepositoryHygiene,
        TaskTestCommands,
        TaskTimeout,
    )


# The launcher imports repository modules before it can construct child environments.
sys.dont_write_bytecode = True


check_repository_hygiene = cast(
    "RepositoryHygiene",
    importlib.import_module(
        "tools.check_repository_hygiene" if __package__ else "check_repository_hygiene",
    ),
)
mutation_task = cast(
    "MutationTaskModule",
    importlib.import_module(
        "tools.mutation_task" if __package__ else "mutation_task",
    ),
)
hypothesis_runner = cast(
    "HypothesisRunner",
    importlib.import_module(
        "tools.hypothesis_run" if __package__ else "hypothesis_run",
    ),
)
task_timeout = cast(
    "TaskTimeout",
    importlib.import_module("tools.task_timeout" if __package__ else "task_timeout"),
)
task_test_commands = cast(
    "TaskTestCommands",
    importlib.import_module(
        "tools.task_test_commands" if __package__ else "task_test_commands"
    ),
)

PROJECT_ROOT: Final = Path(__file__).resolve().parents[1]
BUILD_TARGET: Final = PROJECT_ROOT / "build" / "remove-eml-attachments.pyz"
BUILD_DIRECTORY: Final = PROJECT_ROOT / "build"
RELEASE_DIRECTORY: Final = PROJECT_ROOT / "release-dist"
MUTATION_STATISTICS: Final = PROJECT_ROOT / "mutants" / "mutmut-cicd-stats.json"
MUTATION_RESULTS: Final = BUILD_DIRECTORY / "mutmut-results.txt"
MUTATION_EQUIVALENTS: Final = PROJECT_ROOT / "tools" / "equivalent_mutants.json"
DEVELOPMENT_TEST_TIMEOUT_SECONDS: Final = 600
THOROUGH_TEST_TIMEOUT_SECONDS: Final = 1_800
OBSERVABILITY_VARIABLES: Final = (
    "HYPOTHESIS_EXPERIMENTAL_OBSERVABILITY",
    "HYPOTHESIS_EXPERIMENTAL_OBSERVABILITY_NOCOVER",
)
STRICT_ENVIRONMENT_REMOVALS: Final = (
    hypothesis_runner.STORAGE_ENVIRONMENT_VARIABLE,
    "PYTHONPATH",
)
SHELL_SCRIPTS: Final = tuple(
    str(path)
    for path in sorted((PROJECT_ROOT / "integrations" / "macos-shortcuts").glob("*.sh"))
)
WORKFLOW_FILES: Final = tuple(
    str(path) for path in sorted((PROJECT_ROOT / ".github" / "workflows").glob("*.yml"))
)


def _task_environment(
    *,
    profile: str | None = None,
    environment_updates: Mapping[str, str] | None = None,
    environment_removals: Sequence[str] = (),
) -> dict[str, str]:
    """Return the canonical strict task subprocess environment.

    Returns:
        A fresh child-process environment.

    """
    environment = os.environ.copy()
    environment["PYTHONDEVMODE"] = "1"
    environment["PYTHONDONTWRITEBYTECODE"] = "1"
    environment["PYTHONNOUSERSITE"] = "1"
    environment["PYTHONWARNINGS"] = "error"
    for variable in STRICT_ENVIRONMENT_REMOVALS:
        environment.pop(variable, None)
    for variable in environment_removals:
        environment.pop(variable, None)
    if environment_updates is not None:
        environment.update(environment_updates)
    if profile is not None:
        environment["HYPOTHESIS_PROFILE"] = profile
    return environment


def _run(
    command: Sequence[str],
    *,
    profile: str | None = None,
    timeout_seconds: float | None = 600,
    environment_updates: Mapping[str, str] | None = None,
    environment_removals: Sequence[str] = (),
) -> None:
    """Run one task command without a shell."""
    subprocess.run(
        command,
        check=True,
        cwd=PROJECT_ROOT,
        env=_task_environment(
            profile=profile,
            environment_updates=environment_updates,
            environment_removals=environment_removals,
        ),
        timeout=timeout_seconds,
    )


def _isolated_run(
    storage: Path,
    command: Sequence[str],
    *,
    profile: str | None = None,
    timeout_seconds: float | None = 600,
    environment_updates: Mapping[str, str] | None = None,
) -> None:
    """Run one command with an exact private Hypothesis storage directory."""
    updates = dict(environment_updates or {})
    updates[hypothesis_runner.STORAGE_ENVIRONMENT_VARIABLE] = str(storage)
    _run(
        command,
        profile=profile,
        timeout_seconds=timeout_seconds,
        environment_updates=updates,
        environment_removals=OBSERVABILITY_VARIABLES,
    )


def _check() -> None:
    """Run formatting, linting, type, and module-design checks.

    Raises:
        RuntimeError: If the fresh pre-secret-scan repository audit is dirty.

    """
    _run((sys.executable, "tools/check_repository_hygiene.py"))
    _run(("ruff", "format", "--check", "--no-cache", "."))
    _run(("ruff", "check", "--no-cache", "."))
    _run(("mypy", "--no-incremental", "--cache-dir", os.devnull))
    _run((sys.executable, "tools/check_module_design.py"))
    _run(
        (
            "actionlint",
            "-shellcheck",
            "shellcheck",
            "-pyflakes",
            "pyflakes",
            *WORKFLOW_FILES,
        ),
    )
    _run(("shellcheck", "--shell=sh", *SHELL_SCRIPTS))
    audit = check_repository_hygiene.audit_repository(PROJECT_ROOT)
    if audit.issues:
        diagnostics = "\n".join(audit.diagnostics(PROJECT_ROOT))
        message = f"repository changed before secret scanning:\n{diagnostics}"
        raise RuntimeError(message)
    _run((
        "detect-secrets-hook",
        "--no-verify",
        *(str(path) for path in audit.public_files),
    ))


def _test_result_path(profile: str) -> Path:
    """Return the machine-readable test-report path for one profile.

    Returns:
        A project-local ignored JUnit XML path.

    """
    return BUILD_DIRECTORY / f"test-results-{profile}.xml"


def _test(
    profile: str,
    *,
    timeout_seconds: float | None = None,
    observable: bool = False,
) -> None:
    """Run the complete test suite under one Hypothesis profile."""
    BUILD_DIRECTORY.mkdir(exist_ok=True)
    if timeout_seconds is None:
        timeout_seconds = (
            THOROUGH_TEST_TIMEOUT_SECONDS
            if profile == "project-thorough"
            else DEVELOPMENT_TEST_TIMEOUT_SECONDS
        )
    observability_environment = (
        {OBSERVABILITY_VARIABLES[0]: "1"} if observable else None
    )
    hypothesis_runner.run_isolated(
        lambda storage: _isolated_run(
            storage,
            task_test_commands.pytest_command(
                sys.executable,
                storage.parent / "test-results.xml",
                coverage=False,
            ),
            profile=profile,
            timeout_seconds=timeout_seconds,
            environment_updates=observability_environment,
        ),
        PROJECT_ROOT,
        observations=observable,
        report_destination=_test_result_path(profile),
    )


def _coverage() -> None:
    """Run the branch-coverage gate using the deterministic CI profile."""
    BUILD_DIRECTORY.mkdir(exist_ok=True)

    def coverage_action(storage: Path) -> None:
        coverage_data = storage.parent / ".coverage"
        coverage_environment = {"COVERAGE_FILE": str(coverage_data)}
        _isolated_run(
            storage,
            (sys.executable, "-m", "coverage", "erase"),
            environment_updates=coverage_environment,
        )
        _isolated_run(
            storage,
            task_test_commands.pytest_command(
                sys.executable,
                storage.parent / "test-results.xml",
                coverage=True,
            ),
            profile="project-ci",
            environment_updates=coverage_environment,
        )
        _isolated_run(
            storage,
            (sys.executable, "-m", "coverage", "combine"),
            environment_updates=coverage_environment,
        )
        _isolated_run(
            storage,
            (
                sys.executable,
                "tools/report_coverage.py",
                "--data-file",
                str(coverage_data),
                "--xml-output",
                str(BUILD_DIRECTORY / "coverage.xml"),
            ),
            environment_updates=coverage_environment,
        )

    hypothesis_runner.run_isolated(
        coverage_action,
        PROJECT_ROOT,
        report_destination=_test_result_path("project-ci"),
    )


def _capture_mutation_results() -> None:
    """Capture every named mutant and status in deterministic lexical order."""
    mutation_task.capture_results(
        _mutation_paths(),
        sys.executable,
        _task_environment(),
    )


def _mutation_paths() -> object:
    """Return the canonical mutation workspace and evidence paths.

    Returns:
        An opaque mutation path bundle.

    """
    return mutation_task.mutation_paths(
        PROJECT_ROOT,
        BUILD_DIRECTORY,
        MUTATION_STATISTICS,
        MUTATION_RESULTS,
        MUTATION_EQUIVALENTS,
    )


def _remove_mutation_workspace() -> None:
    """Remove only the canonical generated Mutmut workspace."""
    mutation_task.remove_workspace(_mutation_paths())


def _mutation() -> None:
    """Establish coverage and require 100% of actionable mutants to be killed.

    Raises:
        RuntimeError: If invoked on native Windows without process-fork support.

    """
    if sys.platform == "win32":
        message = (
            "mutation testing requires process fork support; run this task under "
            "WSL or on Linux/macOS"
        )
        raise RuntimeError(message)

    def mutation_action(storage: Path) -> None:
        coverage_variables = tuple(
            variable for variable in os.environ if variable.startswith("COVERAGE_")
        )

        def run_mutation_command(
            command: Sequence[str],
            *,
            profile: str | None = None,
            timeout_seconds: float | None = 600,
            environment_updates: Mapping[str, str] | None = None,
            environment_removals: Sequence[str] = (),
        ) -> None:
            updates = dict(environment_updates or {})
            updates["COVERAGE_FILE"] = str(storage.parent / ".mutmut-coverage")
            updates[hypothesis_runner.STORAGE_ENVIRONMENT_VARIABLE] = str(storage)
            _run(
                command,
                profile=profile,
                timeout_seconds=timeout_seconds,
                environment_updates=updates,
                environment_removals=(
                    *OBSERVABILITY_VARIABLES,
                    *coverage_variables,
                    *environment_removals,
                ),
            )

        mutation_task.run_mutation(
            _mutation_paths(),
            mutation_task.mutation_actions(
                _coverage,
                _remove_mutation_workspace,
                _capture_mutation_results,
                run_mutation_command,
            ),
            sys.executable,
        )

    hypothesis_runner.run_isolated(mutation_action, PROJECT_ROOT)


def _build_zipapp() -> None:
    """Build and verify an explicitly non-release local zipapp."""
    _run((sys.executable, "tools/build_zipapp.py", "--target", str(BUILD_TARGET)))


def _qualify_release() -> None:
    """Build and qualify the complete explicitly local release-candidate set."""
    _run((
        sys.executable,
        "tools/qualify_release.py",
        "--output-directory",
        str(RELEASE_DIRECTORY),
    ))


def _quality() -> None:
    """Run the complete pull-request quality gate."""
    _check()
    _coverage()


_positive_timeout = task_timeout.positive_timeout


def main(argv: list[str] | None = None) -> int:
    """Dispatch one documented local task.

    Returns:
        Zero after the selected task succeeds.

    """
    parser = argparse.ArgumentParser(description=__doc__)
    command_parsers = parser.add_subparsers(dest="task", required=True)
    for task_name in (
        "build",
        "check",
        "coverage",
        "mutation",
        "quality",
        "release",
        "test",
    ):
        command_parsers.add_parser(task_name)
    thorough_parser = command_parsers.add_parser("thorough")
    thorough_parser.add_argument(
        "--timeout-seconds",
        type=_positive_timeout,
        help="whole pytest timeout",
    )
    thorough_parser.add_argument(
        "--observable",
        action="store_true",
        help="write public Hypothesis observations",
    )
    arguments = parser.parse_args(argv)
    task = arguments.task
    actions: dict[str, Callable[[], None]] = {
        "build": _build_zipapp,
        "check": _check,
        "coverage": _coverage,
        "mutation": _mutation,
        "quality": _quality,
        "release": _qualify_release,
        "test": lambda: _test("project-development"),
        "thorough": lambda: _test(
            "project-thorough",
            timeout_seconds=arguments.timeout_seconds,
            observable=arguments.observable,
        ),
    }
    actions[task]()
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
