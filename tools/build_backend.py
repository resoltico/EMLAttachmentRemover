"""Use the invocation datetime with Hatchling's reproducible archive builder."""

from __future__ import annotations

import os
from pathlib import Path
from typing import Any

from hatchling.builders.sdist import SdistBuilder
from hatchling.builders.wheel import WheelBuilder

if __package__:
    from tools.build_timestamp import environment
else:
    from build_timestamp import environment  # type: ignore[import-not-found,no-redef]
from hatchling.build import (
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


def _build(directory: str, *, source: bool, editable: bool = False) -> str:
    """Resolve the dynamic license before Hatchling consumes source PKG-INFO.

    Returns:
        The generated artifact filename.

    """
    builder = SdistBuilder(str(Path.cwd())) if source else WheelBuilder(str(Path.cwd()))
    metadata = builder.metadata.core_raw_metadata
    licenses = builder.metadata.config["tool"]["eml-attachment-remover"]["licenses"]
    # Hatchling reuses the license from PKG-INFO even when it is marked Dynamic.
    # Resolve from the authoritative artifact scope for checkout and sdist builds.
    metadata["license"] = licenses["source" if source else "software"]
    metadata["dynamic"] = [
        field for field in metadata.get("dynamic", []) if field != "license"
    ]
    return Path(
        next(
            builder.build(
                directory=directory, versions=["editable" if editable else "standard"]
            )
        )
    ).name


def build_sdist(
    sdist_directory: str, config_settings: dict[str, Any] | None = None
) -> str:
    """Build source metadata with its software and artwork license.

    Returns:
        The generated source archive filename.

    """
    del config_settings
    return _build(sdist_directory, source=True)


def build_wheel(
    wheel_directory: str,
    config_settings: dict[str, Any] | None = None,
    metadata_directory: str | None = None,
) -> str:
    """Build a software-only wheel, including when invoked from a source archive.

    Returns:
        The generated wheel filename.

    """
    del config_settings, metadata_directory
    return _build(wheel_directory, source=False)


def build_editable(
    wheel_directory: str,
    config_settings: dict[str, Any] | None = None,
    metadata_directory: str | None = None,
) -> str:
    """Build an editable software installation with the same license scope.

    Returns:
        The generated editable wheel filename.

    """
    del config_settings, metadata_directory
    return _build(wheel_directory, source=False, editable=True)
