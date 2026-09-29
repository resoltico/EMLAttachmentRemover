"""The platform-independent static checks of the quality gate."""

from __future__ import annotations

import os
import sys
from typing import TYPE_CHECKING

if TYPE_CHECKING:
    from collections.abc import Callable, Sequence
    from pathlib import Path

    from tools.task_interfaces import RepositoryHygiene


def run(
    run_command: Callable[[Sequence[str]], None],
    hygiene: RepositoryHygiene,
    project_root: Path,
    workflow_files: Sequence[str],
    shell_scripts: Sequence[str],
) -> None:
    """Run formatting, lint, type, module-design, workflow, and secret checks.

    Raises:
        RuntimeError: If the fresh pre-secret-scan repository audit is dirty.

    """
    run_command(("ruff", "format", "--check", "--no-cache", "."))
    run_command(("ruff", "check", "--no-cache", "."))
    run_command(("mypy", "--no-incremental", "--cache-dir", os.devnull))
    run_command((sys.executable, "tools/check_module_design.py"))
    run_command((
        *("actionlint", "-shellcheck", "shellcheck", "-pyflakes", "pyflakes"),
        *workflow_files,
    ))
    run_command(("shellcheck", "--shell=sh", *shell_scripts))
    audit = hygiene.audit_repository(project_root)
    if audit.issues:
        diagnostics = "\n".join(audit.diagnostics(project_root))
        message = f"repository changed before secret scanning:\n{diagnostics}"
        raise RuntimeError(message)
    run_command((
        "detect-secrets-hook",
        "--no-verify",
        *(str(path) for path in audit.public_files),
    ))
