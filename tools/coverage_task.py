"""Report measured coverage even when the measured tests fail."""

from __future__ import annotations

import subprocess
from typing import TYPE_CHECKING, Final

if TYPE_CHECKING:
    from collections.abc import Callable

FAILURE_GROUP_MESSAGE: Final = "tests and coverage reporting failed"


def measure(tests: Callable[[], None], report: Callable[[], None]) -> None:
    """Run the tests, then always combine and publish their coverage.

    Only an unsuccessful test exit still reports: a timed-out run's data is not
    trustworthy and propagates immediately. The test failure always remains the
    task's failure; a concurrent reporting failure is kept beside it.

    Raises:
        ExceptionGroup: If both the tests and the reporting fail.

    """
    try:
        tests()
    except subprocess.CalledProcessError as test_failure:
        try:
            report()
        except subprocess.CalledProcessError as report_failure:
            failures = [test_failure, report_failure]
            raise ExceptionGroup(FAILURE_GROUP_MESSAGE, failures) from None
        raise
    report()
