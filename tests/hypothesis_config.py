"""Configure explicit Hypothesis profiles for this test suite."""

from __future__ import annotations

import os
from pathlib import Path

from hypothesis import Phase, Verbosity, settings
from hypothesis.database import DirectoryBasedExampleDatabase

DEVELOPMENT_PROFILE = "project-development"
CI_PROFILE = "project-ci"
THOROUGH_PROFILE = "project-thorough"
MUTATION_PROFILE = "project-mutation"
ACTIVE_PROFILE = os.environ.get("HYPOTHESIS_PROFILE", DEVELOPMENT_PROFILE)
PROJECT_PROFILES = frozenset(
    {DEVELOPMENT_PROFILE, CI_PROFILE, THOROUGH_PROFILE, MUTATION_PROFILE},
)
PROJECT_ROOT = Path(__file__).resolve().parents[1]
STORAGE_ENVIRONMENT_VARIABLE = "HYPOTHESIS_STORAGE_DIRECTORY"


def _private_storage_root() -> Path:
    """Return the required external Hypothesis storage root.

    Returns:
        The resolved absolute storage root supplied by the canonical task runner.

    Raises:
        RuntimeError: If storage is missing, relative, or inside the repository.

    """
    raw_storage = os.environ.get(STORAGE_ENVIRONMENT_VARIABLE)
    if raw_storage is None:
        message = (
            f"{STORAGE_ENVIRONMENT_VARIABLE} is required; run tests through "
            "tools/tasks.py"
        )
        raise RuntimeError(message)
    candidate = Path(raw_storage)
    if not candidate.is_absolute():
        message = (
            f"{STORAGE_ENVIRONMENT_VARIABLE} must be an absolute path outside "
            "the project"
        )
        raise RuntimeError(message)
    storage = candidate.resolve()
    project = PROJECT_ROOT.resolve()
    if storage == project or storage.is_relative_to(project):
        message = (
            f"{STORAGE_ENVIRONMENT_VARIABLE} must be an absolute path outside "
            "the project"
        )
        raise RuntimeError(message)
    return storage


if ACTIVE_PROFILE not in PROJECT_PROFILES:
    profiles = ", ".join(sorted(PROJECT_PROFILES))
    raise RuntimeError(
        f"unsupported HYPOTHESIS_PROFILE={ACTIVE_PROFILE!r}; choose one of: {profiles}",
    )

PRIVATE_STORAGE_ROOT = _private_storage_root()
LOCAL_DATABASE = DirectoryBasedExampleDatabase(
    PRIVATE_STORAGE_ROOT / "examples",
)
BASE_PROFILE = settings.get_profile("default")
ALL_PHASES = tuple(Phase)


def _configure_profiles() -> None:
    """Register the project's local, CI, and pre-release test profiles."""
    settings.register_profile(
        DEVELOPMENT_PROFILE,
        parent=BASE_PROFILE,
        database=LOCAL_DATABASE,
        max_examples=250,
        deadline=None,
        derandomize=False,
        verbosity=Verbosity.normal,
        phases=ALL_PHASES,
        stateful_step_count=50,
        print_blob=True,
        report_multiple_bugs=True,
        suppress_health_check=(),
        backend="hypothesis",
    )
    settings.register_profile(
        CI_PROFILE,
        parent=BASE_PROFILE,
        max_examples=500,
        deadline=None,
        derandomize=True,
        database=None,
        verbosity=Verbosity.normal,
        phases=ALL_PHASES,
        stateful_step_count=50,
        print_blob=True,
        report_multiple_bugs=True,
        suppress_health_check=(),
        backend="hypothesis",
    )
    settings.register_profile(
        THOROUGH_PROFILE,
        parent=BASE_PROFILE,
        database=LOCAL_DATABASE,
        max_examples=2_000,
        deadline=None,
        derandomize=False,
        verbosity=Verbosity.normal,
        phases=ALL_PHASES,
        stateful_step_count=50,
        print_blob=True,
        report_multiple_bugs=True,
        suppress_health_check=(),
        backend="hypothesis",
    )
    settings.register_profile(
        MUTATION_PROFILE,
        parent=BASE_PROFILE,
        database=None,
        max_examples=50,
        deadline=None,
        derandomize=True,
        verbosity=Verbosity.normal,
        phases=ALL_PHASES,
        stateful_step_count=50,
        print_blob=True,
        report_multiple_bugs=True,
        suppress_health_check=(),
        backend="hypothesis",
    )
    settings.load_profile(ACTIVE_PROFILE)


_configure_profiles()
