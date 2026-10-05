"""Automatic output names fit their directory's limit; explicit names never change."""

from __future__ import annotations

import hashlib
import os
from pathlib import Path

import pytest

from eml_attachment_remover import destination_names

MESSAGE = b"From: a@example.test\r\n\r\nretained\r\n"
SUFFIX = ".mime-pruned.eml"


def _tail(name: str) -> str:
    """Return the hash-and-suffix tail, computed independently of the fitter.

    Returns:
        ``-<16 hex of sha256(native name)>.mime-pruned.eml``; the native form is
        UTF-16 on Windows and UTF-8 elsewhere, following the active ``os.name``.

    """
    encoding = "utf-16-le" if os.name == "nt" else "utf-8"
    digest = hashlib.sha256(name.encode(encoding, "surrogatepass")).hexdigest()
    return f"-{digest[:16]}{SUFFIX}"


@pytest.fixture
def posix(monkeypatch: pytest.MonkeyPatch) -> None:
    """Measure names as a POSIX backend does, whatever host runs the test.

    Only string helpers run under this, never pathlib, which reads ``os.name``.
    """
    monkeypatch.setattr(os, "name", "posix")


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
@pytest.mark.usefixtures("posix")
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


@pytest.mark.usefixtures("posix")
def test_multibyte_names_are_budgeted_in_bytes_without_splitting() -> None:
    """A cut lands between whole characters and the total stays within the limit."""
    source = "ē" * 200 + ".eml"
    fitted = destination_names.fit_name(source, "ē" * 200 + SUFFIX, 255)
    assert len(fitted.encode()) <= 255
    assert fitted.endswith(_tail(source))
    assert set(fitted.removesuffix(_tail(source))) == {"ē"}
    # One more prefix character would not fit the budget.
    assert len((fitted + "ē").encode()) > 255


@pytest.mark.usefixtures("posix")
def test_a_backslash_is_an_ordinary_posix_filename_character(
    monkeypatch: pytest.MonkeyPatch,
) -> None:
    """POSIX grammar splits only at slashes, so a backslash stays in the name."""
    _limit(monkeypatch, 255)
    fitted = destination_names.fitted_default_destination
    assert fitted("dir/a\\b.eml", None) == "dir/a\\b.mime-pruned.eml"
    long_source = "dir/" + "x" * 100 + "\\" + "y" * 200 + ".eml"
    name = long_source.removeprefix("dir/")
    _limit(monkeypatch, 255)
    assert fitted(long_source, None) == "dir/" + name[: 255 - len(_tail(name))] + _tail(
        name
    )


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
    """Pin the directory limit and record which directories were asked about.

    Returns:
        The directory expressions, in the order they were queried.

    """
    asked: list[str] = []

    def name_limit(directory: str) -> int:
        asked.append(directory)
        return limit

    monkeypatch.setattr(destination_names, "name_limit", name_limit)
    return asked


@pytest.mark.usefixtures("posix")
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
    assert asked == ["parent/../inbox", ""]


@pytest.mark.usefixtures("posix")
def test_overlong_names_are_fitted_beside_the_source(
    monkeypatch: pytest.MonkeyPatch,
) -> None:
    """Beside the source, the limit is asked of the source's parent expression."""
    asked = _limit(monkeypatch, 60)
    source = "s" * 90 + ".eml"
    name = "s" * (60 - len(_tail(source))) + _tail(source)
    fitted = destination_names.fitted_default_destination
    assert fitted("in/../" + source, None) == "in/../" + name
    assert asked == ["in/.."]


def test_output_directory_names_are_fitted_to_that_directory(
    monkeypatch: pytest.MonkeyPatch,
) -> None:
    """With --output-dir, the limit is asked of that directory, in the host grammar."""
    asked = _limit(monkeypatch, 60)
    source = "s" * 90 + ".eml"
    tail = _tail(source)
    name = "s" * (60 - len(tail)) + tail
    fitted = destination_names.fitted_default_destination(source, "out/dir")
    assert fitted == os.fspath(Path("out/dir") / name)
    assert asked == ["out/dir"]


def test_windows_grammar_keeps_its_parent_expression(
    monkeypatch: pytest.MonkeyPatch,
) -> None:
    """Drive letters and backslashes survive fitting untouched."""
    monkeypatch.setattr(os, "name", "nt")
    source = "w" * 300 + ".eml"
    tail = _tail(source)
    name = "w" * (255 - len(tail)) + tail
    fitted = destination_names.fitted_default_destination
    assert fitted("C:\\in\\..\\" + source, None) == "C:\\in\\..\\" + name
    assert fitted("C:\\in\\Short.eml", None) == "C:\\in\\Short.mime-pruned.eml"
