"""Expected host-local command plans for workflow parity tests."""

from __future__ import annotations

import os
from pathlib import Path
from typing import TYPE_CHECKING, Final

if TYPE_CHECKING:
    from collections.abc import Mapping, Sequence

HOST_ROOT: Final = Path("/host-ci")


def _lane(python: str) -> dict[str, str]:
    return {
        "UV_PROJECT_ENVIRONMENT": str(HOST_ROOT / f"venv-{python}"),
        "UV_PYTHON": python,
        "VIRTUAL_ENV": str(HOST_ROOT / f"venv-{python}"),
    }


def _installer(python: str) -> dict[str, str]:
    return {**_lane(python), "PYTHONWARNINGS": "default"}


def _type_check(platform: str) -> tuple[str, ...]:
    return (
        "uv",
        "run",
        "mypy",
        "--no-incremental",
        "--cache-dir",
        os.devnull,
        "--platform",
        platform,
    )


EXPECTED_POSIX_PLAN: Final = (
    ("uv sync --locked --group dev --python 3.14.7", _installer("3.14.7"), 600),
    ("test -x /bin/sh", _lane("3.14.7"), 600),
    ("uv run python tools/tasks.py quality", _lane("3.14.7"), 1_800),
    ("uv sync --locked --group dev --python 3.14.7t", _installer("3.14.7t"), 600),
    ("test -x /bin/sh", _lane("3.14.7t"), 600),
    ("uv run python tools/tasks.py quality --native", _lane("3.14.7t"), 1_800),
    (_type_check("darwin"), _lane("3.14.7"), 600),
    (_type_check("linux"), _lane("3.14.7"), 600),
    (_type_check("win32"), _lane("3.14.7"), 600),
    ("uv sync --locked --group dev --python 3.14.7", _installer("3.14.7"), 600),
    ("uv run python tools/check_release_tag.py v1.2.3", _lane("3.14.7"), 600),
    (
        (
            *("uv", "run", "python", "-B", "-m", "tools.release_delivery"),
            *("--output-directory", str(HOST_ROOT / "release-dist"), "--portable-only"),
        ),
        _lane("3.14.7"),
        1_800,
    ),
    (
        (
            *("uv", "run", "--no-project", "--python", "3.14.7", "python"),
            *("-B", "-m", "tools.release_delivery", "--verify-directory"),
            str(HOST_ROOT / "release-dist"),
            "--portable-only",
        ),
        _lane("3.14.7"),
        1_800,
    ),
    ("uv run python tools/tasks.py mutation --workers 4", _lane("3.14.7"), 10_800),
    (
        "uv run python tools/tasks.py thorough --observable --timeout-seconds 3300",
        _lane("3.14.7"),
        3_600,
    ),
    (
        "uv run python tools/finalize_hypothesis_artifacts.py --observations",
        _lane("3.14.7"),
        600,
    ),
    (
        "uv run python tools/tasks.py thorough --observable --timeout-seconds 3300",
        _lane("3.14.7t"),
        3_600,
    ),
    (
        "uv run python tools/finalize_hypothesis_artifacts.py --observations",
        _lane("3.14.7t"),
        600,
    ),
)


def commands(
    expected: Sequence[tuple[object, Mapping[str, str], int]],
) -> list[tuple[tuple[str, ...], Mapping[str, str], int]]:
    return [
        (
            command if isinstance(command, tuple) else tuple(str(command).split()),
            environment,
            timeout,
        )
        for command, environment, timeout in expected
    ]
