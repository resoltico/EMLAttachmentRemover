"""Load release-critical project metadata through strict runtime contracts."""

from __future__ import annotations

import re
import tomllib
from dataclasses import dataclass
from functools import partial
from pathlib import PurePosixPath
from typing import TYPE_CHECKING, Final

try:
    from .archive_surface_policy import DistributionArchiveError, safe_parts
except ImportError:
    from archive_surface_policy import (  # type: ignore[import-not-found,no-redef]
        DistributionArchiveError,
        safe_parts,
    )

if TYPE_CHECKING:
    from collections.abc import Mapping
    from pathlib import Path


_safe_file_parts: Final = partial(safe_parts, directory=False)


@dataclass(frozen=True, slots=True)
class ProjectArchiveMetadata:
    """Hold the validated project values that define release archives."""

    name: str
    version: str
    normalized_name: str
    package_roots: tuple[PurePosixPath, ...]
    license_files: tuple[PurePosixPath, ...]
    scripts: Mapping[str, str]
    summary: str
    requires_python: str
    wheel_tag: str
    wheel_generator: str
    license_expression: str
    authors: tuple[str, ...]
    keywords: tuple[str, ...]
    classifiers: tuple[str, ...]
    dependencies: tuple[str, ...]
    urls: Mapping[str, str]
    readme: PurePosixPath


@dataclass(frozen=True, slots=True)
class _ProjectTables:
    """Hold required nested TOML tables after runtime validation."""

    project: Mapping[str, object]
    build_system: Mapping[str, object]
    runtime: Mapping[str, object]
    wheel: Mapping[str, object]


def load_project_metadata(
    project_config: Path,
    public_sources: Mapping[str, Path],
) -> ProjectArchiveMetadata:
    """Load and validate every release-critical project setting.

    Returns:
        A fully typed immutable metadata contract.

    Raises:
        DistributionArchiveError: If required release metadata is unsafe.

    """
    with project_config.open("rb") as configuration_file:
        configuration = tomllib.load(configuration_file)
    tables = _project_tables(configuration)
    name = _required_text(tables.project, "name")
    requires_python = _required_text(tables.project, "requires-python")
    requirements = _required_string_list(tables.build_system, "requires")
    hatchling_version = _hatchling_version(
        requirements,
        _required_text(tables.build_system, "build-backend"),
    )
    package_paths = _required_string_list(tables.wheel, "packages")
    if not package_paths:
        message = "project packages must contain at least one public package root"
        raise DistributionArchiveError(message)
    return ProjectArchiveMetadata(
        name=name,
        version=_required_text(tables.project, "version"),
        normalized_name=re.sub(r"[-_.]+", "_", name),
        package_roots=_validated_paths(package_paths),
        license_files=_validated_paths(
            _optional_string_list(tables.project, "license-files"),
        ),
        scripts=_scripts(tables.project),
        summary=_required_text(tables.project, "description"),
        requires_python=requires_python,
        wheel_tag=_wheel_tag(
            _required_text(tables.runtime, "implementation"),
            requires_python,
        ),
        wheel_generator=f"hatchling {hatchling_version}",
        license_expression=_required_text(tables.project, "license"),
        authors=_authors(tables.project),
        keywords=_optional_string_list(tables.project, "keywords"),
        classifiers=_optional_string_list(tables.project, "classifiers"),
        dependencies=_optional_string_list(tables.project, "dependencies"),
        urls=_urls(tables.project),
        readme=_readme(tables.project, public_sources),
    )


def normalized_requirement(value: str) -> str:
    """Return a comparison form for equivalent specifier ordering.

    Returns:
        Lexically sorted comma-separated requirement clauses.

    """
    return ",".join(sorted(part.strip() for part in value.split(",")))


def _project_tables(configuration: Mapping[str, object]) -> _ProjectTables:
    project = _required_table(configuration, "project")
    build_system = _required_table(configuration, "build-system")
    tool = _required_table(configuration, "tool")
    remover = _required_table(tool, "eml-attachment-remover")
    runtime = _required_table(remover, "runtime")
    hatch = _required_table(tool, "hatch")
    build = _required_table(hatch, "build")
    targets = _required_table(build, "targets")
    wheel = _required_table(targets, "wheel")
    return _ProjectTables(project, build_system, runtime, wheel)


def _required_table(
    parent: Mapping[str, object],
    field: str,
) -> Mapping[str, object]:
    value = parent.get(field)
    if not isinstance(value, dict):
        message = f"project configuration field {field!r} must be a table"
        raise DistributionArchiveError(message)
    return value


