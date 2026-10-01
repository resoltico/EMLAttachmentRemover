"""Run portable local development and release-preparation tasks."""

from __future__ import annotations

import importlib
import os
import sys
import tempfile
from pathlib import Path
from typing import TYPE_CHECKING, Final, cast

if TYPE_CHECKING:
    from collections.abc import Callable, Mapping, Sequence

    from tools.task_interfaces import (
        CoverageTask,
        HypothesisRunner,
        LocalCi,
        MutationDiagnostics,
        MutationLease,
        MutationTaskModule,
        RepositoryHygiene,
        StaticChecks,
        TaskCli,
        TaskProcess,
        TaskTestCommands,
        TaskTimeout,
    )


# The launcher imports repository modules before it can construct child environments.
sys.dont_write_bytecode = True


def _tool(name: str) -> object:
    """Import one repository tool as a package module or a script sibling.

    Returns:
        The imported module.

    """
    return importlib.import_module(f"tools.{name}" if __package__ else name)


check_repository_hygiene = cast("RepositoryHygiene", _tool("check_repository_hygiene"))
mutation_task = cast("MutationTaskModule", _tool("mutation_task"))
static_checks = cast("StaticChecks", _tool("static_checks"))
mutation_lease = cast("MutationLease", _tool("mutation_lease"))
mutation_diagnostics = cast("MutationDiagnostics", _tool("mutation_diagnostics"))
hypothesis_runner = cast("HypothesisRunner", _tool("hypothesis_run"))
task_timeout = cast("TaskTimeout", _tool("task_timeout"))
task_test_commands = cast("TaskTestCommands", _tool("task_test_commands"))
task_process = cast("TaskProcess", _tool("task_process"))
coverage_task = cast("CoverageTask", _tool("coverage_task"))
local_ci = cast("LocalCi", _tool("local_ci"))
task_cli = cast("TaskCli", _tool("task_cli"))

