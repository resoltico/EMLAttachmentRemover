"""Locate the implementation executed by a signal trace, including Mutmut dispatch."""

from __future__ import annotations

import inspect
from types import FunctionType

from mutmut.mutation.trampoline import get_mutant_under_test


def traced_implementation(function: object) -> FunctionType:
    """Resolve decorators without replacing calls or disabling an active mutant.

    Returns:
        The actual original or selected mutant implementation whose lines execute.

    Raises:
        TypeError: If the target or the pinned Mutmut dispatch contract is invalid.

    """
    while isinstance(function, FunctionType):
        implementation = _mutmut_implementation(function)
        if implementation is not None:
            return implementation
        wrapped = getattr(function, "__wrapped__", None)
        if wrapped is None:
            return function
        function = wrapped
    message = "trace target must be a Python function"
    raise TypeError(message)


def _mutmut_implementation(function: FunctionType) -> FunctionType | None:
    trampoline = inspect.getclosurevars(function).nonlocals.get("trampoline")
    if not isinstance(trampoline, FunctionType):
        return None
    mutants = inspect.getclosurevars(trampoline).nonlocals.get("mutants_dict")
    if not isinstance(mutants, dict) or "_mutmut_orig" not in mutants:
        return None
    module, _, name = get_mutant_under_test().rpartition(".")
    selected = mutants.get(name) if module == function.__module__ else None
    implementation = selected or mutants["_mutmut_orig"]
    if not isinstance(implementation, FunctionType):
        message = "unexpected Mutmut implementation contract"
        raise TypeError(message)
    return implementation
