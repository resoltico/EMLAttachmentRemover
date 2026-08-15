"""Behavioral tests for leak-free strict coverage reporting."""

from __future__ import annotations

import io
import tempfile
import unittest
from contextlib import nullcontext
from pathlib import Path
from unittest.mock import MagicMock, call, patch

from coverage.exceptions import ConfigError
from tools import coverage_xml_safety, report_coverage


def _realistic_xml() -> bytes:
    """Return a minimal internally consistent Coverage.py XML document.

    Returns:
        Valid public Cobertura XML bytes.

    """
    return b"""<?xml version="1.0" ?>
<coverage version="7.15.4" timestamp="1"
 lines-valid="1" lines-covered="1" line-rate="1"
 branches-valid="0" branches-covered="0" branch-rate="1" complexity="0">
  <sources>
    <source>src/eml_attachment_remover</source>
    <source>tools</source>
  </sources>
  <packages>
    <package name="." line-rate="1" branch-rate="1" complexity="0">
      <classes>
        <class name="public.py" filename="public.py" complexity="0"
         line-rate="1" branch-rate="1">
          <methods/>
          <lines><line number="1" hits="1"/></lines>
        </class>
      </classes>
    </package>
  </packages>
</coverage>
"""


def _configured_reporter(
    *,
    total: float = 100.0,
    fail_under: float = 100.0,
    precision: int = 0,
) -> tuple[MagicMock, MagicMock]:
    """Return a synthetic configured reporter and its data handle.

    Returns:
        The reporter and explicitly closable data handle.

    """
    reporter = MagicMock()
    data = MagicMock()
    reporter.get_data.return_value = data
    reporter.report.return_value = total
    reporter.xml_report.side_effect = lambda *, outfile: Path(outfile).write_bytes(
        _realistic_xml()
    )
    reporter.get_option.side_effect = {
        "report:fail_under": fail_under,
        "report:precision": precision,
    }.__getitem__
    return reporter, data


class CoverageThresholdTests(unittest.TestCase):
    """Exercise Coverage's exact threshold and display semantics."""

    def test_threshold_requires_a_literal_hundred(self) -> None:
        reporter, _data = _configured_reporter(total=99.999)
        failure = report_coverage._threshold_error(  # ruff: ignore[private-member-access]
            reporter,
            99.999,
        )
        self.assertIsInstance(failure, report_coverage.CoverageThresholdError)
        self.assertEqual(
            str(failure),
            "total of 99 is less than fail-under=100",
        )

    def test_threshold_uses_configured_rounding_precision(self) -> None:
        reporter, _data = _configured_reporter(
            total=98.6,
            fail_under=99.0,
            precision=0,
        )
        self.assertIsNone(
            report_coverage._threshold_error(reporter, 98.6),  # ruff: ignore[private-member-access]
        )

    def test_threshold_passes_the_exact_nonzero_precision_to_coverage(self) -> None:
        reporter, _data = _configured_reporter(
            total=99.94,
            fail_under=99.95,
            precision=1,
        )
        failure = report_coverage._threshold_error(  # ruff: ignore[private-member-access]
            reporter,
            99.94,
        )
        self.assertEqual(
            str(failure),
            "total of 99.9 is less than fail-under=100.0",
        )

    def test_threshold_rejects_invalid_configuration(self) -> None:
        reporter, _data = _configured_reporter(fail_under=101.0)
        with self.assertRaisesRegex(ConfigError, "between 0 and 100"):
            report_coverage._threshold_error(reporter, 100.0)  # ruff: ignore[private-member-access]

    def test_threshold_rejects_nonnumeric_coverage_options(self) -> None:
        for option, value, message in (
            (
                "report:fail_under",
                "100",
                "coverage report:fail_under must be numeric",
            ),
            (
                "report:fail_under",
                True,
                "coverage report:fail_under must be numeric",
            ),
            (
                "report:precision",
                1.5,
                "coverage report:precision must be an integer",
            ),
            (
                "report:precision",
                False,
                "coverage report:precision must be an integer",
            ),
        ):
            with self.subTest(option=option, value=value):
                reporter, _data = _configured_reporter()
                values: dict[str, object] = {
                    "report:fail_under": 100.0,
                    "report:precision": 0,
                }
                values[option] = value
                reporter.get_option.side_effect = values.__getitem__
                with self.assertRaises(TypeError) as raised:
                    report_coverage._threshold_error(  # ruff: ignore[private-member-access]
                        reporter,
                        100.0,
                    )
                self.assertEqual(str(raised.exception), message)


