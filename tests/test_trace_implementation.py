"""Signal traces follow the pinned Mutmut dispatch without bypassing its behavior."""

from __future__ import annotations

from functools import wraps
from types import FunctionType
from typing import TYPE_CHECKING, cast

import pytest
from mutmut.mutation.trampoline import wrap_in_trampoline

from tests import trace_implementation_support
from tests.trace_implementation_support import traced_implementation

if TYPE_CHECKING:
    from collections.abc import Callable


@pytest.mark.parametrize("selector", ["", "stats", "candidate", "other"])
def test_trace_follows_actual_original_or_selected_mutant(
    monkeypatch: pytest.MonkeyPatch, selector: str
) -> None:
    def original() -> int:
        return 1

    def mutated() -> int:
        return 2

    original.__name__ = "x_original__mutmut_orig"
    implementations: dict[str, Callable[[], int]] = {
        "_mutmut_orig": original,
        "candidate": mutated,
    }
    active = (
        f"{original.__module__}.{selector}"
        if selector in {"candidate", "other"}
        else selector
    )
    # Isolate selector simulation to this synthetic trampoline's globals.
    factory = FunctionType(
        wrap_in_trampoline.__code__,
        {**wrap_in_trampoline.__globals__, "get_mutant_under_test": lambda: active},
    )
    wrapped = cast(
        "Callable[[], int]", factory(implementations, is_classmethod=False)(original)
    )

    @wraps(wrapped)
    def decorated() -> int:
        return wrapped()

    monkeypatch.setattr(
        trace_implementation_support, "get_mutant_under_test", lambda: active
    )
    expected = mutated if selector == "candidate" else original
    assert traced_implementation(decorated) is expected
    assert decorated() == expected()


def test_trace_rejects_a_nonfunction() -> None:
    with pytest.raises(TypeError, match="trace target must be a Python function"):
        traced_implementation(None)
