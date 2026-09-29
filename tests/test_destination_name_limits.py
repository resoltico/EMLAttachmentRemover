"""Filename limits and native units for automatically derived output names."""

from __future__ import annotations

import os
from typing import TYPE_CHECKING

import pytest

from eml_attachment_remover import destination_names

if TYPE_CHECKING:
    from pathlib import Path


def test_native_units_are_utf8_bytes_on_posix_and_utf16_units_on_windows(
    monkeypatch: pytest.MonkeyPatch,
) -> None:
    """A name is measured the way its backend stores it (finding 7)."""
    with monkeypatch.context() as posix_backend:
        posix_backend.setattr(os, "name", "posix")
        assert destination_names.native_length("ē😀a") == 2 + 4 + 1
        assert destination_names.native_bytes("ē") == "ē".encode()
    with monkeypatch.context() as windows:
        windows.setattr(os, "name", "nt")
        # A supplementary character is two UTF-16 units; a lone surrogate is one.
        assert destination_names.native_length("ē😀a") == 1 + 2 + 1
        assert destination_names.native_length("\ud800") == 1
        # A length is a whole number of units, never a fraction.
        assert type(destination_names.native_length("a")) is int
        assert destination_names.native_bytes("a") == b"a\x00"


def test_limit_comes_from_the_directory_the_kernel_will_traverse(
    tmp_path: Path, monkeypatch: pytest.MonkeyPatch
) -> None:
    """POSIX asks pathconf about the directory expression; empty means here."""
    pathconf = getattr(os, "pathconf", None)
    real = 255 if pathconf is None else pathconf(str(tmp_path), "PC_NAME_MAX")
    assert destination_names.name_limit(str(tmp_path)) == real
    asked: list[tuple[str, str]] = []

    def fake(directory: str, name: str) -> int:
        asked.append((directory, name))
        return 99

    monkeypatch.setattr(os, "name", "posix")
    monkeypatch.setattr(os, "pathconf", fake, raising=False)
    assert destination_names.name_limit("") == 99
    assert destination_names.name_limit("in/../out") == 99
    assert asked == [(".", "PC_NAME_MAX"), ("in/../out", "PC_NAME_MAX")]


@pytest.mark.parametrize("answer", [1, 2, 255, 1023])
def test_any_positive_kernel_answer_is_the_limit(
    answer: int, monkeypatch: pytest.MonkeyPatch
) -> None:
    """The limit is the kernel's answer whenever it is positive, even a tiny one."""
    monkeypatch.setattr(os, "name", "posix")
    monkeypatch.setattr(os, "pathconf", lambda *_args: answer, raising=False)
    assert destination_names.name_limit("anywhere") == answer


@pytest.mark.parametrize("answer", [OSError("gone"), ValueError("bad"), -1, 0])
def test_unanswerable_limit_keeps_the_conservative_common_limit(
    answer: object, monkeypatch: pytest.MonkeyPatch
) -> None:
    """A missing directory or unlimited answer never shortens names below 255."""

    def fake(_directory: str, _name: str) -> int:
        if isinstance(answer, Exception):
            raise answer
        return int(str(answer))

    monkeypatch.setattr(os, "name", "posix")
    monkeypatch.setattr(os, "pathconf", fake, raising=False)
    assert destination_names.name_limit("anywhere") == 255


def test_windows_and_pathconf_less_platforms_use_the_common_limit(
    monkeypatch: pytest.MonkeyPatch,
) -> None:
    """NTFS, ReFS, and FAT share 255 UTF-16 units; no pathconf means the same."""
    with monkeypatch.context() as windows:
        windows.setattr(os, "name", "nt")
        # A kernel answer is never consulted on Windows, even if one were offered.
        windows.setattr(os, "pathconf", lambda *_args: 99, raising=False)
        assert destination_names.name_limit("C:\\out") == 255
    monkeypatch.setattr(os, "name", "posix")
    monkeypatch.delattr(os, "pathconf", raising=False)
    assert destination_names.name_limit("/out") == 255