def _required_text(parent: Mapping[str, object], field: str) -> str:
    value = parent.get(field)
    if not isinstance(value, str):
        message = f"project configuration field {field!r} must be text"
        raise DistributionArchiveError(message)
    return value


def _required_string_list(
    parent: Mapping[str, object],
    field: str,
) -> tuple[str, ...]:
    value = parent.get(field)
    if not isinstance(value, list) or not all(isinstance(item, str) for item in value):
        message = f"project configuration field {field!r} must be a string list"
        raise DistributionArchiveError(message)
    return tuple(value)


def _optional_string_list(
    parent: Mapping[str, object],
    field: str,
) -> tuple[str, ...]:
    value = parent.get(field, [])
    if not isinstance(value, list) or not all(isinstance(item, str) for item in value):
        message = f"project {field} must be a string list"
        raise DistributionArchiveError(message)
    return tuple(value)


def _validated_paths(values: tuple[str, ...]) -> tuple[PurePosixPath, ...]:
    """Return project paths after validating their unnormalized text.

    Returns:
        Exact portable file paths without ambiguous textual spellings.

    """
    for value in values:
        _safe_file_parts(value)
    return tuple(PurePosixPath(value) for value in values)


def _scripts(project: Mapping[str, object]) -> Mapping[str, str]:
    configured = project.get("scripts", {})
    if not isinstance(configured, dict):
        message = "project scripts must be a text-to-text table"
        raise DistributionArchiveError(message)
    scripts: dict[str, str] = {}
    for name, target in configured.items():
        if not isinstance(target, str):
            message = "project scripts must be a text-to-text table"
            raise DistributionArchiveError(message)
        scripts[name] = target
    return scripts


def _urls(project: Mapping[str, object]) -> Mapping[str, str]:
    configured = _required_table(project, "urls")
    if not configured:
        message = "project URLs must contain at least one public destination"
        raise DistributionArchiveError(message)
    urls: dict[str, str] = {}
    for label, url in configured.items():
        if not isinstance(url, str):
            message = "project URLs must be a text-to-text table"
            raise DistributionArchiveError(message)
        urls[label] = url
    return urls


def _authors(project: Mapping[str, object]) -> tuple[str, ...]:
    configured = project.get("authors", [])
    if not isinstance(configured, list):
        message = "project authors must be a list"
        raise DistributionArchiveError(message)
    authors: list[str] = []
    for author in configured:
        if not isinstance(author, dict):
            message = "each project author must be a table"
            raise DistributionArchiveError(message)
        name = author.get("name")
        if name is None:
            continue
        if not isinstance(name, str):
            message = "each project author name must be text"
            raise DistributionArchiveError(message)
        authors.append(name)
    return tuple(authors)


def _readme(
    project: Mapping[str, object],
    public_sources: Mapping[str, Path],
) -> PurePosixPath:
    configured = project.get("readme")
    if not isinstance(configured, str):
        message = "project readme must be one static public path"
        raise DistributionArchiveError(message)
    _safe_file_parts(configured)
    readme = PurePosixPath(configured)
    if readme.suffix.casefold() != ".md" or readme.as_posix() not in public_sources:
        message = "project readme must be an audited Markdown source"
        raise DistributionArchiveError(message)
    return readme


def _hatchling_version(requirements: tuple[str, ...], backend: str) -> str:
    hatchling = [item for item in requirements if item.startswith("hatchling")]
    pinned = (
        re.fullmatch(r"hatchling==([A-Za-z0-9][A-Za-z0-9.!+_-]*)", hatchling[0])
        if len(hatchling) == 1
        else None
    )
    if backend != "hatchling.build" or pinned is None:
        message = "release backend must be one exactly pinned Hatchling requirement"
        raise DistributionArchiveError(message)
    return pinned.group(1)


def _wheel_tag(implementation: str, requirement: str) -> str:
    match = re.fullmatch(r">=(\d+)\.(\d+),<(\d+)\.(\d+)", requirement)
    if match is None or implementation != "CPython":
        message = "release wheel requires exact CPython major/minor bounds"
        raise DistributionArchiveError(message)
    lower_major, lower_minor, upper_major, upper_minor = map(int, match.groups())
    if (upper_major, upper_minor) != (lower_major, lower_minor + 1):
        message = "release wheel requires one supported CPython minor version"
        raise DistributionArchiveError(message)
    return f"cp{lower_major}{lower_minor}-none-any"
