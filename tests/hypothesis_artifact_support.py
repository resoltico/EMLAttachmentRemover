"""Shared synthetic observation support for Hypothesis artifact tests."""

from __future__ import annotations

import json
from typing import TYPE_CHECKING

if TYPE_CHECKING:
    from pathlib import Path


def write_observations(root: Path, records: list[object]) -> Path:
    """Write one synthetic Hypothesis JSON-lines observation file.

    Returns:
        The observation path.

    """
    observed = root / ".hypothesis" / "observed"
    observed.mkdir(parents=True)
    path = observed / "2026-01-01_testcases.jsonl"
    path.write_text(
        "".join(f"{json.dumps(record)}\n" for record in records),
        encoding="utf-8",
    )
    return path
