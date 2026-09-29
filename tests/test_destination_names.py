"""Automatic output names fit their directory's limit; explicit names never change."""

from __future__ import annotations

import hashlib
import os
from typing import TYPE_CHECKING

import pytest

from eml_attachment_remover import destination_names

if TYPE_CHECKING:
    from pathlib import Path

MESSAGE = b"From: a@example.test\r\n\r\nretained\r\n"
SUFFIX = ".mime-pruned.eml"


def _tail(name: str, encoding: str = "utf-8") -> str:
    """Return the hash-and-suffix tail, computed independently of the fitter.

    Returns:
        ``-<16 hex of sha256(native name)>.mime-pruned.eml``.

    """
    digest = hashlib.sha256(name.encode(encoding, "surrogatepass")).hexdigest()
    return f"-{digest[:16]}{SUFFIX}"


def test_native_units_are_utf8_bytes_on_posix_and_utf16_units_on_windows(
    monkeypatch: pytest.MonkeyPatch,
) -> None:
    """A name is measured the way its backend stores it (finding 7)."""
    assert destination_names.native_length("ē😀a") == 2 + 4 + 1
    assert destination_names.native_bytes("ē") == "ē".encode()
    with monkeypatch.context() as windows:
        windows.setattr(os, "name", "nt")
        # A supplementary character is two UTF-16 units; a lone surrogate is one.
        assert destination_names.native_length("ē😀a") == 1 + 2 + 1
        assert destination_names.native_length("\ud800") == 1
        assert destination_names.native_bytes("a") == b"a\x00"


def test_limit_comes_from_the_directory_the_kernel_will_traverse(
    tmp_path: Path, monkeypatch: pytest.MonkeyPatch
) -> None:
    """POSIX asks pathconf about the directory expression; empty means here."""
    assert destination_names.name_limit(str(tmp_path)) == os.pathconf(
        str(tmp_path), "PC_NAME_MAX"
    )
    asked: list[tuple[str, str]] = []

    def pathconf(directory: str, name: str) -> int:
        asked.append((directory, name))
        return 99

    monkeypatch.setattr(os, "pathconf", pathconf)
    assert destination_names.name_limit("") == 99
    assert destination_names.name_limit("in/../out") == 99
    assert asked == [(".", "PC_NAME_MAX"), ("in/../out", "PC_NAME_MAX")]


@pytest.mark.parametrize("answer", [OSError("gone"), ValueError("bad"), -1, 0])
def test_unanswerable_limit_keeps_the_conservative_common_limit(
    answer: object, monkeypatch: pytest.MonkeyPatch
) -> None:
    """A missing directory or unlimited answer never shortens names below 255."""

    def pathconf(_directory: str, _name: str) -> int:
        if isinstance(answer, Exception):
            raise answer
        return int(str(answer))

    monkeypatch.setattr(os, "pathconf", pathconf)
    assert destination_names.name_limit("anywhere") == 255


def test_windows_and_pathconf_less_platforms_use_the_common_limit(
    monkeypatch: pytest.MonkeyPatch,
) -> None:
    """NTFS, ReFS, and FAT share 255 UTF-16 units; no pathconf means the same."""
    with monkeypatch.context() as windows:
        windows.setattr(os, "name", "nt")
        assert destination_names.name_limit("C:\\out") == 255
    monkeypatch.delattr(os, "pathconf")
    assert destination_names.name_limit("/out") == 255


@pytest.mark.parametrize(
    ("text", "budget", "expected"),
    [
        ("ēēē", 6, "ēēē"),
        ("ēēē", 5, "ēē"),
        ("ēēē", 1, ""),
        ("ēēē", 0, ""),
        ("ēēē", -7, ""),
        ("abc", 2, "ab"),
        ("", 5, ""),
    ],
)
def test_prefix_keeps_whole_characters_within_the_budget(
    text: str, budget: int, expected: str
) -> None:
    """A cut never splits a multi-byte character."""
    assert destination_names._prefix(text, budget) == expected  # ruff: ignore[private-member-access] - character-boundary receipt.


def test_prefix_never_splits_a_surrogate_pair_on_windows(
    monkeypatch: pytest.MonkeyPatch,
) -> None:
    """Supplementary characters cost two UTF-16 units and are kept or dropped whole."""
    monkeypatch.setattr(os, "name", "nt")
    assert destination_names._prefix("a😀b", 2) == "a"  # ruff: ignore[private-member-access] - surrogate-pair receipt.
    assert destination_names._prefix("a😀b", 3) == "a😀"  # ruff: ignore[private-member-access] - surrogate-pair receipt.


def test_fitting_name_is_returned_unchanged_up_to_the_limit() -> None:
    """A name that fits, exactly, is never altered."""
    derived = "a" * 239 + SUFFIX
    assert len(derived) == 255
    assert destination_names.fit_name("a" * 239 + ".eml", derived, 255) == derived
    assert destination_names.fit_name("s.eml", "s" + SUFFIX, 255) == "s" + SUFFIX


