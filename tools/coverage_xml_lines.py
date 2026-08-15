"""Validate Coverage.py Cobertura line records and reconcile their totals."""

from __future__ import annotations

import re
from dataclasses import dataclass
from typing import TYPE_CHECKING, Final

if __package__:
    from tools.coverage_xml_safety import CoverageXmlError
else:
    from coverage_xml_safety import CoverageXmlError  # type: ignore[import-not-found,no-redef]  # ruff: ignore[unsorted-imports]

if TYPE_CHECKING:
    from collections.abc import Iterable
    from xml.etree.ElementTree import Element

LINE_REQUIRED_ATTRIBUTES: Final = frozenset({"hits", "number"})
LINE_BRANCH_ATTRIBUTES: Final = frozenset({
    "branch",
    "condition-coverage",
    "hits",
    "missing-branches",
    "number",
})
POSITIVE_INTEGER: Final = re.compile(r"[1-9][0-9]*\Z")
CONDITION_COVERAGE: Final = re.compile(
    r"(?P<percentage>100|[0-9]{1,2})% "
    r"\((?P<taken>[0-9]+)/(?P<total>[1-9][0-9]*)\)\Z"
)
MISSING_BRANCHES: Final = re.compile(
    r"(?:exit|[1-9][0-9]*)(?:,(?:exit|[1-9][0-9]*))*\Z"
)


@dataclass(frozen=True, slots=True)
class CoverageTotals:
    """Store internally reconcilable line and branch coverage totals."""

    lines_valid: int = 0
    lines_covered: int = 0
    branches_valid: int = 0
    branches_covered: int = 0

    @classmethod
    def combine(cls, values: Iterable[CoverageTotals]) -> CoverageTotals:
        """Combine any number of line and branch totals.

        Returns:
            The field-wise aggregate.

        """
        materialized = tuple(values)
        return cls(
            sum(value.lines_valid for value in materialized),
            sum(value.lines_covered for value in materialized),
            sum(value.branches_valid for value in materialized),
            sum(value.branches_covered for value in materialized),
        )


def _validate_branch(line: Element, hit: int) -> CoverageTotals:
    """Validate branch metadata and return its complete record totals.

    Returns:
        The record's one line and validated branch totals.

    Raises:
        CoverageXmlError: If branch metadata is invalid or inconsistent.

    """
    if line.attrib["branch"] != "true":
        message = "invalid branch marker"
        raise CoverageXmlError(message)
    match = CONDITION_COVERAGE.fullmatch(line.attrib["condition-coverage"])
    if match is None:
        message = "invalid condition coverage"
        raise CoverageXmlError(message)
    taken = int(match.group("taken"))
    total = int(match.group("total"))
    percentage = int(match.group("percentage"))
    if taken > total or percentage != 100 * taken // total:
        message = "inconsistent condition coverage"
        raise CoverageXmlError(message)
    missing = line.attrib.get("missing-branches")
    if missing is not None and MISSING_BRANCHES.fullmatch(missing) is None:
        message = "invalid missing branches"
        raise CoverageXmlError(message)
    missing_count = 0 if missing is None else len(missing.split(","))
    if missing_count != total - taken:
        message = "inconsistent missing branches"
        raise CoverageXmlError(message)
    return CoverageTotals(1, hit, total, taken)


def validate_line(line: Element) -> CoverageTotals:
    """Validate one line and return its line and branch totals.

    Returns:
        The record's line and branch totals.

    Raises:
        CoverageXmlError: If line structure or metadata is invalid.

    """
    attributes = set(line.attrib)
    branch = "branch" in attributes
    expected = LINE_REQUIRED_ATTRIBUTES
    if branch:
        expected = LINE_BRANCH_ATTRIBUTES
        if "missing-branches" not in attributes:
            expected -= {"missing-branches"}
    invalid_layout = (
        list(line)
        or (line.text is not None and bool(line.text.strip()))
        or (line.tail is not None and bool(line.tail.strip()))
    )
    if attributes != expected or invalid_layout:
        message = "invalid line structure"
        raise CoverageXmlError(message)
    if POSITIVE_INTEGER.fullmatch(line.attrib["number"]) is None:
        message = "invalid line number"
        raise CoverageXmlError(message)
    if line.attrib["hits"] not in {"0", "1"}:
        message = "invalid line hits"
        raise CoverageXmlError(message)
    hit = int(line.attrib["hits"])
    if branch:
        return _validate_branch(line, hit)
    return CoverageTotals(lines_valid=1, lines_covered=hit)
