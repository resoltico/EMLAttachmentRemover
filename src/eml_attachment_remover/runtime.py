"""Enforce the supported interpreter before entering the installed command."""

from __future__ import annotations

import platform
import sys
from typing import Final

SUPPORTED_IMPLEMENTATION: Final = "CPython"
SUPPORTED_VERSION: Final = (3, 14)
UNSUPPORTED_RUNTIME_STATUS: Final = 1


def runtime_supported(implementation: str, version: tuple[int, int]) -> bool:
    """Return whether an interpreter satisfies the installed-command contract.

    Returns:
        Whether both the implementation and major/minor version are supported.

    """
    return implementation == SUPPORTED_IMPLEMENTATION and version == SUPPORTED_VERSION


def main() -> int:
    """Enter the application only on its declared interpreter.

    Returns:
        The application status, or one for an unsupported interpreter.

    """
    implementation = platform.python_implementation()
    version = sys.version_info[:2]
    if not runtime_supported(implementation, version):
        requirement = (
            f"{SUPPORTED_IMPLEMENTATION} "
            f"{SUPPORTED_VERSION[0]}.{SUPPORTED_VERSION[1]}.x"
        )
        sys.stderr.write(
            f"EML Attachment Remover requires {requirement}; "
            f"found {implementation} {version[0]}.{version[1]}\n",
        )
        return UNSUPPORTED_RUNTIME_STATUS

    from .app import (  # ruff: ignore[import-outside-top-level]
        main as application_main,
    )

    return application_main()