@pytest.mark.parametrize("limit", [255, 143, 40])
def test_overlong_name_keeps_a_readable_prefix_and_a_stable_digest(limit: int) -> None:
    """The derived name shrinks to exactly the limit, ending in its own digest."""
    source = "a" * 300 + ".eml"
    fitted = destination_names.fit_name(source, "a" * 300 + SUFFIX, limit)
    tail = _tail(source)
    assert fitted == "a" * (limit - len(tail)) + tail
    assert len(fitted) == limit
    assert fitted == destination_names.fit_name(source, "a" * 300 + SUFFIX, limit)


def test_digest_covers_the_whole_original_name() -> None:
    """Names sharing a long prefix, or differing only in case or extension, differ."""
    limit = 60
    long_prefix = "p" * 100
    outputs = {
        destination_names.fit_name(name, name.removesuffix(".eml") + SUFFIX, limit)
        for name in (
            long_prefix + "1.eml",
            long_prefix + "2.eml",
            long_prefix + "1.EML",
            long_prefix + "1.txt",
        )
    }
    assert len(outputs) == 4


def test_only_a_terminal_eml_suffix_is_replaced_in_the_kept_prefix() -> None:
    """The stem keeps a non-.eml extension, and .EML matches without regard to case."""
    limit = 60
    text = destination_names.fit_name(
        "z" * 80 + ".txt", "z" * 80 + ".txt" + SUFFIX, limit
    )
    assert text == "z" * (limit - len(_tail("z" * 80 + ".txt"))) + _tail(
        "z" * 80 + ".txt"
    )
    upper = destination_names.fit_name("y" * 80 + ".EML", "y" * 80 + SUFFIX, limit)
    assert upper == "y" * (limit - len(_tail("y" * 80 + ".EML"))) + _tail(
        "y" * 80 + ".EML"
    )


def test_multibyte_names_are_budgeted_in_bytes_without_splitting() -> None:
    """A cut lands between whole characters and the total stays within the limit."""
    source = "ē" * 200 + ".eml"
    fitted = destination_names.fit_name(source, "ē" * 200 + SUFFIX, 255)
    assert len(fitted.encode()) <= 255
    assert fitted.endswith(_tail(source))
    assert set(fitted.removesuffix(_tail(source))) == {"ē"}
    # One more prefix character would not fit the budget.
    assert len((fitted + "ē").encode()) > 255


def test_a_limit_smaller_than_the_tail_leaves_only_the_tail() -> None:
    """The tail alone is the floor; a hopeless limit still yields a stable name."""
    source = "q" * 50 + ".eml"
    assert destination_names.fit_name(source, "q" * 50 + SUFFIX, 5) == _tail(source)


def test_windows_names_are_measured_in_utf16_units(
    monkeypatch: pytest.MonkeyPatch,
) -> None:
    """On Windows a supplementary character costs two units of the 255 budget."""
    monkeypatch.setattr(os, "name", "nt")
    source = "😀" * 130 + ".eml"
    fitted = destination_names.fit_name(source, "😀" * 130 + SUFFIX, 255)
    units = destination_names.native_length(fitted)
    assert units <= 255
    assert destination_names.native_length(fitted + "😀") > 255
    expected = hashlib.sha256(source.encode("utf-16-le", "surrogatepass"))
    assert fitted.endswith(f"-{expected.hexdigest()[:16]}{SUFFIX}")


def _limit(monkeypatch: pytest.MonkeyPatch, limit: int) -> list[str]:
    asked: list[str] = []

    def pathconf(directory: str, _name: str) -> int:
        asked.append(directory)
        return limit

    monkeypatch.setattr(os, "pathconf", pathconf)
    return asked


def test_parent_expression_and_short_names_are_preserved(
    monkeypatch: pytest.MonkeyPatch,
) -> None:
    """A name that fits keeps the request's parent expression byte for byte."""
    asked = _limit(monkeypatch, 255)
    fitted = destination_names.fitted_default_destination
    assert fitted("parent/../inbox/Message.EML", None) == (
        "parent/../inbox/Message.mime-pruned.eml"
    )
    assert fitted("Message.EML", None) == "Message.mime-pruned.eml"
    assert asked == ["parent/../inbox", "."]


def test_overlong_names_are_fitted_beside_the_source_or_in_the_output_directory(
    monkeypatch: pytest.MonkeyPatch,
) -> None:
    """The limit is asked of the directory that will receive the copy."""
    asked = _limit(monkeypatch, 60)
    source = "s" * 90 + ".eml"
    name = "s" * (60 - len(_tail(source))) + _tail(source)
    fitted = destination_names.fitted_default_destination
    assert fitted("in/../" + source, None) == "in/../" + name
    assert fitted("in/" + source, "out/dir") == "out/dir/" + name
    assert asked == ["in/..", "out/dir"]


def test_windows_grammar_keeps_its_parent_expression(
    monkeypatch: pytest.MonkeyPatch,
) -> None:
    """Drive letters and backslashes survive fitting untouched."""
    monkeypatch.setattr(os, "name", "nt")
    source = "w" * 300 + ".eml"
    tail = _tail(source, "utf-16-le")
    name = "w" * (255 - len(tail)) + tail
    fitted = destination_names.fitted_default_destination
    assert fitted("C:\\in\\..\\" + source, None) == "C:\\in\\..\\" + name
    assert fitted("C:\\in\\Short.eml", None) == "C:\\in\\Short.mime-pruned.eml"
