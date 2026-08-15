"""Load Hypothesis configuration and finalize its repository-local artifacts."""

import os

import pytest
from tools import finalize_hypothesis_artifacts

from tests import hypothesis_config as _hypothesis_config

__all__ = ["_hypothesis_config"]


@pytest.hookimpl(trylast=True)
def pytest_sessionfinish() -> None:
    """Remove private caches and sanitize enabled observations after every run."""
    finalize_hypothesis_artifacts.finalize(
        observations="HYPOTHESIS_EXPERIMENTAL_OBSERVABILITY" in os.environ,
    )
