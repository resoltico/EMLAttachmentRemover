"""Bound actual finite writes without consuming their budget during preparation."""

from __future__ import annotations

import signal
import subprocess
import sys
import time

import pytest

from tests.deadline_support import finite_main_call

pytestmark = pytest.mark.skipif(
    not hasattr(signal, "SIGALRM"), reason="POSIX operation deadlines"
)
NONTERMINATING_WRITE = """
import signal, sys
from eml_attachment_remover import report_delivery
from eml_attachment_remover.cancellation import DeliveryGuard
from tests.deadline_support import finite_main_call

class NoProgress:
    def write(self, data): return None
    def flush(self): pass

previous = signal.getsignal(signal.SIGALRM)
report_delivery._bounded = finite_main_call(report_delivery._bounded, 0.1)
report_delivery._write_all = finite_main_call(report_delivery._write_all, 0.1)
try:
    if sys.argv[1] == 'direct':
        report_delivery._write_all(NoProgress(), b'payload', DeliveryGuard())
    else:
        sys.stderr = NoProgress()
        report_delivery.write_note('payload', DeliveryGuard())
except AssertionError as error:
    assert str(error) == 'finite report delivery did not complete'
else:
    raise AssertionError('nonterminating writer escaped its deadline')
assert signal.getsignal(signal.SIGALRM) == previous
assert signal.getitimer(signal.ITIMER_REAL) == (0.0, 0.0)
print('Nonterminating write rejected; caller timer restored.')
"""


def test_preparation_does_not_consume_the_operation_deadline() -> None:
    calls: list[float] = []

    def operation() -> None:
        calls.append(time.monotonic())

    bounded = finite_main_call(operation, 0.1)
    alarm = getattr(signal, "SIGALRM", None)
    get_timer = getattr(signal, "getitimer", None)
    timer_kind = getattr(signal, "ITIMER_REAL", None)
    assert alarm is not None
    assert get_timer is not None
    assert timer_kind is not None
    previous = signal.getsignal(alarm)
    time.sleep(0.2)
    bounded()
    assert len(calls) == 1
    assert signal.getsignal(alarm) == previous
    assert get_timer(timer_kind) == (0.0, 0.0)


@pytest.mark.parametrize("mode", ["direct", "bounded"])
def test_nonterminating_writes_fail_and_retire_their_process(mode: str) -> None:
    result = subprocess.run(
        [sys.executable, "-B", "-c", NONTERMINATING_WRITE, mode],
        capture_output=True,
        text=True,
        check=False,
        timeout=10,
    )
    assert result.returncode == 0, result.stdout + result.stderr
    assert result.stdout.strip() == (
        "Nonterminating write rejected; caller timer restored."
    )
