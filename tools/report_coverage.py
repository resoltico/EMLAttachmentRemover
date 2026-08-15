"""Render strict text and XML coverage reports without leaking database handles."""

from __future__ import annotations

import argparse
import importlib
import sys
import tempfile
from contextlib import ExitStack
from pathlib import Path
from typing import TYPE_CHECKING, Final, cast

from coverage import Coverage
from coverage.results import display_covered, should_fail_under

if TYPE_CHECKING:
    from typing import Protocol

    from coverage.sqldata import CoverageData

    class CoverageXmlPublication(Protocol):
        """Describe validated Coverage XML publication operations."""

        def load_validated(self, source: Path) -> bytes:
            """Read and validate private staged XML."""

        def publish(self, content: bytes, destination: Path) -> Path:
            """Atomically publish validated XML."""


coverage_xml = cast(
    "CoverageXmlPublication",
    importlib.import_module(
        "tools.coverage_xml_publication"
        if __package__ or str(Path(__file__).resolve().parents[1]) in sys.path
        else "coverage_xml_publication"
    ),
)

PROJECT_ROOT: Final = Path(__file__).resolve().parents[1]
PROJECT_CONFIG: Final = PROJECT_ROOT / "pyproject.toml"
DEFAULT_XML_OUTPUT: Final = PROJECT_ROOT / "build" / "coverage.xml"
PRIVATE_XML_PREFIX: Final = "eml-attachment-remover-coverage-xml-"
PRIVATE_XML_NAME: Final = "coverage.xml"


class CoverageThresholdError(RuntimeError):
    """Report coverage below the configured precision-aware threshold."""


def _threshold_options(reporter: Coverage) -> tuple[float, int]:
    """Return validated numeric threshold options from Coverage.py.

    Returns:
        The fail-under percentage and display precision.

    Raises:
        TypeError: If Coverage.py supplies an unexpected option type.

    """
    fail_under = reporter.get_option("report:fail_under")
    precision = reporter.get_option("report:precision")
    if not isinstance(fail_under, (int, float)) or isinstance(fail_under, bool):
        message = "coverage report:fail_under must be numeric"
        raise TypeError(message)
    if not isinstance(precision, int) or isinstance(precision, bool):
        message = "coverage report:precision must be an integer"
        raise TypeError(message)
    return float(fail_under), precision


def _build_parser() -> argparse.ArgumentParser:
    """Create the strict coverage-report command-line parser.

    Returns:
        The configured argument parser.

    """
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument(
        "--data-file",
        type=Path,
        required=True,
        help="combined coverage database from ephemeral task storage",
    )
    parser.add_argument(
        "--xml-output",
        type=Path,
        default=DEFAULT_XML_OUTPUT,
        help=f"Cobertura XML report (default: {DEFAULT_XML_OUTPUT})",
    )
    return parser


def _threshold_error(reporter: Coverage, total: float) -> CoverageThresholdError | None:
    """Return the configured threshold failure for a measured total, if any.

    Returns:
        A precision-aware failure, or ``None`` when the threshold is met.

    """
    fail_under, precision = _threshold_options(reporter)
    if not should_fail_under(total, fail_under, precision):
        return None
    displayed = display_covered(total, precision)
    message = f"total of {displayed} is less than fail-under={fail_under:.{precision}f}"
    return CoverageThresholdError(message)


def _register_data_handle(
    reporter: Coverage,
    stack: ExitStack,
    registered: list[CoverageData],
) -> None:
    """Register the reporter's current data handle for unconditional closure."""
    data = reporter.get_data()
    if any(data is existing for existing in registered):
        return
    registered.append(data)
    stack.callback(data.close, force=True)


def report_coverage(data_file: Path, xml_output: Path) -> float:
    """Print text, write XML, enforce the threshold, and close coverage data.

    Returns:
        The measured total percentage when the configured threshold passes.

    Raises:
        CoverageThresholdError: If the configured coverage threshold is missed.

    """
    reporter = Coverage(
        data_file=str(data_file),
        config_file=str(PROJECT_CONFIG),
    )
    registered: list[CoverageData] = []
    with ExitStack() as close_data:
        _register_data_handle(reporter, close_data, registered)
        reporter.load()
        try:
            total = reporter.report()
        finally:
            _register_data_handle(reporter, close_data, registered)
        threshold_error = _threshold_error(reporter, total)
        if threshold_error is not None:
            raise CoverageThresholdError(str(threshold_error))
        with tempfile.TemporaryDirectory(
            dir=data_file.parent,
            prefix=PRIVATE_XML_PREFIX,
        ) as private_directory:
            staged_xml = Path(private_directory) / PRIVATE_XML_NAME
            try:
                reporter.xml_report(outfile=str(staged_xml))
            finally:
                _register_data_handle(reporter, close_data, registered)
            content = coverage_xml.load_validated(staged_xml)
        coverage_xml.publish(content, xml_output)
        return total


def main(argv: list[str] | None = None) -> int:
    """Render both configured reports and enforce the configured threshold.

    Returns:
        Zero when coverage passes, or two when it misses ``fail_under``.

    """
    arguments = _build_parser().parse_args(argv)
    try:
        report_coverage(arguments.data_file, arguments.xml_output)
    except CoverageThresholdError as error:
        print(f"Coverage failure: {error}", file=sys.stderr)
        return 2
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
