"""Atomic filesystem contracts for validated Coverage XML publication."""

from __future__ import annotations

import os
import tempfile
import unittest
from pathlib import Path
from typing import TYPE_CHECKING
from unittest.mock import patch

from tools import coverage_xml_publication, coverage_xml_safety

from tests.coverage_xml_samples import VALID_COVERAGE_XML, altered

if TYPE_CHECKING:
    from types import TracebackType
    from typing import Self

OLD_PUBLIC_XML = b"old public Coverage XML evidence"


class _PartialWriter:
    """Write one partial chunk, then simulate a storage failure."""

    def __init__(self, descriptor: int) -> None:
        self._descriptor = descriptor
        self._writes = 0

    def __enter__(self) -> Self:
        """Return this open writer.

        Returns:
            This writer.

        """
        return self

    def __exit__(
        self,
        exception_type: type[BaseException] | None,
        exception: BaseException | None,
        traceback: TracebackType | None,
    ) -> None:
        """Close the reserved descriptor unconditionally."""
        os.close(self._descriptor)

    def write(self, content: bytes) -> int:
        """Write only the first chunk and fail on the second.

        Returns:
            The partial byte count on the first call.

        Raises:
            OSError: On every call after the first partial write.

        """
        self._writes += 1
        if self._writes > 1:
            message = "simulated partial write failure"
            raise OSError(message)
        partial = max(1, len(content) // 2)
        return os.write(self._descriptor, content[:partial])

    def flush(self) -> None:
        """Provide the binary writer protocol; failures occur before this call."""

    def fileno(self) -> int:
        """Return the reserved descriptor.

        Returns:
            The open descriptor.

        """
        return self._descriptor


class _NonProgressWriter(_PartialWriter):
    """Return a selected impossible write count without modifying the file."""

    def __init__(self, descriptor: int, result: int) -> None:
        super().__init__(descriptor)
        self._result = result

    def write(self, content: bytes) -> int:
        """Return the configured invalid write count.

        Returns:
            Zero or a count larger than the supplied content.

        """
        if self._result == 0:
            return 0
        return len(content) + 1


class CoverageXmlSourceTests(unittest.TestCase):
    """Accept only regular, readable, validated private-stage sources."""

    def test_load_validated_returns_exact_realistic_bytes(self) -> None:
        with tempfile.TemporaryDirectory() as directory:
            source = Path(directory) / "coverage.xml"
            source.write_bytes(VALID_COVERAGE_XML)

            content = coverage_xml_publication.load_validated(source)

        self.assertEqual(content, VALID_COVERAGE_XML)

    def test_load_rejects_missing_directory_symbolic_and_invalid_sources(self) -> None:
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            regular = root / "regular.xml"
            regular.write_bytes(VALID_COVERAGE_XML)
            symbolic = root / "symbolic.xml"
            symbolic.symlink_to(regular)
            directory_source = root / "directory.xml"
            directory_source.mkdir()
            invalid = root / "invalid.xml"
            invalid.write_bytes(altered(b"<coverage ", b"<private "))
            cases = (
                (root / "missing.xml", "cannot inspect staged Coverage XML"),
                (directory_source, "staged Coverage XML must be a regular file"),
                (symbolic, "staged Coverage XML must be a regular file"),
            )
            for source, expected in cases:
                with (
                    self.subTest(source=source.name),
                    self.assertRaises(
                        coverage_xml_safety.CoverageXmlError,
                    ) as raised,
                ):
                    coverage_xml_publication.load_validated(source)
                self.assertEqual(str(raised.exception), expected)
            with self.assertRaises(coverage_xml_safety.CoverageXmlError):
                coverage_xml_publication.load_validated(invalid)

    def test_load_wraps_source_inspection_and_read_failures(self) -> None:
        source = Path("public-stage.xml")
        with (
            patch.object(Path, "lstat", side_effect=OSError("inspect failed")),
            self.assertRaises(coverage_xml_safety.CoverageXmlError) as raised,
        ):
            coverage_xml_publication.load_validated(source)
        self.assertEqual(
            str(raised.exception),
            "cannot inspect staged Coverage XML",
        )
        with tempfile.TemporaryDirectory() as directory:
            source = Path(directory) / "coverage.xml"
            source.write_bytes(VALID_COVERAGE_XML)
            with (
                patch.object(Path, "read_bytes", side_effect=OSError("read failed")),
                self.assertRaises(coverage_xml_safety.CoverageXmlError) as raised,
            ):
                coverage_xml_publication.load_validated(source)
            self.assertEqual(
                str(raised.exception),
                "cannot read staged Coverage XML",
            )


class CoverageXmlAtomicPublicationTests(unittest.TestCase):
    """Preserve the old report across every pre-commit publication failure."""

    def test_publish_creates_parent_and_atomically_writes_exact_bytes(self) -> None:
        real_mkstemp = tempfile.mkstemp
        with tempfile.TemporaryDirectory() as directory:
            destination = Path(directory) / "nested" / "coverage.xml"
            with patch(
                "tools.coverage_xml_publication.tempfile.mkstemp",
                wraps=real_mkstemp,
            ) as reserve:
                published = coverage_xml_publication.publish(
                    VALID_COVERAGE_XML,
                    destination,
                )

            self.assertEqual(published, destination)
            self.assertEqual(destination.read_bytes(), VALID_COVERAGE_XML)
            self.assertEqual(reserve.call_args.kwargs["dir"], destination.parent)
            self.assertEqual(
                reserve.call_args.kwargs["prefix"],
                ".coverage.xml.",
            )
            self.assertEqual(
                reserve.call_args.kwargs["suffix"],
                coverage_xml_publication.TEMPORARY_SUFFIX,
            )
            self.assertFalse(tuple(destination.parent.glob(".coverage.xml.*.tmp")))

    def test_invalid_content_leaves_prior_report_intact(self) -> None:
        with tempfile.TemporaryDirectory() as directory:
            destination = Path(directory) / "coverage.xml"
            destination.write_bytes(OLD_PUBLIC_XML)

            with self.assertRaises(coverage_xml_safety.CoverageXmlError):
                coverage_xml_publication.publish(b"<private/>", destination)

            self.assertEqual(destination.read_bytes(), OLD_PUBLIC_XML)

    def test_destination_parent_and_target_must_be_real_and_regular(self) -> None:
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            real_parent = root / "real"
            real_parent.mkdir()
            symbolic_parent = root / "symbolic-parent"
            symbolic_parent.symlink_to(real_parent, target_is_directory=True)
            regular = real_parent / "regular.xml"
            regular.write_bytes(OLD_PUBLIC_XML)
            symbolic = real_parent / "symbolic.xml"
            symbolic.symlink_to(regular)
            directory_target = real_parent / "directory.xml"
            directory_target.mkdir()
            cases = (
                (
                    symbolic_parent / "coverage.xml",
                    "Coverage XML directory must be real",
                ),
                (symbolic, "Coverage XML destination must be regular"),
                (directory_target, "Coverage XML destination must be regular"),
            )
            for destination, expected in cases:
                with (
                    self.subTest(destination=destination.name),
                    self.assertRaises(
                        coverage_xml_safety.CoverageXmlError,
                    ) as raised,
                ):
                    coverage_xml_publication.publish(
                        VALID_COVERAGE_XML,
                        destination,
                    )
                self.assertEqual(str(raised.exception), expected)

    def test_destination_inspection_failures_are_contextual(self) -> None:
        with tempfile.TemporaryDirectory() as directory:
            destination = Path(directory) / "coverage.xml"
            with (
                patch.object(Path, "lstat", side_effect=OSError("parent failed")),
                self.assertRaises(coverage_xml_safety.CoverageXmlError) as raised,
            ):
                coverage_xml_publication.publish(VALID_COVERAGE_XML, destination)
            self.assertEqual(
                str(raised.exception),
                "cannot inspect Coverage XML directory",
            )

            original_lstat = Path.lstat

            def fail_target(path: Path) -> os.stat_result:
                if path == destination:
                    message = "target failed"
                    raise OSError(message)
                return original_lstat(path)

            with (
                patch.object(Path, "lstat", autospec=True, side_effect=fail_target),
                self.assertRaises(coverage_xml_safety.CoverageXmlError) as raised,
            ):
                coverage_xml_publication.publish(VALID_COVERAGE_XML, destination)
            self.assertEqual(
                str(raised.exception),
                "cannot inspect Coverage XML destination",
            )

    def test_directory_creation_and_reservation_failures_preserve_prior(self) -> None:
        with tempfile.TemporaryDirectory() as directory:
            destination = Path(directory) / "coverage.xml"
            destination.write_bytes(OLD_PUBLIC_XML)
            failures = (
                (
                    patch.object(Path, "mkdir", side_effect=OSError("mkdir failed")),
                    "cannot create Coverage XML directory",
                ),
                (
                    patch(
                        "tools.coverage_xml_publication.tempfile.mkstemp",
                        side_effect=OSError("reserve failed"),
                    ),
                    "cannot reserve Coverage XML publication",
                ),
            )
            for failure, expected in failures:
                with (
                    self.subTest(failure=failure),
                    failure,
                    self.assertRaises(
                        coverage_xml_safety.CoverageXmlError,
                    ) as raised,
                ):
                    coverage_xml_publication.publish(
                        VALID_COVERAGE_XML,
                        destination,
                    )
                self.assertEqual(str(raised.exception), expected)
                self.assertEqual(destination.read_bytes(), OLD_PUBLIC_XML)

    def test_partial_write_failure_leaves_prior_report_intact(self) -> None:
        with tempfile.TemporaryDirectory() as directory:
            destination = Path(directory) / "coverage.xml"
            destination.write_bytes(OLD_PUBLIC_XML)
            with (
                patch(
                    "tools.coverage_xml_publication.os.fdopen",
                    side_effect=lambda descriptor, _mode: _PartialWriter(descriptor),
                ),
                self.assertRaises(coverage_xml_safety.CoverageXmlError) as raised,
            ):
                coverage_xml_publication.publish(VALID_COVERAGE_XML, destination)

            self.assertEqual(
                str(raised.exception),
                "cannot publish Coverage XML atomically",
            )
            self.assertEqual(
                str(raised.exception.__cause__),
                "Coverage XML publication write was incomplete",
            )
            self.assertEqual(destination.read_bytes(), OLD_PUBLIC_XML)
            self.assertFalse(tuple(destination.parent.glob(".coverage.xml.*.tmp")))

    def test_nonprogress_writes_are_rejected_without_replacement(self) -> None:
        for result in (0, len(VALID_COVERAGE_XML) + 1):
            with tempfile.TemporaryDirectory() as directory:
                destination = Path(directory) / "coverage.xml"
                destination.write_bytes(OLD_PUBLIC_XML)
                with (
                    self.subTest(result=result),
                    patch(
                        "tools.coverage_xml_publication.os.fdopen",
                        side_effect=lambda descriptor, _mode, selected=result: (
                            _NonProgressWriter(
                                descriptor,
                                selected,
                            )
                        ),
                    ),
                    self.assertRaises(
                        coverage_xml_safety.CoverageXmlError,
                    ) as raised,
                ):
                    coverage_xml_publication.publish(
                        VALID_COVERAGE_XML,
                        destination,
                    )
                self.assertEqual(
                    str(raised.exception),
                    "cannot publish Coverage XML atomically",
                )
                self.assertEqual(
                    str(raised.exception.__cause__),
                    "Coverage XML publication write was incomplete",
                )
                self.assertEqual(destination.read_bytes(), OLD_PUBLIC_XML)

    def test_fdopen_failure_closes_descriptor_and_preserves_prior(self) -> None:
        real_close = os.close
        with tempfile.TemporaryDirectory() as directory:
            destination = Path(directory) / "coverage.xml"
            destination.write_bytes(OLD_PUBLIC_XML)
            with (
                patch(
                    "tools.coverage_xml_publication.os.fdopen",
                    side_effect=RuntimeError("fdopen failed"),
                ),
                patch(
                    "tools.coverage_xml_publication.os.close",
                    wraps=real_close,
                ) as close,
                self.assertRaisesRegex(RuntimeError, "fdopen failed"),
            ):
                coverage_xml_publication.publish(VALID_COVERAGE_XML, destination)
            close.assert_called_once()
            self.assertEqual(destination.read_bytes(), OLD_PUBLIC_XML)
            self.assertFalse(tuple(destination.parent.glob(".coverage.xml.*.tmp")))

    def test_fsync_and_replace_failures_preserve_prior(self) -> None:
        failures = (
            patch(
                "tools.coverage_xml_publication.os.fsync",
                side_effect=OSError("fsync failed"),
            ),
            patch.object(Path, "replace", side_effect=OSError("replace failed")),
        )
        for failure in failures:
            with tempfile.TemporaryDirectory() as directory:
                destination = Path(directory) / "coverage.xml"
                destination.write_bytes(OLD_PUBLIC_XML)
                with (
                    self.subTest(failure=failure),
                    failure,
                    self.assertRaises(coverage_xml_safety.CoverageXmlError),
                ):
                    coverage_xml_publication.publish(
                        VALID_COVERAGE_XML,
                        destination,
                    )
                self.assertEqual(destination.read_bytes(), OLD_PUBLIC_XML)
                self.assertFalse(tuple(destination.parent.glob(".coverage.xml.*.tmp")))


if __name__ == "__main__":
    unittest.main(verbosity=2)
