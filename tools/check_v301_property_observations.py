"""Require every declared v3 property family to emit genuine observations."""

from __future__ import annotations

import json
from pathlib import Path
from typing import Final

PROJECT_ROOT: Final = Path(__file__).resolve().parents[1]
MANIFEST: Final = PROJECT_ROOT / "tests" / "v301_property_families.json"
OBSERVED: Final = PROJECT_ROOT / ".hypothesis" / "observed"
MANIFEST_ERROR: Final = "invalid v3 property-family manifest"
OBSERVATION_ERROR: Final = "v3 property observations are malformed"


class PropertyObservationError(ValueError):
    """Report unusable property-family manifests or observation evidence."""


def _families(path: Path) -> dict[str, tuple[str, ...]]:
    """Load one strict public manifest of property family node IDs.

    Returns:
        Family names mapped to their nonempty expected property node IDs.

    Raises:
        PropertyObservationError: If the manifest is absent, malformed, or incomplete.

    """
    try:
        loaded = json.loads(path.read_text(encoding="utf-8"))
    except (OSError, json.JSONDecodeError) as error:
        message = "cannot load v3 property-family manifest"
        raise PropertyObservationError(message) from error
    if not isinstance(loaded, dict) or loaded.get("schema_version") != 1:
        raise PropertyObservationError(MANIFEST_ERROR)
    values = loaded.get("families")
    if not isinstance(values, dict) or not values:
        raise PropertyObservationError(MANIFEST_ERROR)
    result: dict[str, tuple[str, ...]] = {}
    for name, identifiers in values.items():
        if not isinstance(name, str) or not isinstance(identifiers, list):
            raise PropertyObservationError(MANIFEST_ERROR)
        if not identifiers or not all(
            isinstance(value, str) and value for value in identifiers
        ):
            raise PropertyObservationError(MANIFEST_ERROR)
        result[name] = tuple(identifiers)
    return result


def _observed_properties(directory: Path) -> set[str]:
    """Return every passed generated test-case property from safe JSONL records.

    Returns:
        The set of generated test-case property identifiers.

    Raises:
        PropertyObservationError: If observation evidence is absent or malformed.

    """
    paths = sorted(directory.glob("*_testcases.jsonl"))
    if not paths:
        message = "v3 property observations are absent"
        raise PropertyObservationError(message)
    result: set[str] = set()
    for path in paths:
        result.update(_properties_in_file(path))
    return result


def _properties_in_file(path: Path) -> set[str]:
    """Return passed test-case properties from one strict observation file.

    Returns:
        The set of passed generated property node IDs.

    """
    result: set[str] = set()
    for line in path.read_text(encoding="utf-8").splitlines():
        result.update(_property_in_record(line))
    return result


def _property_in_record(line: str) -> set[str]:
    """Return the passed generated property represented by one JSONL record.

    Returns:
        A singleton ID set for a passed generated case, otherwise an empty set.

    Raises:
        PropertyObservationError: If the JSON record is not structurally valid.

    """
    try:
        record = json.loads(line)
    except json.JSONDecodeError as error:
        raise PropertyObservationError(OBSERVATION_ERROR) from error
    if not isinstance(record, dict):
        raise PropertyObservationError(OBSERVATION_ERROR)
    if record.get("type") != "test_case" or record.get("status") != "passed":
        return set()
    property_id = record.get("property")
    if not isinstance(property_id, str) or not property_id:
        raise PropertyObservationError(OBSERVATION_ERROR)
    return {property_id}


def check(manifest: Path = MANIFEST, observed: Path = OBSERVED) -> None:
    """Require one passed generated case for each declared v3 property family.

    Raises:
        PropertyObservationError: If any declared family lacks observation evidence.

    """
    properties = _observed_properties(observed)
    missing = [
        name
        for name, identifiers in _families(manifest).items()
        if not any(identifier in properties for identifier in identifiers)
    ]
    if missing:
        message = f"v3 property families lack observations: {', '.join(missing)}"
        raise PropertyObservationError(message)


def main() -> int:
    """Print one concise property-evidence result for local and CI qualification.

    Returns:
        Zero only when every declared family has generated observation evidence.

    """
    try:
        check(MANIFEST, OBSERVED)
    except PropertyObservationError as error:
        print(error)
        return 1
    print("v3 property observations cover every declared family")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
