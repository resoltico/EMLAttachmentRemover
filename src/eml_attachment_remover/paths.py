"""Validate source and destination filesystem paths."""

from __future__ import annotations

import ntpath
import os
import platform
import stat
import unicodedata
from pathlib import Path

from .models import CliError, ExitCode


def _default_destination(source: Path) -> Path:
    """Derive a non-destructive output filename beside the source.

    Returns:
        The default derived EML path.

    """
    if source.suffix.casefold() == ".eml":
        return source.with_name(f"{source.stem}.attachments-removed{source.suffix}")
    return source.with_name(f"{source.name}.attachments-removed.eml")


def _absolute_path(path: Path) -> Path:
    """Return a normalized absolute path without resolving symbolic links.

    Returns:
        The expanded path with lexical dot segments removed.

    """
    expanded = path.expanduser()
    absolute = expanded if expanded.is_absolute() else Path.cwd() / expanded
    return Path(os.path.normpath(absolute))


def _validate_source(source: Path) -> Path:
    """Validate the source and return its absolute path.

    Returns:
        The validated absolute source path.

    Raises:
        CliError: If the source does not exist or is not a regular file.

    """
    try:
        source = _absolute_path(source)
        source_stat = source.stat()
    except FileNotFoundError as exc:
        raise CliError(
            ExitCode.INPUT_ERROR,
            f"input file does not exist: {source}",
        ) from exc
    except (OSError, RuntimeError) as exc:
        raise CliError(
            ExitCode.INPUT_ERROR,
            f"could not inspect input path {source}: {exc}",
        ) from exc
    if not stat.S_ISREG(source_stat.st_mode):
        raise CliError(
            ExitCode.INPUT_ERROR,
            f"input path is not a regular file: {source}",
        )
    return source


def _destination_exists(destination: Path) -> bool:
    """Return whether a destination entry exists, including a symbolic link.

    Returns:
        ``True`` when a directory entry exists.

    Raises:
        CliError: If the destination cannot be inspected.

    """
    try:
        destination.lstat()
    except FileNotFoundError:
        return False
    except OSError as exc:
        raise CliError(
            ExitCode.WRITE_ERROR,
            f"could not inspect output path {destination}: {exc}",
        ) from exc
    return True


def _paths_alias(source: Path, destination: Path) -> bool:
    """Return whether two existing paths identify the same file.

    Returns:
        ``True`` for the same file, hard link, or resolving symbolic-link alias.

    """
    try:
        return source.samefile(destination)
    except FileNotFoundError, OSError:
        return False


def _validate_destination_parent(destination: Path) -> None:
    """Require the destination parent to be an existing directory.

    Raises:
        CliError: If the parent does not exist or is not a directory.

    """
    parent = destination.parent
    try:
        parent_stat = parent.stat()
    except FileNotFoundError as exc:
        raise CliError(
            ExitCode.WRITE_ERROR,
            f"output directory does not exist: {parent}",
        ) from exc
    except OSError as exc:
        raise CliError(
            ExitCode.WRITE_ERROR,
            f"could not inspect output directory {parent}: {exc}",
        ) from exc
    if not stat.S_ISDIR(parent_stat.st_mode):
        raise CliError(
            ExitCode.WRITE_ERROR,
            f"output parent is not a directory: {parent}",
        )


def _canonical_destination(destination: Path) -> Path:
    """Bind an output to its existing canonical parent directory.

    Returns:
        The resolved parent joined to the untouched final output name.

    Raises:
        CliError: If the parent cannot be resolved safely.

    """
    _validate_destination_parent(destination)
    try:
        parent = destination.parent.resolve(strict=True)
    except (OSError, RuntimeError) as exc:
        raise CliError(
            ExitCode.WRITE_ERROR,
            f"could not resolve output directory {destination.parent}: {exc}",
        ) from exc
    return parent / destination.name


def _validate_paths(
    source: Path,
    destination: Path,
    *,
    force: bool,
) -> tuple[Path, Path]:
    """Validate source and destination paths before writing.

    Returns:
        The absolute validated source and destination paths.

    Raises:
        CliError: If input, output, aliasing, or parent-directory checks fail.

    """
    source = _validate_source(source)
    try:
        destination = _absolute_path(destination)
    except (OSError, RuntimeError) as exc:
        raise CliError(
            ExitCode.WRITE_ERROR,
            f"could not resolve output path {destination}: {exc}",
        ) from exc
    destination = _canonical_destination(destination)
    if source == destination or _paths_alias(source, destination):
        raise CliError(
            ExitCode.OUTPUT_CONFLICT,
            "refusing to overwrite the source EML directly or through an alias",
        )
    destination_exists = _destination_exists(destination)
    if destination_exists and not force:
        raise CliError(
            ExitCode.OUTPUT_CONFLICT,
            f"output already exists: {destination}; use --force to replace it",
        )
    if destination_exists and destination.is_dir():
        raise CliError(
            ExitCode.OUTPUT_CONFLICT,
            f"output path is a directory: {destination}",
        )
    return source, destination


