"""Failed setup and interrupted teardown cannot poison same-process API reuse."""

from __future__ import annotations

import inspect
import signal
import sys
import threading
from contextlib import contextmanager
from contextvars import copy_context
from typing import TYPE_CHECKING

import pytest

from eml_attachment_remover import cancellation, staged_output
from eml_attachment_remover.cancellation_owner import CancellationOwner, cleanup_actions
from eml_attachment_remover.cancellation_state import CURRENT, CancellationState
from eml_attachment_remover.domain import ItemStatus
from eml_attachment_remover.processing import process_file
from tests.deadline_support import finite_operation
from tests.live_report_support import MESSAGE
from tests.trace_implementation_support import traced_implementation

if TYPE_CHECKING:
    from collections.abc import Generator
    from pathlib import Path
    from types import FrameType

    from _typeshed import TraceFunction


@contextmanager
def _interrupt(function: object, statement: str) -> Generator[list[int]]:
    function = traced_implementation(function)
    lines, first = inspect.getsourcelines(function)
    target = next(
        first + index for index, line in enumerate(lines) if statement in line
    )
    code = function.__code__
    sent: list[int] = []

    def trace(frame: FrameType, event: str, _argument: object) -> TraceFunction:
        if (
            event == "line"
            and frame.f_code is code
            and frame.f_lineno == target
            and not sent
        ):
            sent.append(1)
            signal.raise_signal(signal.SIGINT)
        return trace

    previous = sys.gettrace()
    sys.settrace(trace)
    try:
        yield sent
    finally:
        sys.settrace(previous)


def _reuse(root: Path) -> None:
    source = root / "second.eml"
    source.write_bytes(MESSAGE)
    with _interrupt(staged_output._finish_or_raise, "return receipt") as sent:  # ruff: ignore[private-member-access] - real proven publisher return.
        ledger = process_file(str(source))
    item = ledger.items[0]
    assert sent == [1]
    assert ledger.interruption is not None
    assert item.status is ItemStatus.CREATED
    assert item.terminalized
    assert item.publication is not None
    assert item.publication.address_verified
    assert source.read_bytes() == MESSAGE
    assert (root / "second.mime-pruned.eml").is_file()
    assert CURRENT.get() is None
    assert not _monitors()


def _monitors() -> list[threading.Thread]:
    return [
        thread
        for thread in threading.enumerate()
        if thread.name == "eml-cancellation-monitor"
    ]


@pytest.mark.parametrize("boundary", ["setup", "shutdown"])
def test_real_boundary_signal_restores_context_before_another_api_call(
    tmp_path: Path, boundary: str
) -> None:
    source = tmp_path / "first.eml"
    source.write_bytes(MESSAGE)
    previous = signal.getsignal(signal.SIGINT)
    function, statement = (
        (
            inspect.unwrap(cancellation.install_cancellation_handlers),
            "with _handling(state.record)",
        )
        if boundary == "setup"
        else (CancellationOwner._reset, "CURRENT.reset(self.token)")  # ruff: ignore[private-member-access] - teardown context boundary.
    )
    with _interrupt(function, statement) as sent:
        if boundary == "setup":
            with pytest.raises(KeyboardInterrupt):
                process_file(str(source), dry_run=True)
        else:
            ledger = process_file(str(source), dry_run=True)
            assert ledger.interruption is not None
            assert ledger.items[0].status is ItemStatus.WOULD_CREATE
    assert sent == [1]
    assert CURRENT.get() is None
    assert signal.getsignal(signal.SIGINT) == previous
    assert not _monitors()
    _reuse(tmp_path)


@pytest.mark.parametrize("failure", ["constructor", "start", "after_start", "join"])
def test_resource_failure_restores_ownership_before_another_api_call(
    tmp_path: Path, monkeypatch: pytest.MonkeyPatch, failure: str
) -> None:
    source = tmp_path / "first.eml"
    source.write_bytes(MESSAGE)
    previous = signal.getsignal(signal.SIGINT)
    operation = (
        "__init__"
        if failure == "constructor"
        else "join"
        if failure == "join"
        else "start"
    )
    original = getattr(threading.Thread, operation)
    failed = OSError("public controller resource failure")

    def injected(worker: threading.Thread, *args: object, **kwargs: object) -> None:
        if failure in {"after_start", "join"}:
            original(worker, *args, **kwargs)
        raise failed

    with monkeypatch.context() as patch:
        patch.setattr(threading.Thread, operation, injected)
        with pytest.raises(
            OSError, match="public controller resource failure"
        ) as caught:
            process_file(str(source), dry_run=True)
    assert caught.value is failed
    assert CURRENT.get() is None
    assert signal.getsignal(signal.SIGINT) == previous
    assert not _monitors()
    _reuse(tmp_path)


def test_cleanup_preserves_primary_and_attempts_each_independent_release() -> None:
    primary = KeyboardInterrupt()
    calls: list[int] = []

    def fail() -> None:
        calls.append(1)
        message = "public release failure"
        raise OSError(message)

    try:
        cleanup_actions(primary, (fail, lambda: calls.append(2), fail))
    except KeyboardInterrupt:
        pytest.fail("cleanup re-raised a retained primary interruption")
    assert calls == [1, 2, 1]
    assert primary.__notes__ == [
        "cancellation cleanup: OSError: public release failure",
        "cancellation cleanup: OSError: public release failure",
    ]


def test_nested_scopes_share_one_live_owner() -> None:
    with cancellation.install_cancellation_handlers():
        state = CURRENT.get()
        monitors = _monitors()
        assert len(monitors) == 1
        with cancellation.install_cancellation_handlers():
            assert CURRENT.get() is state
            assert _monitors() == monitors
        assert CURRENT.get() is state
        assert state is not None
        assert state.active
    assert CURRENT.get() is None
    assert not _monitors()


