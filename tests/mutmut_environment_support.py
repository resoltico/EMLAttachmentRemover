"""Keep test-controlled environments transparent to Mutmut's selector."""

from __future__ import annotations

import os
from contextlib import contextmanager
from typing import TYPE_CHECKING
from unittest.mock import patch

from tools import mutmut_workspace

if TYPE_CHECKING:
    from collections.abc import Iterator, Mapping
    from pathlib import Path


def selector_preserving_environment(values: Mapping[str, str]) -> dict[str, str]:
    """Return controlled values without overwriting an active Mutmut selector.

    Returns:
        A fresh environment containing the active selector when one is present.

    """
    if mutmut_workspace.MARKER in values:
        message = "inject Mutmut policy markers explicitly, not through os.environ"
        raise ValueError(message)
    environment = dict(values)
    selector = os.environ.get(mutmut_workspace.MARKER)
    if selector is not None:
        environment[mutmut_workspace.MARKER] = selector
    return environment


@contextmanager
def explicit_mutmut_marker(marker: str) -> Iterator[None]:
    """Inject a marker into workspace policy calls without masking trampolines.

    Yields:
        Control while both generated-artifact predicates receive the marker.

    """
    original_root_file = mutmut_workspace.generated_root_file
    original_sidecar = mutmut_workspace.generated_sidecar

    def generated_root_file(path: Path, root: Path) -> bool:
        return original_root_file(path, root, marker=marker)

    def generated_sidecar(path: Path, root: Path) -> bool:
        return original_sidecar(path, root, marker=marker)

    with (
        patch.object(
            mutmut_workspace,
            "generated_root_file",
            generated_root_file,
        ),
        patch.object(mutmut_workspace, "generated_sidecar", generated_sidecar),
    ):
        yield
