"""Use the invocation datetime with Hatchling's reproducible archive builder."""

from __future__ import annotations

import os

if __package__:
    from tools.build_timestamp import environment
else:
    from build_timestamp import environment  # type: ignore[import-not-found,no-redef]
from hatchling.build import (
    build_editable,
    build_sdist,
    build_wheel,
    get_requires_for_build_editable,
    get_requires_for_build_sdist,
    get_requires_for_build_wheel,
)

__all__ = [
    "build_editable",
    "build_sdist",
    "build_wheel",
    "get_requires_for_build_editable",
    "get_requires_for_build_sdist",
    "get_requires_for_build_wheel",
]

os.environ.update(environment())