def test_inactive_bound_context_is_rejected_as_broken_ownership() -> None:
    token = CURRENT.set(CancellationState())
    try:
        with (
            pytest.raises(RuntimeError, match="inactive cancellation context"),
            cancellation.install_cancellation_handlers(),
        ):
            pytest.fail("inactive context must not be reused")
    finally:
        CURRENT.reset(token)


def test_copied_thread_context_acquires_its_own_controller() -> None:
    seen: list[CancellationState | None] = []

    def child() -> None:
        inherited = CURRENT.get()
        with cancellation.install_cancellation_handlers():
            current = CURRENT.get()
            assert current is not inherited
            assert current is not None
            assert current.owner_thread == threading.get_ident()
            seen.append(current)
        assert CURRENT.get() is inherited

    with cancellation.install_cancellation_handlers():
        enclosing = CURRENT.get()
        context = copy_context()
        worker = threading.Thread(target=context.run, args=(child,))
        worker.start()
        worker.join()
        assert CURRENT.get() is enclosing
    assert len(seen) == 1
    assert not _monitors()


def test_shutdown_wakes_the_monitor_without_waiting_for_its_poll_timeout(
    monkeypatch: pytest.MonkeyPatch,
) -> None:
    entered = threading.Event()
    woke = threading.Event()

    class WakeReceipt(threading.Event):
        def wait(self, timeout: float | None = None) -> bool:
            entered.set()
            result = super().wait(timeout)
            woke.set()
            return result

    state = CancellationState(stop=WakeReceipt())
    monkeypatch.setattr(cancellation, "CancellationState", lambda: state)
    monkeypatch.setattr(cancellation, "POLL_SECONDS", 60)
    with cancellation.install_cancellation_handlers():
        assert entered.wait(1)
    assert woke.is_set()
    assert not state.active
    assert state.stop.is_set()
    assert not _monitors()


def test_cleanup_failure_without_a_primary_retains_later_failure_details() -> None:
    def failed() -> None:
        message = "public cleanup failure"
        raise OSError(message)

    with pytest.raises(OSError, match="public cleanup failure") as caught:
        cleanup_actions(None, (failed, failed))
    assert caught.value.__notes__ == [
        "cancellation cleanup: OSError: public cleanup failure"
    ]


def test_join_failure_cannot_replace_a_body_interruption(
    monkeypatch: pytest.MonkeyPatch,
) -> None:
    original = threading.Thread.join
    primary = KeyboardInterrupt()
    previous = signal.getsignal(signal.SIGINT)

    def failed_join(worker: threading.Thread, timeout: float | None = None) -> None:
        original(worker, timeout)
        message = "public join failure"
        raise OSError(message)

    with monkeypatch.context() as patch:
        patch.setattr(threading.Thread, "join", failed_join)
        with (
            pytest.raises(KeyboardInterrupt) as caught,
            cancellation.install_cancellation_handlers(),
        ):
            raise primary
    assert caught.value is primary
    assert primary.__notes__ == ["cancellation cleanup: OSError: public join failure"]
    assert CURRENT.get() is None
    assert signal.getsignal(signal.SIGINT) == previous
    assert not _monitors()


def test_a_monitor_that_does_not_stop_is_reported_after_context_reset(
    tmp_path: Path, monkeypatch: pytest.MonkeyPatch
) -> None:
    source = tmp_path / "first.eml"
    source.write_bytes(MESSAGE)
    with monkeypatch.context() as patch:
        patch.setattr(threading.Thread, "is_alive", lambda _worker: True)
        with pytest.raises(RuntimeError, match="watchdog did not stop"):
            process_file(str(source), dry_run=True)
    assert CURRENT.get() is None
    assert not _monitors()
    _reuse(tmp_path)


def test_a_real_unresponsive_monitor_has_a_bounded_join() -> None:
    unblock = threading.Event()
    worker = threading.Thread(target=unblock.wait, daemon=True)
    state = CancellationState()
    owner = CancellationOwner(state, worker, 0.01)
    try:
        with (
            finite_operation(5),
            pytest.raises(RuntimeError, match=r"^cancellation watchdog did not stop$"),
            owner.scope(),
        ):
            pass
        assert CURRENT.get() is None
    finally:
        unblock.set()
        worker.join(1)
    assert not worker.is_alive()


@pytest.mark.parametrize("boundary", ["install", "restore"])
def test_signal_handler_failure_attempts_every_other_restoration(
    monkeypatch: pytest.MonkeyPatch, boundary: str
) -> None:
    watched = cancellation._watched_signals()  # ruff: ignore[private-member-access] - exact handler ownership.
    previous = {number: signal.getsignal(number) for number in watched}
    original = signal.signal
    injected: list[int] = []

    def failed(number: int, handler: object) -> object:
        result = original(number, handler)  # type: ignore[arg-type]
        restoring = handler == previous[number]
        if not injected and restoring == (boundary == "restore"):
            injected.append(number)
            message = "public signal ownership failure"
            raise OSError(message)
        return result

    with monkeypatch.context() as patch:
        patch.setattr(signal, "signal", failed)
        with (
            pytest.raises(OSError, match="public signal ownership failure"),
            cancellation.install_cancellation_handlers(),
        ):
            assert CURRENT.get() is not None
    assert injected
    assert {number: signal.getsignal(number) for number in watched} == previous
    assert CURRENT.get() is None
    assert not _monitors()
