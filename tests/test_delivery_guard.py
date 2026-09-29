"""Cancellation during report delivery is absorbed, then bounded."""

from __future__ import annotations

import os
import signal
import threading

import pytest

from eml_attachment_remover import cancellation


def _deliver(number: int) -> None:
    """Send a signal only if the guard has taken it over, never to a default handler."""
    assert signal.getsignal(number) not in {signal.SIG_DFL, signal.SIG_IGN}
    signal.raise_signal(number)


class _Clock:
    """A settable monotonic clock."""

    def __init__(self) -> None:
        self.now = 1000.0

    def __call__(self) -> float:
        return self.now


@pytest.fixture
def clock() -> _Clock:
    """Provide a settable clock for the guard under test.

    Returns:
        The settable clock.

    """
    return _Clock()


@pytest.fixture
def exits(monkeypatch: pytest.MonkeyPatch) -> list[int]:
    """Record hard exits instead of ending the test process.

    Returns:
        The statuses passed to the hard exit.

    """
    seen: list[int] = []
    monkeypatch.setattr(cancellation, "hard_exit", seen.append)
    return seen


def test_signals_are_recorded_not_raised_and_handlers_are_restored() -> None:
    """A signal during delivery never interrupts it; the caller reads what arrived."""
    before = signal.getsignal(signal.SIGINT)
    with cancellation.delivery_guard() as guard:
        _deliver(signal.SIGINT)
        _deliver(signal.SIGINT)
    assert guard.signals == [signal.SIGINT, signal.SIGINT]
    assert signal.getsignal(signal.SIGINT) is before


def test_an_undisturbed_guard_never_ends_the_process(
    clock: _Clock, exits: list[int]
) -> None:
    """Without a signal there is no deadline, however slow the consumer is."""
    with cancellation.delivery_guard(clock) as guard:
        clock.now += 10 * cancellation.GRACE_SECONDS
        guard.check()
    assert exits == []


def test_the_grace_starts_at_the_first_signal_not_at_the_last_output(
    clock: _Clock, exits: list[int]
) -> None:
    """A consumer stalled before the signal still gets the full grace afterwards."""
    with cancellation.delivery_guard(clock) as guard:
        clock.now += 3 * cancellation.GRACE_SECONDS
        _deliver(signal.SIGINT)
        guard.check()
        assert exits == []
        clock.now += cancellation.GRACE_SECONDS - 0.001
        guard.check()
        assert exits == []
        clock.now += 0.001
        guard.check()
    assert exits == [int(cancellation.INTERRUPTED_STATUS)]


def test_output_progress_restarts_the_grace(clock: _Clock, exits: list[int]) -> None:
    """A slow consumer that keeps accepting output is never cut off."""
    with cancellation.delivery_guard(clock) as guard:
        _deliver(signal.SIGINT)
        for _ in range(5):
            clock.now += cancellation.GRACE_SECONDS - 1
            guard.note_progress()
            guard.check()
    assert exits == []


def test_a_repeated_signal_ends_the_process_immediately(
    clock: _Clock, exits: list[int]
) -> None:
    """Pressing the interrupt twice always works, whatever the consumer does."""
    with cancellation.delivery_guard(clock) as guard:
        _deliver(signal.SIGINT)
        guard.check()
        assert exits == []
        _deliver(signal.SIGINT)
        guard.check()
    assert exits == [130]


def test_hard_exit_ends_the_process_without_unwinding(
    monkeypatch: pytest.MonkeyPatch,
) -> None:
    """The real exit is ``os._exit`` with the given status."""
    seen: list[int] = []
    monkeypatch.setattr(os, "_exit", seen.append)
    cancellation.hard_exit(130)
    assert seen == [130]


def test_the_guard_changes_nothing_off_the_main_thread() -> None:
    """Signal handlers belong to the main thread; workers only run the block."""
    seen: list[bool] = []

    def worker() -> None:
        before = signal.getsignal(signal.SIGINT)
        with cancellation.delivery_guard() as guard:
            seen.append(signal.getsignal(signal.SIGINT) is before)
            seen.append(guard.signals == [])

    thread = threading.Thread(target=worker)
    thread.start()
    thread.join()
    assert seen == [True, True]
