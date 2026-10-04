"""Typed late binding and status handling for Windows native operations."""

from __future__ import annotations

import ctypes
from typing import TYPE_CHECKING, Literal, overload

from .domain import AppError, ExitCode
from .native_windows_abi import STATUS_NOT_SUPPORTED

if TYPE_CHECKING:
    from collections.abc import Callable


@overload
def ctypes_attribute(name: Literal["WinDLL"]) -> Callable[..., ctypes.CDLL]: ...


@overload
def ctypes_attribute(name: Literal["get_last_error"]) -> Callable[[], int]: ...


@overload
def ctypes_attribute(name: Literal["FormatError"]) -> Callable[[int], str]: ...


@overload
def ctypes_attribute(name: str) -> object: ...


def ctypes_attribute(name: str) -> object:
    """Return a dynamically present ctypes attribute without import-time Windows use.

    Returns:
        The untyped ctypes attribute selected by its exact native name.

    """
    return getattr(ctypes, name)


class Msvcrt:
    """Expose the two CRT calls needed by the native descriptor boundary."""

    @staticmethod
    def get_osfhandle(descriptor: int) -> int:
        """Return one native handle obtained from its owned CRT descriptor."""  # ruff: ignore[docstring-missing-returns] - exact conversion is immediate.
        return int(__import__("msvcrt").get_osfhandle(descriptor))

    @staticmethod
    def open_osfhandle(handle: int, flags: int) -> int:
        """Transfer one native handle to the CRT with exact binary flags."""  # ruff: ignore[docstring-missing-returns] - exact conversion is immediate.
        return int(__import__("msvcrt").open_osfhandle(handle, flags))


def check_publication_support(status: int) -> None:
    """Reject an explicitly unsupported native publication operation.

    Raises:
        AppError: If Windows reports STATUS_NOT_SUPPORTED for the rename.

    """
    if status == STATUS_NOT_SUPPORTED:
        raise AppError(
            ExitCode.ATOMIC_PUBLICATION_UNSUPPORTED,
            "destination does not support exclusive atomic publication",
        )
