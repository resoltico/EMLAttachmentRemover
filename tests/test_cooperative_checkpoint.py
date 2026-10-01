"""Request recording, coherent operations, and bounded processing cancellation."""

from __future__ import annotations

import signal
import threading
import time
from typing import override

import pytest

from eml_attachment_remover import cancellation
from eml_attachment_remover.cancellation_state import CURRENT, CancellationState


def test_signal_is_recorded_until_a_safe_checkpoint() -> None:
    previous = signal.getsignal(signal.SIGINT)
    with cancellation.install_cancellation_handlers():
        with cancellation.coherent_operation():
            signal.raise_signal(signal.SIGINT)
            cancellation.checkpoint()
            assert CURRENT.get() is not None
        with pytest.raises(cancellation.CancellationSignal) as caught:
            cancellation.checkpoint()
        assert caught.value.number == signal.SIGINT
        assert caught.value.name == "SIGINT"
    assert CURRENT.get() is None
    assert signal.getsignal(signal.SIGINT) == previous


def test_previsibility_checkpoint_can_cancel_an_uncommitted_operation() -> None:
    with cancellation.install_cancellation_handlers():
        with cancellation.coherent_operation():
            signal.raise_signal(signal.SIGINT)
            with pytest.raises(cancellation.CancellationSignal):
                cancellation.checkpoint(before_visibility=True)


def test_unresponsive_work_uses_the_same_bounded_escape(
    monkeypatch: pytest.MonkeyPatch,
) -> None:
    exits: list[int] = []
    finished = threading.Event()

    def exit_test(status: int) -> None:
        exits.append(status)
        finished.set()

    monkeypatch.setattr(cancellation, "hard_exit", exit_test)
    monkeypatch.setattr(cancellation, "GRACE_SECONDS", 0.05)
    with cancellation.install_cancellation_handlers():
        state = CURRENT.get()
        assert state is not None
        state.record(signal.SIGINT, None)
        assert finished.wait(1)
        with pytest.raises(cancellation.CancellationSignal):
            cancellation.checkpoint()
    assert exits
    assert set(exits) == {130}


def test_delivery_records_late_requests_on_the_processing_owner() -> None:
    with cancellation.install_cancellation_handlers():
        with cancellation.delivery_guard() as guard:
            signal.raise_signal(signal.SIGINT)
            signal.raise_signal(signal.SIGINT)
            state = CURRENT.get()
            assert state is not None
            assert state.signals == guard.signals == [signal.SIGINT, signal.SIGINT]
            assert state.acknowledged == 0
        assert guard.result(0) == 130
        assert state.acknowledged == 2
        cancellation.checkpoint()


def test_processing_grace_allows_a_pending_request_before_expiry(
    monkeypatch: pytest.MonkeyPatch,
) -> None:
    with cancellation.install_cancellation_handlers():
        state = CURRENT.get()
        assert state is not None
        state.record(signal.SIGINT, None)
        monkeypatch.setattr(cancellation, "GRACE_SECONDS", 1000)
        with pytest.raises(cancellation.CancellationSignal):
            cancellation.checkpoint()
        assert state.active
        assert not state.delivering
        threading.Event().wait(0.1)


@pytest.mark.parametrize(("elapsed", "requests"), [(0.1, 1), (10.0, 1), (0.1, 2)])
def test_processing_monitor_honors_exact_grace_and_repeat_boundaries(
    monkeypatch: pytest.MonkeyPatch, elapsed: float, requests: int
) -> None:
    class Stop(threading.Event):
        calls = 0

        @override
        def wait(self, timeout: float | None = None) -> bool:
            self.calls += 1
            return self.calls > 1

    state = CancellationState(active=True, stop=Stop())
    monkeypatch.setattr(time, "monotonic", lambda: 40.0)
    for _ in range(requests):
        state.record(signal.SIGINT, None)
    assert state.requested_at == pytest.approx(40.0)
    monkeypatch.setattr(time, "monotonic", lambda: 40.0 + elapsed)
    exits: list[int] = []

    def exited(status: int) -> None:
        exits.append(status)
        state.active = False

    monkeypatch.setattr(cancellation, "hard_exit", exited)
    cancellation._watch_processing(state)  # ruff: ignore[private-member-access] - real monitor policy under a finite scheduler.
    assert exits == ([130] if requests == 2 or elapsed >= 10 else [])
