"""Validate the exact public Coverage.py Cobertura XML contract."""

from __future__ import annotations

import re
from decimal import Decimal, InvalidOperation
from typing import TYPE_CHECKING, Final

import coverage

if __package__:
    from tools.coverage_xml_lines import CoverageTotals, validate_line
    from tools.coverage_xml_safety import (
        CoverageXmlError,
        require_public_root,
        require_relative_path,
    )
else:
    from coverage_xml_lines import CoverageTotals, validate_line  # type: ignore[import-not-found,no-redef]  # ruff: ignore[unsorted-imports]
    from coverage_xml_safety import (  # type: ignore[import-not-found,no-redef]
        CoverageXmlError,
        require_public_root,
        require_relative_path,
    )

if TYPE_CHECKING:
    from collections.abc import Collection
    from xml.etree.ElementTree import Element

__all__ = ["CoverageXmlError", "validate"]

EXPECTED_SOURCES: Final = ("src/eml_attachment_remover", "tools")

ROOT_ATTRIBUTES: Final = frozenset({
    "branch-rate",
    "branches-covered",
    "branches-valid",
    "complexity",
    "line-rate",
    "lines-covered",
    "lines-valid",
    "timestamp",
    "version",
})
PACKAGE_ATTRIBUTES: Final = frozenset({
    "branch-rate",
    "complexity",
    "line-rate",
    "name",
})
CLASS_ATTRIBUTES: Final = frozenset({
    "branch-rate",
    "complexity",
    "filename",
    "line-rate",
    "name",
})
UNSIGNED_INTEGER: Final = re.compile(r"(?:0|[1-9][0-9]*)\Z")


def _require_attributes(element: Element, expected: Collection[str]) -> None:
    """Require one element's exact expected attribute names.

    Raises:
        CoverageXmlError: If an attribute is missing or unexpected.

    """
    if set(element.attrib) != set(expected):
        message = f"invalid {element.tag} attributes"
        raise CoverageXmlError(message)


def _require_children(element: Element, names: tuple[str, ...]) -> list[Element]:
    """Require exact child order and whitespace-only container layout.

    Returns:
        The validated direct children.

    Raises:
        CoverageXmlError: If child elements or layout text are unexpected.

    """
    children = list(element)
    if tuple(child.tag for child in children) != names:
        message = f"invalid {element.tag} children"
        raise CoverageXmlError(message)
    if element.text is not None and element.text.strip():
        message = f"unexpected {element.tag} text"
        raise CoverageXmlError(message)
    if element.tail is not None and element.tail.strip():
        message = f"unexpected {element.tag} tail"
        raise CoverageXmlError(message)
    return children


def _require_nonnegative(value: str, field: str) -> int:
    """Require and return one canonical nonnegative integer.

    Returns:
        The parsed integer.

    Raises:
        CoverageXmlError: If the value is not canonical and nonnegative.

    """
    if UNSIGNED_INTEGER.fullmatch(value) is None:
        message = f"invalid {field}"
        raise CoverageXmlError(message)
    return int(value)


def _require_rate(
    value: str,
    field: str,
    totals: tuple[int, int] | None = None,
) -> None:
    """Require a finite and, with ``(covered, valid)`` totals, exact rate.

    Raises:
        CoverageXmlError: If the rate is invalid or inconsistent.

    """
    try:
        rate = Decimal(value)
    except InvalidOperation as error:
        message = f"invalid {field}"
        raise CoverageXmlError(message) from error
    if not rate.is_finite() or rate < 0 or rate > 1:
        message = f"invalid {field}"
        raise CoverageXmlError(message)
    if totals is not None:
        covered, valid = totals
        expected = "1" if valid == 0 else f"{covered / valid:.4g}"
        if value != expected:
            message = f"inconsistent {field}"
            raise CoverageXmlError(message)


