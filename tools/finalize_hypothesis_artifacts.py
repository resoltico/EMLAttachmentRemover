"""Make repository-local Hypothesis evidence safe for public retention."""

from __future__ import annotations

import argparse
import importlib
import json
import shutil
import stat
import sys
import tempfile
from contextlib import suppress
from json import JSONDecodeError
from pathlib import Path
from typing import TYPE_CHECKING, Final

if TYPE_CHECKING:
    from collections.abc import Callable

    from tools import hypothesis_observation_safety as observation_safety
else:
    observation_safety = importlib.import_module(
        "tools.hypothesis_observation_safety"
        if __package__
        else "hypothesis_observation_safety"
    )

PROJECT_ROOT: Final = Path(__file__).resolve().parents[1]
LOCAL_ONLY_DIRECTORIES: Final = ("constants", "patches")
OBSERVATIONS_LABEL: Final = "Hypothesis observations"
ATOMIC_PREFIX: Final = ".public-hypothesis-"
FINALIZATION_GROUP_MESSAGE: Final = (
    "tests and Hypothesis artifact finalization both failed"
)
OBSERVATION_HELP: Final = "sanitize observation JSONL for public retention"
UTF8: Final = "utf-8"

HypothesisArtifactError = observation_safety.HypothesisArtifactError
_replacement_prefixes = observation_safety.replacement_prefixes
_public_value = observation_safety.public_value
_reject_absolute_paths = observation_safety.reject_absolute_paths
_contains_absolute_path = observation_safety.contains_absolute_path


def _directory_mode(path: Path, label: str) -> int | None:
    """Return a safe directory's mode, or ``None`` when it is absent.

    Returns:
        The directory mode when the path exists.

    """
    try:
        mode = path.lstat().st_mode
    except FileNotFoundError:
        return None
    except OSError as error:
        message = f"cannot inspect {label} {path}: {error}"
        raise HypothesisArtifactError(message) from error
    if not stat.S_ISDIR(mode):
        message = f"{label} must be a real directory when present: {path}"
        raise HypothesisArtifactError(message)
    return mode


def _remove_local_only_directories(root: Path) -> None:
    """Remove path-bearing constants and local suggested-source patches."""
    for name in LOCAL_ONLY_DIRECTORIES:
        path = root / ".hypothesis" / name
        if _directory_mode(path, f"Hypothesis {name}") is None:
            continue
        try:
            shutil.rmtree(path)
        except OSError as error:
            message = f"cannot remove Hypothesis {name} {path}: {error}"
            raise HypothesisArtifactError(message) from error
        if path.exists() or path.is_symlink():
            message = f"Hypothesis {name} removal was incomplete: {path}"
            raise HypothesisArtifactError(message)


def _load_observation(
    line: str,
    path: Path,
    line_number: int,
) -> observation_safety.JsonValue:
    """Load one strict JSON observation line.

    Returns:
        A JSON-compatible object.

    """
    if not line:
        message = f"blank Hypothesis observation at {path}:{line_number}"
        raise HypothesisArtifactError(message)
    try:
        loaded: object = json.loads(line)
    except JSONDecodeError as error:
        message = f"invalid Hypothesis observation at {path}:{line_number}: {error}"
        raise HypothesisArtifactError(message) from error
    if not isinstance(loaded, dict) or not all(type(key) is str for key in loaded):
        message = (
            f"Hypothesis observation must be a JSON object at {path}:{line_number}"
        )
        raise HypothesisArtifactError(message)
    return loaded


def _atomic_write(path: Path, content: str) -> None:
    """Atomically replace one observation file with sanitized UTF-8 JSONL."""
    try:
        _replace_with_temporary(path, content)
    except OSError as error:
        message = f"cannot publish sanitized Hypothesis observations {path}: {error}"
        raise HypothesisArtifactError(message) from error


def _replace_with_temporary(path: Path, content: str) -> None:
    """Replace one observation file and remove any temporary residue."""
    with tempfile.NamedTemporaryFile(
        mode="w",
        encoding=UTF8,
        newline="\n",
        prefix=ATOMIC_PREFIX,
        dir=path.parent,
        delete=False,
    ) as output:
        temporary = Path(output.name)
        try:
            output.write(content)
            output.close()
            temporary.replace(path)
        finally:
            with suppress(OSError):
                temporary.unlink()