def _validate_output_directory(output_directory: Path) -> Path:
    """Validate a shared batch output directory.

    Returns:
        The absolute directory path.

    Raises:
        CliError: If the path does not exist or is not a directory.

    """
    try:
        absolute = _absolute_path(output_directory)
        metadata = absolute.stat()
    except FileNotFoundError as exc:
        raise CliError(
            ExitCode.WRITE_ERROR,
            f"output directory does not exist: {output_directory}",
        ) from exc
    except OSError as exc:
        raise CliError(
            ExitCode.WRITE_ERROR,
            f"could not inspect output directory {output_directory}: {exc}",
        ) from exc
    if not stat.S_ISDIR(metadata.st_mode):
        raise CliError(
            ExitCode.WRITE_ERROR,
            f"output path is not a directory: {absolute}",
        )
    try:
        return absolute.resolve(strict=True)
    except (OSError, RuntimeError) as exc:
        raise CliError(
            ExitCode.WRITE_ERROR,
            f"could not resolve output directory {output_directory}: {exc}",
        ) from exc


def _destination_for_source(source: Path, output_directory: Path | None) -> Path:
    """Derive a destination for one source and optional output directory.

    Returns:
        The beside-source default or a same-named path in the output directory.

    """
    default = _default_destination(source)
    return default if output_directory is None else output_directory / default.name


def _path_collision_key(path: Path) -> str:
    """Return a platform-aware key for detecting batch output collisions.

    Returns:
        A normalized path string suitable for conservative collision checks.

    """
    value = str(path)
    runtime_platform = platform.system()
    if runtime_platform == "Windows":
        return ntpath.normcase(value)
    if runtime_platform == "Darwin":
        return unicodedata.normalize("NFD", value).casefold()
    return value


def _output_collision_path(path: Path) -> Path:
    """Resolve only an output's parent for alias-aware collision checks.

    Returns:
        A path with symbolic parent aliases resolved and the final name untouched.

    """
    return _canonical_destination(path)


def _planned_outputs(
    sources: list[Path],
    explicit_output: Path | None,
    output_directory: Path | None,
) -> list[tuple[Path, Path]]:
    """Build and validate all source-to-destination mappings before writing.

    Returns:
        Ordered source and destination pairs.

    Raises:
        CliError: If destinations collide with each other or selected sources.

    """
    normalized_sources = [_absolute_path(source) for source in sources]
    _validate_single_batch(normalized_sources)
    if explicit_output is not None:
        plans = [(normalized_sources[0], _absolute_path(explicit_output))]
    else:
        plans = [
            (
                source,
                _absolute_path(_destination_for_source(source, output_directory)),
            )
            for source in normalized_sources
        ]
    normalized_destinations: dict[str, Path] = {}
    canonical_plans: list[tuple[Path, Path]] = []
    for source, destination in plans:
        normalized_destination = _absolute_path(destination)
        try:
            collision_path = _output_collision_path(normalized_destination)
        except (OSError, RuntimeError) as exc:
            raise CliError(
                ExitCode.WRITE_ERROR,
                f"could not inspect planned output path {destination}: {exc}",
            ) from exc
        key = _path_collision_key(collision_path)
        previous = normalized_destinations.get(key)
        if previous is None:
            previous = next(
                (
                    candidate
                    for candidate in normalized_destinations.values()
                    if _paths_alias(candidate, collision_path)
                ),
                None,
            )
        if previous is not None:
            raise CliError(
                ExitCode.OUTPUT_CONFLICT,
                "multiple inputs resolve to the same output path: "
                f"{previous} and {destination}",
            )
        normalized_destinations[key] = collision_path
        canonical_plans.append((source, collision_path))
        for selected_source in normalized_sources:
            if _paths_alias_if_present(
                selected_source,
                collision_path,
            ):
                raise CliError(
                    ExitCode.OUTPUT_CONFLICT,
                    "refusing to use a selected source file as a batch output: "
                    f"{collision_path}",
                )
    return canonical_plans


def _validate_single_batch(sources: list[Path]) -> None:
    """Fail a one-file invocation before inspecting its output path."""
    if len(sources) == 1:
        _validate_source(sources[0])


def _paths_alias_if_present(source: Path, destination: Path) -> bool:
    """Compare a source and output only when the source currently exists.

    Returns:
        ``True`` for a lexical or filesystem alias.

    """
    try:
        source.lstat()
    except OSError:
        return False
    return source == destination or _paths_alias(source, destination)
