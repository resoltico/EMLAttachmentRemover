"""Refuse unavailable chosen folders once before frontend processing starts."""

from __future__ import annotations

from pathlib import Path

from .domain import AppError, ExitCode
from .native_paths import bind_destination


def check(directory: str) -> None:
    """Use backend directory binding, without probing writes or promising future access.

    Raises:
        AppError: If the chosen directory cannot be bound for processing.

    """
    try:
        bind_destination(str(Path(directory) / ".eml-destination-check"))
    except AppError as error:
        raise AppError(
            ExitCode.WRITE_ERROR,
            "The selected destination folder is unavailable or cannot be opened. "
            "Reconnect the volume or choose another folder. No copies were created.",
            phase="destination",
        ) from error
