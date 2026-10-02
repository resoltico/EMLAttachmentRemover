"""Read the source-controlled native application build number."""

from __future__ import annotations

import tomllib
from typing import TYPE_CHECKING, Final

if TYPE_CHECKING:
    from pathlib import Path

MAX_BUILD_NUMBER: Final = 9999


def build_number(configuration: Path) -> str:
    """Return the numeric bundle build version from canonical project metadata.

    Returns:
        The validated build number as a bundle-compatible string.

    Raises:
        ValueError: If the required build number is absent or invalid.

    """
    with configuration.open("rb") as source:
        metadata = tomllib.load(source)
    try:
        value = metadata["tool"]["eml-attachment-remover"]["macos"]["build-number"]
    except KeyError as error:
        message = "Declare tool.eml-attachment-remover.macos.build-number"
        raise ValueError(message) from error
    if type(value) is not int or not 1 <= value <= MAX_BUILD_NUMBER:
        message = "Native build-number must be an integer from 1 to 9999"
        raise ValueError(message)
    return str(value)
