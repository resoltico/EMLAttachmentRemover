"""Cancellation handler installation, fallback naming, and restoration contracts."""

from __future__ import annotations

import os
import signal
import threading
from typing import TYPE_CHECKING, cast

import pytest

from eml_attachment_remover import cancellation

if TYPE_CHECKING:
    from collections.abc import Callable


def test_cancellation_fallback_and_installed_handler_restore(
    monkeypatch: pytest.MonkeyPatch,
) -> None:
    assert cancellation.cancellation_name(999) == "signal-999"
    previous = signal.getsignal(signal.SIGINT)
    with cancellation.install_cancellation_handlers():
        handler = signal.getsignal(signal.SIGINT)
        assert callable(handler)
        with pytest.raises(cancellation.CancellationSignal):
            cast("Callable[[int, object], None]", handler)(signal.SIGINT, None)
    assert signal.getsignal(signal.SIGINT) == previous
    watched = cast(
        "Callable[[], tuple[int, ...]]", cancellation.__dict__["_watched_signals"]
    )
    monkeypatch.setattr(os, "name", "nt")
    assert watched() == (signal.SIGINT,)
    monkeypatch.setattr(os, "name", "posix")
    monkeypatch.setitem(signal.__dict__, "SIGHUP", signal.SIGINT)
    assert watched() == (signal.SIGINT, signal.SIGTERM, signal.SIGINT)
    monkeypatch.delattr(signal, "SIGHUP", raising=False)
    assert watched() == (signal.SIGINT, signal.SIGTERM)


def test_cancellation_context_is_noop_outside_the_main_thread(
    monkeypatch: pytest.MonkeyPatch,
) -> None:
    marker = object()
    monkeypatch.setattr(threading, "current_thread", lambda: marker)
    with cancellation.install_cancellation_handlers():
        pass
