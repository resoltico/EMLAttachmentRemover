"""Give concurrent Mutmut workers distinct pytest temporary directories."""

from __future__ import annotations

import os
import shutil
import sys
from pathlib import Path
from typing import TYPE_CHECKING, Final

import pytest

if TYPE_CHECKING:
    from _pytest.config import Config
    from _pytest.config.argparsing import Parser


TEMPORARY_ROOT_VARIABLE: Final = "EML_MUTATION_PYTEST_TEMPORARY_ROOT"
MUTANT_MARKER_VARIABLE: Final = "MUTANT_UNDER_TEST"
BASETEMP_KEY: Final = pytest.StashKey[tuple[Path, int]]()


def _workspace_import_root(cwd: Path) -> Path | None:
    """Return the complete generated Mutmut workspace below or at ``cwd``.

    Returns:
        The generated workspace when its repository markers are complete.

    """
    for candidate in (cwd / "mutants", cwd):
        if (
            candidate.is_dir()
            and (candidate / "pyproject.toml").is_file()
            and (candidate / "tools").is_dir()
        ):
            return candidate
    return None


def pytest_load_initial_conftests(
    early_config: Config,
    parser: Parser,
    args: list[str],
) -> None:
    """Set a process-private base temp directory before pytest parses options."""
    del parser
    if os.environ.get(MUTANT_MARKER_VARIABLE) is not None:
        workspace = _workspace_import_root(Path.cwd())
        if workspace is not None:
            sys.path.insert(0, str(workspace))
            package = sys.modules.get(__package__)
            package_paths = getattr(package, "__path__", None)
            workspace_tools = str(workspace / "tools")
            if isinstance(package_paths, list) and workspace_tools not in package_paths:
                package_paths.insert(0, workspace_tools)
    root = os.environ.get(TEMPORARY_ROOT_VARIABLE)
    if root is not None:
        basetemp = Path(root) / str(os.getpid())
        early_config.stash[BASETEMP_KEY] = (basetemp, os.getpid())
        args.extend(("--basetemp", str(basetemp)))


def pytest_unconfigure(config: Config) -> None:
    """Remove this session's private base temp directory, best-effort.

    Mutmut forks one pytest session per mutant, so without this the directories
    grow with completed mutants. Removal never raises: a failing hook inside a
    mutant child could change that mutant's verdict. The campaign owner removes
    the whole temporary root afterwards, including directories of killed workers.
    Only the creating process removes a directory, never an inheriting fork.
    """
    recorded = config.stash.get(BASETEMP_KEY, None)
    if recorded is not None and recorded[1] == os.getpid():
        shutil.rmtree(recorded[0], ignore_errors=True)
