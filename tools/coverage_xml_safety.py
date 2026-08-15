"""Parse Coverage XML safely and reject private or absolute path content."""

from __future__ import annotations

import importlib
import re
from pathlib import PurePosixPath, PureWindowsPath
from typing import TYPE_CHECKING, Final, cast

from defusedxml import ElementTree
from defusedxml.common import DefusedXmlException

if TYPE_CHECKING:
    from typing import Protocol
    from xml.etree.ElementTree import Element

    class PrivacyPolicy(Protocol):
        """Describe the repository privacy check used at publication time."""

        def public_content_messages(self, text: str) -> tuple[str, ...]:
            """Return public-content violations."""


privacy_policy = cast(
    "PrivacyPolicy",
    importlib.import_module(
        "tools.repository_hygiene_policy"
        if __package__
        else "repository_hygiene_policy"
    ),
)

UTF8: Final = "utf-8"
FILE_URI: Final = re.compile(r"(?i)(?<![A-Za-z0-9])file:")
WINDOWS_ABSOLUTE: Final = re.compile(
    r"(?i)(?<![A-Za-z0-9])(?:[a-z]:[\\/]|\\{2}[^\\/\s]+[\\/])"
)
POSIX_ABSOLUTE: Final = re.compile(r"(?<![A-Za-z0-9_:</])/(?![/*])[^\s<>\"']+")
UNC_ABSOLUTE: Final = re.compile(r"(?<![A-Za-z0-9_:<])//[^/\s<>\"']+/[^\s<>\"']+")
PATH_PATTERNS: Final = (FILE_URI, WINDOWS_ABSOLUTE, POSIX_ABSOLUTE, UNC_ABSOLUTE)


class CoverageXmlError(ValueError):
    """Report malformed or machine-private Coverage XML evidence."""


def require_relative_path(
    value: str,
    field: str,
    *,
    required_suffix: str | None,
) -> None:
    """Require a normalized, portable, non-traversing relative POSIX path.

    Raises:
        CoverageXmlError: If the value is not a safe relative path.

    """
    posix = PurePosixPath(value)
    windows = PureWindowsPath(value)
    invalid = (
        not value
        or value == "."
        or value != value.strip()
        or "\\" in value
        or posix.is_absolute()
        or windows.is_absolute()
        or str(posix) != value
        or ".." in posix.parts
        or (required_suffix is not None and posix.suffix != required_suffix)
    )
    if invalid:
        message = f"invalid relative {field}"
        raise CoverageXmlError(message)


def require_public_root(content: bytes) -> Element:
    """Parse XML safely after rejecting machine-private content.

    Returns:
        The safe parsed document root.

    Raises:
        CoverageXmlError: If XML is malformed, unsafe, or machine-private.

    """
    try:
        text = content.decode(UTF8)
        root = ElementTree.fromstring(text)
    except (UnicodeError, ElementTree.ParseError, DefusedXmlException) as error:
        message = "Coverage XML is not safe, valid UTF-8 XML"
        raise CoverageXmlError(message) from error
    visible_values = "\n".join(
        value
        for element in root.iter()
        for value in (*element.attrib.values(), element.text, element.tail)
        if value is not None
    )
    privacy_issues = (
        *privacy_policy.public_content_messages(text),
        *privacy_policy.public_content_messages(visible_values),
    )
    absolute_path = any(
        pattern.search(candidate) is not None
        for pattern in PATH_PATTERNS
        for candidate in (text, visible_values)
    ) or any(value.strip() == "/" for value in visible_values.splitlines())
    if privacy_issues or absolute_path:
        message = "Coverage XML contains private identity or an absolute file path"
        raise CoverageXmlError(message)
    return root