PROJECT_ROOT: Final = Path(__file__).resolve().parents[1]
BUILD_TARGET: Final = PROJECT_ROOT / "build" / "remove-eml-attachments.pyz"
BUILD_DIRECTORY: Final = PROJECT_ROOT / "build"
MUTATION_STATISTICS: Final = PROJECT_ROOT / "mutants" / "mutmut-cicd-stats.json"
MUTATION_RESULTS: Final = BUILD_DIRECTORY / "mutmut-results.txt"
MUTATION_EQUIVALENTS: Final = PROJECT_ROOT / "tools" / "equivalent_mutants.json"
MUTATION_DIAGNOSTICS: Final = BUILD_DIRECTORY / "mutation-diagnostics"
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
    """Run one task command without a shell, owning its descendants."""
    task_process.run(
        command,
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


def _hygiene() -> None:
    """Audit the checkout itself: paths, line endings, links, and generated files."""
    _run((sys.executable, "tools/check_repository_hygiene.py"))


def _static() -> None:
    """Run the platform-independent formatting, lint, type, and secret checks."""
    static_checks.run(
        _run,
        check_repository_hygiene,
        PROJECT_ROOT,
        WORKFLOW_FILES,
        SHELL_SCRIPTS,
    )


def _check() -> None:
    """Run every static check, including the host-dependent repository audit."""
    _hygiene()
    _static()


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
    if observable:
        _run((sys.executable, "tools/check_v301_property_observations.py"))


def _coverage() -> None:
    """Run the branch-coverage gate and report its data even after test failures."""
    BUILD_DIRECTORY.mkdir(exist_ok=True)

    def coverage_action(storage: Path) -> None:
        coverage_data = storage.parent / ".coverage"
        coverage_environment = {"COVERAGE_FILE": str(coverage_data)}

        def run(command: Sequence[str], profile: str | None = None) -> None:
            _isolated_run(
                storage,
                command,
                profile=profile,
                environment_updates=coverage_environment,
            )

        def report() -> None:
            run((sys.executable, "-m", "coverage", "combine"))
            run((
                sys.executable,
                "tools/report_coverage.py",
                "--data-file",
                str(coverage_data),
                "--xml-output",
                str(BUILD_DIRECTORY / "coverage.xml"),
            ))

        run((sys.executable, "-m", "coverage", "erase"))
        coverage_task.measure(
            lambda: run(
                task_test_commands.pytest_command(
                    sys.executable,
                    storage.parent / "test-results.xml",
                    coverage=True,
                ),
                profile="project-ci",
            ),
            report,
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


def _capture_mutation_diagnostics() -> None:
    """Bundle a bounded patch and mapped tests for every mutant that survived."""
    paths = _mutation_paths()
    environment = _task_environment()
    mutation_diagnostics.collect(
        MUTATION_RESULTS,
        PROJECT_ROOT / "mutants" / "mutmut-stats.json",
        MUTATION_DIAGNOSTICS,
        lambda mutant: mutation_task.show_mutant(
            paths, sys.executable, environment, mutant
        ),
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


def _mutation(*, workers: int | None = None, preflight: bool = True) -> None:
    """Establish coverage and require 100% of actionable mutants to be killed.

    Raises:
        RuntimeError: If invoked on native Windows without process-fork support.

    """
    runtime_platform = sys.platform
    if runtime_platform == "win32":
        message = (
            "mutation testing requires process fork support; run this task under "
            "WSL or on Linux/macOS"
        )
        raise RuntimeError(message)

    def mutation_action(storage: Path) -> None:
        run_mutation_command = mutation_task.command_runner(
            _run,
            storage,
            hypothesis_runner.STORAGE_ENVIRONMENT_VARIABLE,
            OBSERVABILITY_VARIABLES,
        )

        mutation_task.run_mutation(
            _mutation_paths(),
            mutation_task.mutation_actions(
                _coverage,
                _remove_mutation_workspace,
                _capture_mutation_results,
                run_mutation_command,
                _capture_mutation_diagnostics,
            ),
            sys.executable,
            workers=workers,
            preflight=preflight,
        )

    with mutation_lease.lease(BUILD_DIRECTORY):
        hypothesis_runner.run_isolated(mutation_action, PROJECT_ROOT)


def _build_zipapp() -> None:
    """Build and verify an explicitly non-release local zipapp."""
    _run((sys.executable, "tools/build_zipapp.py", "--target", str(BUILD_TARGET)))


def _qualify_release() -> None:
    """Build and qualify release candidates outside the repository checkout."""
    output_directory = Path(tempfile.mkdtemp(prefix="eml-attachment-remover-release-"))
    _run((
        sys.executable,
        "-B",
        "-m",
        "tools.release_delivery",
        "--output-directory",
        str(output_directory),
    ))
    print(f"qualified release candidates: {output_directory}")


def _quality(*, native: bool = False) -> None:
    """Run the complete pull-request quality gate.

    With ``native``, only what depends on this host runs: the repository audit and
    coverage. One shared lane owns static checks, including all mypy platforms.
    """
    if sys.platform == "darwin":
        _run(
            ("/bin/sh", str(PROJECT_ROOT / "integrations/macos-ui/quality.sh")),
            environment_updates={"EML_REMOVER_PYTHON": sys.executable},
        )
    if native:
        _hygiene()
    else:
        _check()
    _coverage()


def _ci(release_tag: str | None, workers: int | None) -> None:
    """Run every CI gate this host can reproduce, in owned process groups."""
    local_ci.run_local_ci(
        PROJECT_ROOT,
        lambda command, environment, timeout: task_process.run(
            command,
            cwd=PROJECT_ROOT,
            env=_task_environment(environment_updates=environment),
            timeout=timeout,
            grace_seconds=local_ci.OWNER_GRACE_SECONDS,
        ),
        release_tag=release_tag,
        workers=mutation_task.AUTOMATIC_WORKERS if workers is None else str(workers),
        lease=lambda: mutation_lease.lease(BUILD_DIRECTORY),
    )


_positive_timeout = task_timeout.positive_timeout


def main(argv: list[str] | None = None) -> int:
    """Dispatch one documented local task.

    Returns:
        Zero after the selected task succeeds.

    """
    parser = task_cli.build_parser(
        __doc__,
        _positive_timeout,
        mutation_task.parse_workers,
    )
    arguments = parser.parse_args(argv)
    task = arguments.task
    actions: dict[str, Callable[[], None]] = {
        "build": _build_zipapp,
        "check": _check,
        "ci": lambda: _ci(arguments.release_tag, arguments.workers),
        "coverage": _coverage,
        "mutation": lambda: _mutation(
            workers=arguments.workers,
            preflight=not arguments.allow_stale_manifest,
        ),
        "quality": lambda: _quality(native=arguments.native),
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
