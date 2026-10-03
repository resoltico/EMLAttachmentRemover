"""Load Hypothesis configuration and finalize its repository-local artifacts."""

from __future__ import annotations

import os
import signal
import tempfile
import uuid
from contextlib import nullcontext
from pathlib import Path
from types import SimpleNamespace
from typing import TYPE_CHECKING, Never

import pytest
from mutmut.mutation.trampoline import get_mutant_under_test
from mutmut.state import state as mutation_state
from tools import finalize_hypothesis_artifacts, tasks

import eml_attachment_remover
from tests import hypothesis_config as _hypothesis_config
from tests.deadline_support import finite_operation

if TYPE_CHECKING:
    from collections.abc import Iterator

__all__ = ["_hypothesis_config"]
CHILD_STATS = pytest.StashKey[Path]()


@pytest.fixture(autouse=True)  # ruff: ignore[pytest-fixture-autouse] - signal tests own their baseline policy independent of the invoking shell.
def _controlled_sigint_policy() -> Iterator[None]:
    previous = signal.getsignal(signal.SIGINT)
    signal.signal(signal.SIGINT, signal.default_int_handler)
    try:
        yield
    finally:
        signal.signal(signal.SIGINT, previous)


@pytest.fixture(scope="session")
def _child_journal_root() -> Iterator[Path]:
    """Own journal storage until all function-scoped fault injections retire.

    Yields:
        One private session directory outside every test's working directory.

    """
    with tempfile.TemporaryDirectory(prefix="eml-child-call-names-") as private:
        yield Path(private)


@pytest.fixture(autouse=True)  # ruff: ignore[pytest-fixture-autouse] - spawned source tests must execute the selected mutant.
def _mutated_child_imports(
    monkeypatch: pytest.MonkeyPatch,
    request: pytest.FixtureRequest,
    _child_journal_root: Path,
) -> None:
    selector = get_mutant_under_test()
    source = Path(eml_attachment_remover.__file__).resolve().parents[1]
    workspace = source.parent
    if workspace.name == "mutants" and (workspace / "pyproject.toml").is_file():
        bootstrap = workspace / "tests" / "mutation_child_bootstrap"
        monkeypatch.setenv(
            "PYTHONPATH", os.pathsep.join((str(bootstrap), str(source), str(workspace)))
        )
        monkeypatch.setenv("EML_MUTATION_CHILD_WORKSPACE", str(workspace))
        if selector == "stats":
            journal = _child_journal_root / (uuid.uuid4().hex + ".txt")
            journal.touch(mode=0o600)
            request.node.stash[CHILD_STATS] = journal
            monkeypatch.setenv("EML_MUTATION_CHILD_STATS", str(journal))


@pytest.hookimpl(wrapper=True)
def pytest_runtest_call(item: pytest.Item) -> Iterator[None]:
    """Attribute measured child calls before Mutmut collects this test's mapping.

    Yields:
        Control to the test and its subprocess ownership cleanup.

    """
    try:
        yield
    finally:
        journal = item.stash.get(CHILD_STATS, None)
        if journal is not None:
            mutation_state()._stats.update(journal.read_text().splitlines())  # ruff: ignore[private-member-access] - pinned Mutmut measured-call mapping.


@pytest.fixture(autouse=True)  # ruff: ignore[pytest-fixture-autouse] - nontermination must fail before the runner timeout.
def _bound_finite_output_mutations() -> Iterator[None]:
    selector = get_mutant_under_test()
    if selector.startswith(
        "eml_attachment_remover.report_delivery.x__write_all__mutmut_"
    ):
        with finite_operation(10):
            yield
    else:
        yield


@pytest.fixture(scope="session", autouse=True)  # ruff: ignore[pytest-fixture-autouse] - process exits are tested in exec children.
def _contain_unexpected_process_exit() -> Iterator[None]:
    """Fail unexpected in-process exits without killing the mutation worker."""

    def unexpected(status: int) -> Never:
        pytest.fail(f"unexpected in-process os._exit({status})")

    with pytest.MonkeyPatch.context() as patch:
        patch.setattr(os, "_exit", unexpected)
        yield


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