def _validate_root(root: Element) -> tuple[Element, Element]:
    """Validate metadata and return sources and packages containers.

    Returns:
        The sources and packages elements.

    Raises:
        CoverageXmlError: If root structure or metadata is invalid.

    """
    if root.tag != "coverage":
        message = "root is not coverage"
        raise CoverageXmlError(message)
    _require_attributes(root, ROOT_ATTRIBUTES)
    lines_valid = _require_nonnegative(root.attrib["lines-valid"], "lines-valid")
    lines_covered = _require_nonnegative(root.attrib["lines-covered"], "lines-covered")
    branches_valid = _require_nonnegative(
        root.attrib["branches-valid"], "branches-valid"
    )
    branches_covered = _require_nonnegative(
        root.attrib["branches-covered"], "branches-covered"
    )
    if lines_covered > lines_valid or branches_covered > branches_valid:
        message = "covered total exceeds valid total"
        raise CoverageXmlError(message)
    for field in ("line-rate", "branch-rate"):
        _require_rate(root.attrib[field], field)
    if root.attrib["complexity"] != "0":
        message = "invalid complexity"
        raise CoverageXmlError(message)
    if UNSIGNED_INTEGER.fullmatch(root.attrib["timestamp"]) is None:
        message = "invalid timestamp"
        raise CoverageXmlError(message)
    if root.attrib["version"] != coverage.__version__:
        message = "invalid version"
        raise CoverageXmlError(message)
    sources, packages = _require_children(root, ("sources", "packages"))
    return sources, packages


def _validate_sources(sources: Element) -> None:
    """Validate a nonempty, unique set of relative source roots.

    Raises:
        CoverageXmlError: If a source root or its container is invalid.

    """
    _require_attributes(sources, ())
    if sources.text is not None and sources.text.strip():
        message = "unexpected sources text"
        raise CoverageXmlError(message)
    if sources.tail is not None and sources.tail.strip():
        message = "unexpected sources tail"
        raise CoverageXmlError(message)
    children = list(sources)
    if not children or any(child.tag != "source" for child in children):
        message = "invalid sources children"
        raise CoverageXmlError(message)
    values: list[str] = []
    for source in children:
        _require_attributes(source, ())
        if list(source) or source.text is None:
            message = "invalid source content"
            raise CoverageXmlError(message)
        if source.tail is not None and source.tail.strip():
            message = "unexpected source tail"
            raise CoverageXmlError(message)
        require_relative_path(source.text, "source", required_suffix=None)
        values.append(source.text)
    if tuple(values) != EXPECTED_SOURCES:
        message = "sources do not match configured coverage scopes"
        raise CoverageXmlError(message)


def _validate_class(class_element: Element) -> tuple[str, CoverageTotals]:
    """Validate one class and return its filename and totals.

    Returns:
        The unique relative filename and aggregate class totals.

    Raises:
        CoverageXmlError: If class structure or metadata is invalid.

    """
    _require_attributes(class_element, CLASS_ATTRIBUTES)
    methods, lines = _require_children(class_element, ("methods", "lines"))
    _require_attributes(methods, ())
    _require_children(methods, ())
    _require_attributes(lines, ())
    if lines.text is not None and lines.text.strip():
        message = "unexpected lines text"
        raise CoverageXmlError(message)
    if lines.tail is not None and lines.tail.strip():
        message = "unexpected lines tail"
        raise CoverageXmlError(message)
    if any(line.tag != "line" for line in lines):
        message = "invalid lines children"
        raise CoverageXmlError(message)
    line_elements = list(lines)
    numbers = [line.attrib["number"] for line in line_elements]
    if len(numbers) != len(set(numbers)):
        message = "duplicate line number"
        raise CoverageXmlError(message)
    totals = CoverageTotals.combine(validate_line(line) for line in line_elements)
    _require_rate(
        class_element.attrib["line-rate"],
        "class line-rate",
        (totals.lines_covered, totals.lines_valid),
    )
    _require_rate(
        class_element.attrib["branch-rate"],
        "class branch-rate",
        (totals.branches_covered, totals.branches_valid),
    )
    if class_element.attrib["complexity"] != "0":
        message = "invalid class complexity"
        raise CoverageXmlError(message)
    if not class_element.attrib["name"].strip():
        message = "invalid class name"
        raise CoverageXmlError(message)
    filename = class_element.attrib["filename"]
    require_relative_path(filename, "filename", required_suffix=".py")
    return filename, totals


