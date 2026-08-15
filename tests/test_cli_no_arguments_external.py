"""Exercise the empty-argument CLI contract through public entry points."""

from __future__ import annotations

import shutil
import subprocess
import sys
from pathlib import Path
from typing import TYPE_CHECKING

from eml_attachment_remover import PROGRAM_VERSION, models
from eml_attachment_remover.models import PROGRAM_NAME
from tests.test_support import (
    EXPECTED_NO_ARGUMENTS,
    SUBPROCESS_TIMEOUT_SECONDS,
    subprocess_environment,
)

if TYPE_CHECKING:
    from collections.abc import Sequence


def _run_external(
    command: Sequence[str], working_directory: Path
) -> subprocess.CompletedProcess[str]:
    """Run one public entry point without inherited source-tree imports.

    Returns:
        The completed text-mode command result.

    """
    return subprocess.run(
        command,
        cwd=working_directory,
        env=subprocess_environment(),
        text=True,
        capture_output=True,
        check=False,
        timeout=SUBPROCESS_TIMEOUT_SECONDS,
    )


def _assert_no_arguments_contract(result: subprocess.CompletedProcess[str]) -> None:
    """Require the exact first-run screen and stream discipline."""
    assert result.returncode == int(models.ExitCode.USAGE)
    assert not result.stdout
    assert result.stderr == EXPECTED_NO_ARGUMENTS
    assert PROGRAM_VERSION not in result.stderr


def test_installed_console_script_has_exact_no_arguments_contract(
    tmp_path: Path,
) -> None:
    """Field the installed console script from outside the project directory."""
    command = shutil.which(PROGRAM_NAME, path=str(Path(sys.executable).parent))

    assert command is not None
    _assert_no_arguments_contract(_run_external([command], tmp_path))


def test_module_entry_point_has_exact_no_arguments_contract(tmp_path: Path) -> None:
    """Field ``python -m`` from outside the project directory."""
    _assert_no_arguments_contract(
        _run_external([sys.executable, "-m", "eml_attachment_remover"], tmp_path),
    )
