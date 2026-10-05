"""Retain upstream runtime notices and supply authenticated missing texts."""

from __future__ import annotations

import json
import shutil
from pathlib import Path
from typing import Final

SUPPLEMENTS: Final = (
    Path(__file__).resolve().parents[1] / "integrations/macos-ui/runtime-notices"
)


def apply(runtime: Path) -> None:
    """Fill missing notices and refuse a runtime lacking declared license texts.

    Raises:
        ValueError: If metadata refers to a missing, empty or unsafe notice.

    """
    for source in sorted(SUPPLEMENTS.glob("LICENSE.*.txt")):
        destination = runtime / "licenses" / source.name
        if not destination.exists():
            shutil.copy2(source, destination)
    metadata = json.loads((runtime / "PYTHON.json").read_bytes())
    for name in references(metadata):
        path = runtime / name
        if (
            not name.startswith("licenses/")
            or ".." in Path(name).parts
            or path.is_symlink()
            or not path.is_file()
            or not path.stat().st_size
        ):
            message = "runtime is missing a safe, nonempty declared license notice"
            raise ValueError(message)


def references(value: object) -> set[str]:
    """Collect upstream references; malformed declared fields are refused.

    Returns:
        Every declared license pathname in the upstream metadata.

    """
    result: set[str] = set()
    if isinstance(value, dict):
        for key, child in value.items():
            if key in {"license_path", "license_paths"}:
                result.update(_notice_names(child, key))
            else:
                result.update(references(child))
    elif isinstance(value, list):
        for child in value:
            result.update(references(child))
    return result


def _notice_names(value: object, field: str) -> set[str]:
    """Validate the singular/plural forms declared by upstream metadata.

    Returns:
        The complete declared pathname set.

    Raises:
        ValueError: If any declared pathname is not a string.

    """
    values = [value] if field == "license_path" else value
    if not isinstance(values, list) or not all(
        isinstance(item, str) for item in values
    ):
        message = "invalid runtime license references"
        raise ValueError(message)
    return set(values)
