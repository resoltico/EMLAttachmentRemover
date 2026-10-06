"""Validate build identities independently of Git, CI and compilation order."""

from __future__ import annotations

from typing import TYPE_CHECKING

import pytest
from tools.macos_metadata import build_number

if TYPE_CHECKING:
    from pathlib import Path


@pytest.mark.parametrize("value", [1, 5, 9999])
def test_build_number_survives_a_source_tree_without_git(
    tmp_path: Path, value: int
) -> None:
    configuration = tmp_path / "pyproject.toml"
    configuration.write_text(
        f"[tool.eml-attachment-remover.macos]\nbuild-number = {value}\n"
    )
    assert build_number(configuration) == str(value)
    assert build_number(configuration) == str(value)


@pytest.mark.parametrize("value", ["true", '"5"', "0", "-1", "10000", "5.0"])
def test_invalid_build_numbers_are_refused(tmp_path: Path, value: str) -> None:
    configuration = tmp_path / "pyproject.toml"
    configuration.write_text(
        f"[tool.eml-attachment-remover.macos]\nbuild-number = {value}\n"
    )
    with pytest.raises(
        ValueError, match=r"^Native build-number must be an integer from 1 to 9999$"
    ):
        build_number(configuration)


def test_absent_build_number_has_actionable_guidance(tmp_path: Path) -> None:
    configuration = tmp_path / "pyproject.toml"
    configuration.write_text('[project]\nversion = "4.0.0"\n')
    with pytest.raises(
        ValueError, match=r"^Declare tool\.eml-attachment-remover\.macos\.build-number$"
    ):
        build_number(configuration)
