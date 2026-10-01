"""One real SIGINT at a semantic statement during owned signal finalization."""

from __future__ import annotations

import inspect
import signal
import sys
from contextlib import contextmanager
from typing import TYPE_CHECKING
from unittest.mock import patch

from eml_attachment_remover import cancellation
from eml_attachment_remover.cancellation_owner import CancellationOwner
from eml_attachment_remover.cancellation_state import CancellationState
from tests.trace_implementation_support import traced_implementation

if TYPE_CHECKING:
    from collections.abc import Iterator
    from types import FrameType

    from _typeshed import TraceFunction


@contextmanager
def interrupt_at(boundary: str) -> Iterator[list[int]]:
    """Record a signal only while the selected invocation still owns its handler.

    Yields:
        The one-shot receipt list proving the selected real boundary was reached.

    """
    if boundary in {"join", "reset"}:
        method = "_" + boundary
        original = getattr(CancellationOwner, method)
        sent: list[int] = []

        def interrupt_operation(owner: CancellationOwner) -> None:
            if not sent:
                sent.append(1)
                signal.raise_signal(signal.SIGINT)
            original(owner)

        # Wrap the operation, preserving Mutmut's dispatch and selected mutant.
        with patch.object(CancellationOwner, method, interrupt_operation):
            yield sent
        return
    function, statement = {
        "stop": (inspect.unwrap(CancellationOwner.scope), "self.state.active = False"),
        "processing_restore": (
            inspect.unwrap(cancellation._handling),  # ruff: ignore[private-member-access] - handler handback boundary.
            "cleanup_actions(",
        ),
        "delivery_restore": (
            inspect.unwrap(cancellation._handling),  # ruff: ignore[private-member-access] - handler handback boundary.
            "cleanup_actions(",
        ),
    }[boundary]
    function = traced_implementation(function)
    lines, first = inspect.getsourcelines(function)
    target = next(
        first + index for index, line in enumerate(lines) if statement in line
    )
    sent = []

    def trace(frame: FrameType, event: str, _argument: object) -> TraceFunction:
        if (
            event == "line"
            and frame.f_code is function.__code__
            and frame.f_lineno == target
            and not sent
        ):
            handler = signal.getsignal(signal.SIGINT)
            owned = frame.f_locals.get("handler", handler)
            processing = isinstance(getattr(owned, "__self__", None), CancellationState)
            wanted = (
                (boundary == "processing_restore" and processing)
                or (boundary == "delivery_restore" and not processing)
                or not boundary.endswith("restore")
            )
            if wanted:
                assert handler == owned
                assert callable(handler)
                sent.append(1)
                signal.raise_signal(signal.SIGINT)
        return trace

    previous = sys.gettrace()
    sys.settrace(trace)
    try:
        yield sent
    finally:
        sys.settrace(previous)
