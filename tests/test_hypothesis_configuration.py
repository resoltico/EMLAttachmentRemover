"""Verify that project-level Hypothesis configuration is active and enforceable."""

from __future__ import annotations

import ast
import json
import os
import re
import subprocess
import sys
import unittest
from pathlib import Path
from typing import TYPE_CHECKING, Final, cast

from hypothesis import settings

from tests import hypothesis_config

if TYPE_CHECKING:
    from collections.abc import Mapping

PROJECT_ROOT = Path(__file__).resolve().parents[1]
CI_ENVIRONMENT_VARIABLES: Final = frozenset(
    {
        "CI",
        "__TOX_ENVIRONMENT_VARIABLE_ORIGINAL_CI",
        "TF_BUILD",
        "bamboo.buildKey",
        "BUILDKITE",
        "CIRCLECI",
        "CIRRUS_CI",
        "CODEBUILD_BUILD_ID",
        "GITHUB_ACTIONS",
        "GITLAB_CI",
        "HEROKU_TEST_RUN_ID",
        "TEAMCITY_VERSION",
    },
)
OBSERVABILITY_VARIABLES: Final = frozenset(
    {
        "HYPOTHESIS_EXPERIMENTAL_OBSERVABILITY",
        "HYPOTHESIS_EXPERIMENTAL_OBSERVABILITY_NOCOVER",
    },
)
PROFILE_PROGRAM: Final = """import json
from hypothesis import settings
import tests

active = settings.default
assert active is not None
print(json.dumps({
    "backend": active.backend,
    "database": None if active.database is None else type(active.database).__name__,
    "database_path": None if active.database is None else str(active.database.path),
    "deadline": active.deadline,
    "derandomize": active.derandomize,
    "max_examples": active.max_examples,
    "phases": [phase.name for phase in active.phases],
    "print_blob": active.print_blob,
    "profile": settings.get_current_profile_name(),
    "report_multiple_bugs": active.report_multiple_bugs,
    "stateful_step_count": active.stateful_step_count,
    "suppress_health_check": [check.name for check in active.suppress_health_check],
    "verbosity": active.verbosity.name,
}, sort_keys=True))
"""
FAILING_PROGRAM: Final = """from hypothesis import given, strategies as st
from tests import hypothesis_config

@given(st.integers())
def impossible(value: int) -> None:
    assert value < 0, f"replayed value={value}"

impossible()
"""
REPRODUCE_PATTERN: Final = re.compile(
    r"@reproduce_failure\((?P<version>'[^']+'), (?P<blob>b'[^']+')\)",
)
ALL_PHASE_NAMES: Final = [
    "explicit",
    "reuse",
    "generate",
    "target",
    "shrink",
    "explain",
]
SUBPROCESS_TIMEOUT_SECONDS: Final = 30


def _environment(*, profile: str, ambient_ci: bool) -> dict[str, str]:
    """Return a controlled environment for one profile-contract subprocess.

    Returns:
        A copy of the process environment with an explicit profile and CI state.

    """
    environment = os.environ.copy()
    for variable in CI_ENVIRONMENT_VARIABLES | OBSERVABILITY_VARIABLES:
        environment.pop(variable, None)
    if ambient_ci:
        environment["CI"] = "true"
    environment["HYPOTHESIS_PROFILE"] = profile
    environment[hypothesis_config.STORAGE_ENVIRONMENT_VARIABLE] = str(
        hypothesis_config.PRIVATE_STORAGE_ROOT
    )
    environment["PYTHONDEVMODE"] = "1"
    environment["PYTHONNOUSERSITE"] = "1"
    environment["PYTHONWARNINGS"] = "error"
    return environment


def _run_program(
    program: str, environment: Mapping[str, str]
) -> subprocess.CompletedProcess[str]:
    """Execute one isolated Hypothesis contract program.

    Returns:
        The completed subprocess with captured text output.

    """
    return subprocess.run(
        [sys.executable, "-X", "dev", "-W", "error", "-c", program],
        cwd=PROJECT_ROOT,
        env=environment,
        text=True,
        capture_output=True,
        check=False,
        timeout=SUBPROCESS_TIMEOUT_SECONDS,
    )