class CoverageReportingTests(unittest.TestCase):
    """Exercise text, XML, threshold, and database-close orchestration."""

    def test_reporting_loads_prints_writes_xml_and_closes_data(self) -> None:
        reporter, data = _configured_reporter()
        with tempfile.TemporaryDirectory() as directory:
            base = Path(directory)
            data_file = base / "combined.coverage"
            xml_output = base / "nested" / "reports" / "coverage.xml"
            with patch.object(
                report_coverage,
                "Coverage",
                return_value=reporter,
            ) as coverage:
                total = report_coverage.report_coverage(data_file, xml_output)
            published = xml_output.read_bytes()
            staged_output = Path(reporter.xml_report.call_args.kwargs["outfile"])
        self.assertEqual(total, 100.0)
        self.assertEqual(published, _realistic_xml())
        coverage.assert_called_once_with(
            data_file=str(data_file),
            config_file=str(report_coverage.PROJECT_CONFIG),
        )
        self.assertEqual(reporter.get_data.call_count, 3)
        reporter.load.assert_called_once_with()
        reporter.report.assert_called_once_with()
        self.assertEqual(staged_output.name, report_coverage.PRIVATE_XML_NAME)
        self.assertTrue(
            staged_output.parent.name.startswith(report_coverage.PRIVATE_XML_PREFIX)
        )
        self.assertEqual(staged_output.parent.parent, data_file.parent)
        self.assertNotEqual(staged_output, xml_output)
        self.assertFalse(staged_output.parent.exists())
        data.close.assert_called_once_with(force=True)

    def test_reporting_closes_each_remapped_data_handle(self) -> None:
        reporter, _data = _configured_reporter()
        handles = [MagicMock(), MagicMock(), MagicMock()]
        reporter.get_data.side_effect = handles
        with tempfile.TemporaryDirectory() as directory:
            base = Path(directory)
            with patch.object(report_coverage, "Coverage", return_value=reporter):
                report_coverage.report_coverage(
                    base / "data",
                    base / "coverage.xml",
                )
        for handle in handles:
            self.assertEqual(handle.close.call_args_list, [call(force=True)])

    def test_reporting_attempts_every_close_when_one_close_fails(self) -> None:
        reporter, _data = _configured_reporter()
        handles = [MagicMock(), MagicMock(), MagicMock()]
        handles[-1].close.side_effect = RuntimeError("close failed")
        reporter.get_data.side_effect = handles
        with tempfile.TemporaryDirectory() as directory:
            base = Path(directory)
            with (
                patch.object(report_coverage, "Coverage", return_value=reporter),
                self.assertRaisesRegex(RuntimeError, "close failed"),
            ):
                report_coverage.report_coverage(
                    base / "data",
                    base / "coverage.xml",
                )
        for handle in handles:
            handle.close.assert_called_once_with(force=True)

    def test_reporting_preserves_the_exact_threshold_diagnostic(self) -> None:
        reporter, data = _configured_reporter(total=99.0)
        with tempfile.TemporaryDirectory() as directory:
            base = Path(directory)
            destination = base / "reports" / "coverage.xml"
            destination.parent.mkdir()
            destination.write_bytes(_realistic_xml())
            with (
                patch.object(report_coverage, "Coverage", return_value=reporter),
                self.assertRaises(report_coverage.CoverageThresholdError) as raised,
            ):
                report_coverage.report_coverage(
                    base / "data",
                    destination,
                )
            preserved = destination.read_bytes()
        self.assertEqual(
            str(raised.exception),
            "total of 99 is less than fail-under=100",
        )
        reporter.xml_report.assert_not_called()
        self.assertEqual(preserved, _realistic_xml())
        data.close.assert_called_once_with(force=True)

    def test_invalid_private_render_is_cleaned_without_replacing_prior(self) -> None:
        reporter, data = _configured_reporter()

        def write_invalid(*, outfile: str) -> int:
            return Path(outfile).write_bytes(b"<private/>")

        reporter.xml_report.side_effect = write_invalid
        with tempfile.TemporaryDirectory() as directory:
            base = Path(directory)
            destination = base / "coverage.xml"
            destination.write_bytes(_realistic_xml())
            with (
                patch.object(report_coverage, "Coverage", return_value=reporter),
                self.assertRaises(coverage_xml_safety.CoverageXmlError),
            ):
                report_coverage.report_coverage(base / "data", destination)
            staged = Path(reporter.xml_report.call_args.kwargs["outfile"])
            preserved = destination.read_bytes()

        self.assertFalse(staged.parent.exists())
        self.assertEqual(preserved, _realistic_xml())
        data.close.assert_called_once_with(force=True)

    def test_reporting_closes_data_after_every_processing_failure(self) -> None:
        stages = ("load", "report", "mkdir", "xml_report", "threshold")
        for stage in stages:
            with self.subTest(stage=stage):
                self._assert_failure_closes_data(stage)

    def _assert_failure_closes_data(self, stage: str) -> None:
        reporter, data = _configured_reporter(
            total=99.0 if stage == "threshold" else 100.0,
        )
        with tempfile.TemporaryDirectory() as directory:
            base = Path(directory)
            xml_output = base / "nested" / "coverage.xml"
            context = patch.object(report_coverage, "Coverage", return_value=reporter)
            if stage in {"load", "report", "xml_report"}:
                getattr(reporter, stage).side_effect = RuntimeError(stage)
            mkdir_context = (
                patch.object(Path, "mkdir", side_effect=RuntimeError(stage))
                if stage == "mkdir"
                else nullcontext()
            )
            with context, mkdir_context, self.assertRaises(RuntimeError):
                report_coverage.report_coverage(base / "data", xml_output)
        data.close.assert_called_once_with(force=True)