def _validate_package(package: Element) -> tuple[str, tuple[str, ...], CoverageTotals]:
    """Validate one package and return identity and totals.

    Returns:
        The package name, class filenames, and aggregate totals.

    Raises:
        CoverageXmlError: If package structure or metadata is invalid.

    """
    _require_attributes(package, PACKAGE_ATTRIBUTES)
    (classes,) = _require_children(package, ("classes",))
    _require_attributes(classes, ())
    if classes.text is not None and classes.text.strip():
        message = "unexpected classes text"
        raise CoverageXmlError(message)
    if classes.tail is not None and classes.tail.strip():
        message = "unexpected classes tail"
        raise CoverageXmlError(message)
    class_elements = list(classes)
    if not class_elements or any(item.tag != "class" for item in class_elements):
        message = "invalid classes children"
        raise CoverageXmlError(message)
    records = tuple(_validate_class(item) for item in class_elements)
    filenames = tuple(record[0] for record in records)
    totals = CoverageTotals.combine(record[1] for record in records)
    _require_rate(package.attrib["line-rate"], "package line-rate")
    _require_rate(package.attrib["branch-rate"], "package branch-rate")
    if package.attrib["complexity"] != "0":
        message = "invalid package complexity"
        raise CoverageXmlError(message)
    name = package.attrib["name"]
    if not name.strip() or any(character in name for character in "/\\\x00"):
        message = "invalid package name"
        raise CoverageXmlError(message)
    return name, filenames, totals


def _validate_packages(packages: Element) -> CoverageTotals:
    """Validate packages and return aggregate line and branch totals.

    Returns:
        Aggregate totals for every class in every package.

    Raises:
        CoverageXmlError: If packages are absent, duplicated, or invalid.

    """
    _require_attributes(packages, ())
    if packages.text is not None and packages.text.strip():
        message = "unexpected packages text"
        raise CoverageXmlError(message)
    if packages.tail is not None and packages.tail.strip():
        message = "unexpected packages tail"
        raise CoverageXmlError(message)
    package_elements = list(packages)
    if not package_elements or any(item.tag != "package" for item in package_elements):
        message = "invalid packages children"
        raise CoverageXmlError(message)
    records = tuple(_validate_package(package) for package in package_elements)
    names = tuple(record[0] for record in records)
    filenames = tuple(filename for record in records for filename in record[1])
    if len(names) != len(set(names)) or len(filenames) != len(set(filenames)):
        message = "duplicate package or filename"
        raise CoverageXmlError(message)
    return CoverageTotals.combine(record[2] for record in records)


def validate(content: bytes) -> None:
    """Require the exact safe Coverage.py Cobertura document contract.

    Raises:
        CoverageXmlError: If any XML, privacy, schema, or consistency check fails.

    """
    root = require_public_root(content)
    sources, packages = _validate_root(root)
    _validate_sources(sources)
    totals = _validate_packages(packages)
    reported = CoverageTotals(
        int(root.attrib["lines-valid"]),
        int(root.attrib["lines-covered"]),
        int(root.attrib["branches-valid"]),
        int(root.attrib["branches-covered"]),
    )
    hidden = (
        (
            reported.lines_valid - totals.lines_valid,
            reported.lines_covered - totals.lines_covered,
        ),
        (
            reported.branches_valid - totals.branches_valid,
            reported.branches_covered - totals.branches_covered,
        ),
    )
    if any(valid < 0 or covered < 0 or covered > valid for valid, covered in hidden):
        message = "root totals cannot reconcile with line records"
        raise CoverageXmlError(message)
    _require_rate(
        root.attrib["line-rate"],
        "line-rate",
        (reported.lines_covered, reported.lines_valid),
    )
    _require_rate(
        root.attrib["branch-rate"],
        "branch-rate",
        (reported.branches_covered, reported.branches_valid),
    )
