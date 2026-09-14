"""Give concurrent Mutmut workers distinct pytest temporary directories."""

from __future__ import annotations

import os
from pathlib import Path
from typing import TYPE_CHECKING, Final

if TYPE_CHECKING:
    from _pytest.config import Config
    from _pytest.config.argparsing import Parser


TEMPORARY_ROOT_VARIABLE: Final = "EML_MUTATION_PYTEST_TEMPORARY_ROOT"


def pytest_load_initial_conftests(
    early_config: Config,
    parser: Parser,
    args: list[str],
) -> None:
    """Set a process-private base temp directory before pytest parses options."""
    del early_config, parser
    root = os.environ.get(TEMPORARY_ROOT_VARIABLE)
    if root is not None:
        args.extend(("--basetemp", str(Path(root) / str(os.getpid()))))
