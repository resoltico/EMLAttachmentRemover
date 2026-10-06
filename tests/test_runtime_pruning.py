"""Processor runtime trimming preserves processing imports, links and license data."""

from __future__ import annotations

import json
from typing import TYPE_CHECKING

import pytest
from tools.runtime_pruning import prune

if TYPE_CHECKING:
    from pathlib import Path


def test_nonprocessing_components_are_removed_without_touching_processing_files(
    tmp_path: Path,
) -> None:
    removed = [
        "include/Python.h",
        "lib/pkgconfig/python.pc",
        "lib/python3.14/config-3.14-darwin/Makefile",
        "lib/python3.14/config-3.14-darwin/python.o",
        "share/man/man1/python3.14.1",
        "bin/pip3.14",
        "bin/idle3",
        "bin/pydoc3",
        "bin/python3.14-config",
        "lib/libtcl9.0.dylib",
        "lib/tcl9.0/code.tcl",
        "lib/tk9.0/code.tcl",
        "lib/itcl4.3/code.tcl",
        "lib/thread3.0/code.tcl",
        "lib/python3.14/test/example.py",
        "lib/python3.14/idlelib/example.py",
        "lib/python3.14/tkinter/example.py",
        "lib/python3.14/turtledemo/example.py",
        "lib/python3.14/turtle.py",
        "lib/python3.14/ensurepip/example.py",
        "lib/python3.14/site-packages/pip/example.py",
        "lib/python3.14/lib-dynload/_tkinter.cpython-314-darwin.so",
        "lib/python3.14/lib-dynload/_testcapi.cpython-314-darwin.so",
        "lib/python3.14/lib-dynload/_ctypes_test.cpython-314-darwin.so",
    ]
    retained = [
        "bin/python3.14",
        "lib/libpython3.14.dylib",
        "PYTHON.json",
        "licenses/LICENSE.cpython.txt",
        "licenses/LICENSE.tcl.txt",
        "lib/python3.14/email/__init__.py",
        "lib/python3.14/encodings/__init__.py",
        "lib/python3.14/ssl.py",
        "lib/python3.14/lib-dynload/_ssl.so",
    ]
    for name in removed + retained:
        path = tmp_path / name
        path.parent.mkdir(parents=True, exist_ok=True)
        path.write_bytes(name.encode())
    (tmp_path / "bin/python3").symlink_to("python3.14")
    prune(tmp_path)
    assert all(not (tmp_path / name).exists() for name in removed)
    assert all((tmp_path / name).read_bytes() == name.encode() for name in retained)
    assert (tmp_path / "bin/python3").readlink().as_posix() == "python3.14"
    assert not (tmp_path / "share").exists()


def test_pruning_a_component_link_does_not_delete_its_target(tmp_path: Path) -> None:
    stdlib = tmp_path / "lib/python3.14"
    stdlib.mkdir(parents=True)
    target = tmp_path / "retained"
    target.mkdir()
    (target / "keep").write_bytes(b"retained")
    (stdlib / "site-packages").symlink_to(target, target_is_directory=True)
    prune(tmp_path)
    assert not (stdlib / "site-packages").exists()
    assert (target / "keep").read_bytes() == b"retained"


@pytest.mark.parametrize(
    ("arguments", "removed"),
    [
        ("'--enable-static-libpython-for-interpreter' --enable-shared", True),
        ("--enable-shared", False),
        ("--enable-static-libpython-for-interpreter=no", False),
        ("'unterminated", False),
    ],
)
def test_embedding_library_is_removed_only_for_an_explicit_static_interpreter(
    tmp_path: Path, arguments: str, *, removed: bool
) -> None:
    """Unknown or dynamic configurations cannot lose their Python shared library."""
    library = tmp_path / "lib/libpython3.14.dylib"
    library.parent.mkdir()
    library.write_bytes(b"embedding library")
    (tmp_path / "PYTHON.json").write_text(
        json.dumps({"python_config_vars": {"CONFIG_ARGS": arguments}})
    )
    prune(tmp_path)
    assert library.exists() is not removed


@pytest.mark.parametrize("metadata", [[], {}, {"python_config_vars": []}])
def test_unknown_embedding_configuration_preserves_the_library(
    tmp_path: Path,
    metadata: object,
) -> None:
    library = tmp_path / "lib/libpython3.14.dylib"
    library.parent.mkdir()
    library.write_bytes(b"retained library")
    (tmp_path / "PYTHON.json").write_text(json.dumps(metadata))
    share = tmp_path / "share"
    share.mkdir()
    (share / "retained.txt").write_bytes(b"retained document")
    prune(tmp_path)
    assert library.read_bytes() == b"retained library"
    assert (share / "retained.txt").read_bytes() == b"retained document"