def _sanitize_observation_file(
    path: Path,
    replacements: tuple[tuple[str, str], ...],
) -> int:
    """Sanitize one regular JSON-lines observation file.

    Returns:
        The number of observations written.

    """
    try:
        mode = path.lstat().st_mode
    except OSError as error:
        message = f"cannot inspect Hypothesis observation file {path}: {error}"
        raise HypothesisArtifactError(message) from error
    if not stat.S_ISREG(mode) or path.suffix != ".jsonl":
        message = f"unexpected Hypothesis observation artifact: {path}"
        raise HypothesisArtifactError(message)
    try:
        lines = path.read_text(encoding=UTF8).splitlines()
    except (OSError, UnicodeError) as error:
        message = f"cannot read Hypothesis observations {path}: {error}"
        raise HypothesisArtifactError(message) from error
    public: list[observation_safety.JsonValue] = []
    for index, line in enumerate(lines, start=1):
        observation = _public_value(
            _load_observation(line, path, index),
            replacements,
            observation_root=True,
        )
        _reject_absolute_paths(observation, path, index)
        public.append(observation)
    content = "".join(
        f"{json.dumps(item, sort_keys=True, separators=(',', ':'))}\n"
        for item in public
    )
    for prefix, _replacement in replacements:
        if prefix in content:
            message = f"private path remains in Hypothesis observations {path}"
            raise HypothesisArtifactError(message)
    _atomic_write(path, content)
    return len(public)


def _sanitize_observations(
    root: Path,
    additional: tuple[tuple[Path, str], ...] = (),
) -> tuple[int, int]:
    """Sanitize the canonical observation directory without following links.

    Returns:
        The number of files and observations sanitized.

    """
    observed = root / ".hypothesis" / "observed"
    if _directory_mode(observed, OBSERVATIONS_LABEL) is None:
        return 0, 0
    try:
        paths = sorted(observed.iterdir())
    except OSError as error:
        message = f"cannot list Hypothesis observations {observed}: {error}"
        raise HypothesisArtifactError(message) from error
    replacements = _replacement_prefixes(root, additional)
    observations = sum(_sanitize_observation_file(path, replacements) for path in paths)
    return len(paths), observations


def finalize(
    root: Path = PROJECT_ROOT,
    *,
    observations: bool = False,
    additional_replacements: tuple[tuple[Path, str], ...] = (),
) -> tuple[int, int]:
    """Remove private caches and optionally sanitize public observations.

    Returns:
        The number of observation files and records sanitized.

    """
    root = root.resolve()
    _remove_local_only_directories(root)
    return (
        _sanitize_observations(root, additional_replacements)
        if observations
        else (0, 0)
    )


def run_and_finalize(
    action: Callable[[], None],
    root: Path = PROJECT_ROOT,
    *,
    observations: bool = False,
) -> None:
    """Run a test action and always finalize, retaining both failures."""
    try:
        action()
    finally:
        action_error = sys.exception()
        if action_error is None:
            finalize(root, observations=observations)
        else:
            _finalize_after_error(action_error, root, observations=observations)


def _finalize_after_error(
    action_error: BaseException,
    root: Path,
    *,
    observations: bool,
) -> None:
    """Finalize during exception unwinding and preserve a second failure.

    Raises:
        BaseExceptionGroup: If an interrupt and finalization both fail.
        ExceptionGroup: If a normal exception and finalization both fail.

    """
    try:
        finalize(root, observations=observations)
    except HypothesisArtifactError as finalization_error:
        if isinstance(action_error, Exception):
            raise ExceptionGroup(
                FINALIZATION_GROUP_MESSAGE,
                [action_error, finalization_error],
            ) from None
        raise BaseExceptionGroup(
            FINALIZATION_GROUP_MESSAGE,
            [action_error, finalization_error],
        ) from None


def main(argv: list[str] | None = None) -> int:
    """Finalize canonical artifacts and return a command-line status.

    Returns:
        Zero after all requested finalization succeeds.

    """
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument(
        "--observations",
        action="store_true",
        help=OBSERVATION_HELP,
    )
    arguments = parser.parse_args(argv)
    try:
        files, records = finalize(PROJECT_ROOT, observations=arguments.observations)
    except HypothesisArtifactError as error:
        print(error, file=sys.stderr)
        return 1
    print(f"Hypothesis artifacts finalized: files={files}, records={records}")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