def _expected_profile(profile: str) -> dict[str, object]:
    """Return the complete effective settings contract for one project profile.

    Returns:
        JSON-compatible expected settings.

    """
    variable_settings: dict[str, tuple[int, bool, str | None]] = {
        hypothesis_config.DEVELOPMENT_PROFILE: (
            250,
            False,
            "DirectoryBasedExampleDatabase",
        ),
        hypothesis_config.CI_PROFILE: (500, True, None),
        hypothesis_config.THOROUGH_PROFILE: (
            2_000,
            False,
            "DirectoryBasedExampleDatabase",
        ),
        hypothesis_config.MUTATION_PROFILE: (50, True, None),
    }
    max_examples, derandomize, database = variable_settings[profile]
    database_path = (
        None
        if database is None
        else str(hypothesis_config.PRIVATE_STORAGE_ROOT / "examples")
    )
    return {
        "backend": "hypothesis",
        "database": database,
        "database_path": database_path,
        "deadline": None,
        "derandomize": derandomize,
        "max_examples": max_examples,
        "phases": ALL_PHASE_NAMES,
        "print_blob": True,
        "profile": profile,
        "report_multiple_bugs": True,
        "stateful_step_count": 50,
        "suppress_health_check": [],
        "verbosity": "normal",
    }


class HypothesisConfigurationTests(unittest.TestCase):
    """Exercise profile selection and failure reporting as external behaviour."""

    def test_imported_configuration_is_the_active_profile(self) -> None:
        self.assertEqual(
            settings.get_current_profile_name(),
            hypothesis_config.ACTIVE_PROFILE,
        )
        self.assertIs(
            settings.default,
            settings.get_profile(hypothesis_config.ACTIVE_PROFILE),
        )

    def test_every_profile_contract_is_independent_of_ambient_ci(self) -> None:
        for profile in sorted(hypothesis_config.PROJECT_PROFILES):
            for ambient_ci in (False, True):
                with self.subTest(profile=profile, ambient_ci=ambient_ci):
                    result = _run_program(
                        PROFILE_PROGRAM,
                        _environment(profile=profile, ambient_ci=ambient_ci),
                    )
                    self.assertEqual(result.returncode, 0, result.stderr)
                    actual = cast("dict[str, object]", json.loads(result.stdout))
                    self.assertEqual(actual, _expected_profile(profile))

    def test_unknown_profile_fails_before_tests_run(self) -> None:
        result = _run_program(
            "import tests",
            _environment(profile="unknown-project-profile", ambient_ci=True),
        )
        self.assertNotEqual(result.returncode, 0)
        self.assertIn("unsupported HYPOTHESIS_PROFILE", result.stderr)

    def test_external_storage_is_required_and_repository_storage_is_rejected(
        self,
    ) -> None:
        """Fail before tests can create an opaque replay database in the project."""
        missing = _environment(
            profile=hypothesis_config.DEVELOPMENT_PROFILE,
            ambient_ci=False,
        )
        missing.pop(hypothesis_config.STORAGE_ENVIRONMENT_VARIABLE)
        missing_result = _run_program("import tests", missing)
        self.assertNotEqual(missing_result.returncode, 0)
        self.assertIn(
            "HYPOTHESIS_STORAGE_DIRECTORY is required; run tests through "
            "tools/tasks.py",
            missing_result.stderr,
        )

        for unsafe_storage in (
            ".hypothesis",
            str(PROJECT_ROOT / ".hypothesis"),
        ):
            with self.subTest(storage=unsafe_storage):
                unsafe = _environment(
                    profile=hypothesis_config.THOROUGH_PROFILE,
                    ambient_ci=False,
                )
                unsafe[hypothesis_config.STORAGE_ENVIRONMENT_VARIABLE] = unsafe_storage
                unsafe_result = _run_program("import tests", unsafe)
                self.assertNotEqual(unsafe_result.returncode, 0)
                self.assertIn(
                    "HYPOTHESIS_STORAGE_DIRECTORY must be an absolute path outside "
                    "the project",
                    unsafe_result.stderr,
                )

    def test_ci_profile_reports_a_minimized_counterexample(self) -> None:
        environment = _environment(
            profile=hypothesis_config.CI_PROFILE,
            ambient_ci=True,
        )
        result = _run_program(FAILING_PROGRAM, environment)
        self.assertNotEqual(result.returncode, 0)
        self.assertIn("Failing test case", result.stderr)
        self.assertRegex(result.stderr, r"(?m)^\s+value=0,$")
        match = REPRODUCE_PATTERN.search(result.stderr)
        self.assertIsNotNone(match)
        assert match is not None
        version = ast.literal_eval(match.group("version"))
        blob = ast.literal_eval(match.group("blob"))
        self.assertIsInstance(version, str)
        self.assertIsInstance(blob, bytes)
        replay_program = f"""from hypothesis import given, reproduce_failure
from hypothesis import strategies as st
from tests import hypothesis_config

@reproduce_failure({version!r}, {blob!r})
@given(st.integers())
def impossible(value: int) -> None:
    assert value < 0, f"replayed value={{value}}"

impossible()
"""
        replay = _run_program(replay_program, environment)
        self.assertNotEqual(replay.returncode, 0)
        self.assertIn("AssertionError: replayed value=0", replay.stderr)
