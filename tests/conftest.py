"""Load Hypothesis configuration and finalize its repository-local artifacts."""

import os
from contextlib import nullcontext
from types import SimpleNamespace

import pytest
from tools import finalize_hypothesis_artifacts, tasks

from tests import hypothesis_config as _hypothesis_config

__all__ = ["_hypothesis_config"]


@pytest.fixture(autouse=True)  # ruff: ignore[pytest-fixture-autouse] - no test may take the real lease.
def _private_mutation_lease(monkeypatch: pytest.MonkeyPatch) -> None:
    """Keep task tests from contending for the checkout's real mutation lease.

    A campaign runs this suite inside itself, so a test of the mutation task that
    took the real lease would always find it held.
    """
    monkeypatch.setattr(
        tasks, "mutation_lease", SimpleNamespace(lease=lambda _build: nullcontext())
    )


@pytest.hookimpl(trylast=True)
def pytest_sessionfinish() -> None:
    """Remove private caches and sanitize enabled observations after every run."""
    finalize_hypothesis_artifacts.finalize(
        observations="HYPOTHESIS_EXPERIMENTAL_OBSERVABILITY" in os.environ,
    )
