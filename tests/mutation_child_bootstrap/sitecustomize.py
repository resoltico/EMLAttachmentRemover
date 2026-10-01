"""Initialize generated-source children and retain measured Mutmut call names."""

from __future__ import annotations

import os
from pathlib import Path

from mutmut import stats
from mutmut.configuration import config
from mutmut.mutation import trampoline


def _configure_workspace(workspace: str) -> None:
    original = Path.cwd()
    try:
        os.chdir(workspace)
        config()
    finally:
        os.chdir(original)


def _retain_call_names(path: str) -> None:
    original = stats.record_trampoline_hit
    seen: set[str] = set()

    def recorded(name: str, caller: str | None = None) -> None:
        original(name, caller)
        if name not in seen:
            seen.add(name)
            with Path(path).open("a", encoding="utf-8") as journal:
                journal.write(name + "\n")

    stats.record_trampoline_hit = recorded
    setattr(trampoline, "record_trampoline_hit", recorded)  # ruff: ignore[set-attr-with-constant] - reexported runtime binding in pinned Mutmut.


if workspace := os.environ.get("EML_MUTATION_CHILD_WORKSPACE"):
    _configure_workspace(workspace)
    if journal := os.environ.get("EML_MUTATION_CHILD_STATS"):
        _retain_call_names(journal)
