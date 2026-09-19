"""Literal final-address reopening behind one cross-platform native boundary."""

from __future__ import annotations

import os
from typing import TYPE_CHECKING

from . import native_windows_binding as _windows

if TYPE_CHECKING:
    from .domain import PathValue


def open_final_address(address: PathValue) -> int:
    """Open one literal final address without following its final filesystem entry.

    Returns:
        An owned descriptor for the literal address.

    Raises:
        OSError: If the address is absent, unsafe, or cannot be reopened natively.

    """
    text = address.text
    if text is None:
        message = "final address has no native text"
        raise OSError(message)
    if os.name == "nt":
        return _windows.open_final_address(text)
    return os.open(
        os.fsencode(text),
        (
            os.O_RDONLY
            | getattr(os, "O_NONBLOCK", 0)
            | getattr(os, "O_NOFOLLOW", 0)
            | getattr(os, "O_CLOEXEC", 0)
        ),
    )