class CoverageReportCommandTests(unittest.TestCase):
    """Exercise defaults and both stable command exit statuses."""

    def test_parser_requires_external_data_and_defaults_only_xml_output(self) -> None:
        data_file = Path("/public/ephemeral/.coverage")
        parser = report_coverage._build_parser()  # ruff: ignore[private-member-access]
        with (
            patch("sys.stderr", new_callable=io.StringIO),
            self.assertRaises(SystemExit) as raised,
        ):
            parser.parse_args([])
        self.assertEqual(raised.exception.code, 2)
        arguments = parser.parse_args(["--data-file", str(data_file)])
        self.assertEqual(arguments.data_file, data_file)
        self.assertEqual(arguments.xml_output, report_coverage.DEFAULT_XML_OUTPUT)

    def test_parser_help_describes_both_exact_report_destinations(self) -> None:
        xml_output = Path("/public/report.xml")
        with (
            patch.object(report_coverage, "DEFAULT_XML_OUTPUT", xml_output),
            patch("sys.stdout", new_callable=io.StringIO) as stdout,
            self.assertRaises(SystemExit) as raised,
        ):
            report_coverage.main(["--help"])
        self.assertEqual(raised.exception.code, 0)
        normalized = " ".join(stdout.getvalue().split())
        self.assertIn(str(report_coverage.__doc__), normalized)
        self.assertIn(
            "--data-file DATA_FILE combined coverage database from ephemeral task "
            "storage",
            normalized,
        )
        self.assertIn(
            f"Cobertura XML report (default: {xml_output})",
            normalized,
        )

    def test_main_returns_zero_after_success(self) -> None:
        data_file = Path("/public/data")
        xml_output = Path("/public/coverage.xml")
        with patch.object(
            report_coverage, "report_coverage", return_value=100.0
        ) as run:
            status = report_coverage.main(
                [
                    "--data-file",
                    str(data_file),
                    "--xml-output",
                    str(xml_output),
                ],
            )
        self.assertEqual(status, 0)
        run.assert_called_once_with(data_file, xml_output)

    def test_main_reports_threshold_failure_and_returns_two(self) -> None:
        failure = report_coverage.CoverageThresholdError("public threshold failure")
        data_file = Path("/public/ephemeral/.coverage")
        with (
            patch.object(report_coverage, "report_coverage", side_effect=failure),
            patch("sys.stderr", new_callable=io.StringIO) as stderr,
        ):
            status = report_coverage.main(["--data-file", str(data_file)])
        self.assertEqual(status, 2)
        self.assertEqual(
            stderr.getvalue(),
            "Coverage failure: public threshold failure\n",
        )


if __name__ == "__main__":
    unittest.main(verbosity=2)
