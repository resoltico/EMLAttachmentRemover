"""Derive the CPython-specific pure-Python wheel tag from project metadata."""

from __future__ import annotations

import re
import tomllib
from pathlib import Path
from typing import Any, Final, override

from hatchling.builders.config import BuilderConfig
from hatchling.builders.hooks.plugin.interface import BuildHookInterface

REQUIRES_PATTERN: Final = re.compile(r">=(\d+)\.(\d+),<(\d+)\.(\d+)")


def wheel_tag(project_config: Path) -> str:
    """Return the exact CPython wheel tag derived from canonical metadata.

    Returns:
        A CPython-major/minor, ABI-independent, platform-independent tag.

    Raises:
        ValueError: If the runtime declaration cannot produce one safe tag.

    """
    with project_config.open("rb") as config_file:
        configuration = tomllib.load(config_file)
    requires_python = str(configuration["project"]["requires-python"])
    runtime = configuration["tool"]["eml-attachment-remover"]["runtime"]
    implementation = str(runtime["implementation"])
    match = REQUIRES_PATTERN.fullmatch(requires_python)
    if match is None or implementation != "CPython":
        message = "wheel tag requires exact CPython major/minor project bounds"
        raise ValueError(message)
    lower_major, lower_minor, upper_major, upper_minor = map(int, match.groups())
    if (upper_major, upper_minor) != (lower_major, lower_minor + 1):
        message = "wheel tag requires one supported CPython minor version"
        raise ValueError(message)
    return f"cp{lower_major}{lower_minor}-none-any"


class CustomBuildHook(BuildHookInterface[BuilderConfig]):
    """Set a precise wheel tag before Hatchling constructs the archive."""

    @override
    def initialize(self, version: str, build_data: dict[str, Any]) -> None:
        """Apply the metadata-derived tag to standard and editable wheels."""
        build_data["tag"] = wheel_tag(Path(self.root) / "pyproject.toml")
