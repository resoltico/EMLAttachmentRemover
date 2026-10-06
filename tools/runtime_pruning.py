"""Remove development, package-management and Tk components from processor runtimes."""

from __future__ import annotations

import json
import shlex
import shutil
from typing import TYPE_CHECKING, Final

if TYPE_CHECKING:
    from pathlib import Path

STDLIB_COMPONENTS: Final = (
    "test",
    "idlelib",
    "tkinter",
    "turtledemo",
    "turtle.py",
    "ensurepip",
    "site-packages",
)
BIN_COMPONENTS: Final = (
    "pip",
    "pip3",
    "pip3.14",
    "idle3",
    "idle3.14",
    "pydoc3",
    "pydoc3.14",
    "python3-config",
    "python3.14-config",
)


def prune(runtime: Path) -> None:
    """Keep processing modules and notices while omitting non-processing components."""
    library = runtime / "lib"
    stdlib = library / "python3.14"
    paths = [runtime / "include", library / "pkgconfig", runtime / "share/man"]
    paths.extend(stdlib.glob("config-*"))
    if _static_interpreter(runtime):
        paths.extend(library.glob("libpython*.dylib"))
    paths.extend(stdlib / name for name in STDLIB_COMPONENTS)
    paths.extend(runtime / "bin" / name for name in BIN_COMPONENTS)
    for pattern in ("_tkinter*", "_test*.so", "_ctypes_test*.so"):
        paths.extend((stdlib / "lib-dynload").glob(pattern))
    if library.is_dir():
        paths.extend(
            path
            for path in library.iterdir()
            if path.name.startswith(("tcl", "tk", "itcl", "thread", "libtcl"))
        )
    for path in paths:
        if path.is_symlink() or path.is_file():
            path.unlink()
        elif path.is_dir():
            shutil.rmtree(path)

    share = runtime / "share"
    if share.is_dir() and not share.is_symlink() and not any(share.iterdir()):
        share.rmdir()


def _static_interpreter(runtime: Path) -> bool:
    """Recognize an explicit upstream build option before omitting embedding libraries.

    Returns:
        True only when source metadata declares a statically linked interpreter.

    """
    try:
        metadata = json.loads((runtime / "PYTHON.json").read_text(encoding="utf-8"))
        config = (
            metadata.get("python_config_vars", {}) if isinstance(metadata, dict) else {}
        )
        arguments = config.get("CONFIG_ARGS") if isinstance(config, dict) else None
        return isinstance(
            arguments, str
        ) and "--enable-static-libpython-for-interpreter" in shlex.split(arguments)
    except OSError, ValueError:
        return False
