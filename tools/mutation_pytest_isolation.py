"""Give concurrent Mutmut workers distinct pytest temporary directories."""

from __future__ import annotations

import os
import sys
from pathlib import Path
from typing import TYPE_CHECKING, Final

if TYPE_CHECKING:
    from _pytest.config import Config
    from _pytest.config.argparsing import Parser


TEMPORARY_ROOT_VARIABLE: Final = "EML_MUTATION_PYTEST_TEMPORARY_ROOT"
MUTANT_MARKER_VARIABLE: Final = "MUTANT_UNDER_TEST"


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
    del early_config, parser
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
        args.extend(("--basetemp", str(Path(root) / str(os.getpid()))))
