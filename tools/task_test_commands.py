"""Build cache-free pytest commands for the portable task runner."""

from __future__ import annotations

from typing import TYPE_CHECKING

if TYPE_CHECKING:
    from pathlib import Path


def _pytest_module(coverage: object) -> tuple[str, ...]:
    """Return the exact pytest module prefix for a strict boolean mode.

    Returns:
        The direct pytest or coverage-wrapped pytest module arguments.

    Raises:
        TypeError: If the assurance mode is not an actual boolean.

    """
    if coverage is True:
        return "coverage", "run", "-m", "pytest"
    if coverage is False:
        return ("pytest",)
    message = "coverage mode must be a boolean"
    raise TypeError(message)


def pytest_command(
    executable: str,
    report: Path,
    *,
    coverage: bool,
) -> tuple[str, ...]:
    """Return one strict pytest command with repository caching disabled.

    Returns:
        The direct test or coverage-wrapped pytest command.

    """
    module = _pytest_module(coverage)
    return (
        executable,
        "-X",
        "dev",
        "-W",
        "error",
        "-m",
        *module,
        "-vv",
        "-p",
        "no:cacheprovider",
        "--hypothesis-show-statistics",
        f"--junitxml={report}",
    )
