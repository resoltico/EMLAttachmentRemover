"""Wheel-tag build hook contracts for the one supported CPython minor."""

from __future__ import annotations

from types import SimpleNamespace
from typing import TYPE_CHECKING, cast

import pytest
from tools.hatch_build import CustomBuildHook, wheel_tag

if TYPE_CHECKING:
    from pathlib import Path


def _project(path: Path, *, requires: str, implementation: str = "CPython") -> Path:
    config = path / "pyproject.toml"
    config.write_text(
        "\n".join((
            "[project]",
            f'requires-python = "{requires}"',
            "[tool.eml-attachment-remover.runtime]",
            f'implementation = "{implementation}"',
        )),
        encoding="utf-8",
    )
    return config


def test_wheel_tag_requires_one_exact_cpython_minor(tmp_path: Path) -> None:
    config = _project(tmp_path, requires=">=3.14,<3.15")
    assert wheel_tag(config) == "cp314-none-any"
    for requires, implementation in ((">=3.14,<3.16", "CPython"), (">=3.14", "PyPy")):
        invalid = _project(tmp_path, requires=requires, implementation=implementation)
        with pytest.raises(ValueError, match="wheel tag"):
            wheel_tag(invalid)


def test_build_hook_sets_the_metadata_derived_tag(tmp_path: Path) -> None:
    _project(tmp_path, requires=">=3.14,<3.15")
    hook = cast("CustomBuildHook", SimpleNamespace(root=str(tmp_path)))
    build_data: dict[str, object] = {}
    CustomBuildHook.initialize(hook, "3.0.0", build_data)
    assert build_data == {"tag": "cp314-none-any"}
