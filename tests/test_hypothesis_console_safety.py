"""Property-test terminal-safe rendering of untrusted message metadata."""

from __future__ import annotations

import io
import unicodedata
from typing import Final

from hypothesis import event, example, given
from hypothesis import strategies as st

from eml_attachment_remover import models

UNSAFE_CATEGORIES: Final = ("Cc", "Cf", "Cs", "Zl", "Zp")
UNSAFE_CHARACTER: Final[st.SearchStrategy[str]] = st.characters(
    categories=UNSAFE_CATEGORIES,
)


@example(character="\N{CONTROL SEQUENCE INTRODUCER}")
@example(character="\N{RIGHT-TO-LEFT OVERRIDE}")
@example(character="\N{LINE SEPARATOR}")
@example(character="\U000e0001")
@given(character=UNSAFE_CHARACTER)
def test_generated_terminal_controls_are_rendered_as_visible_ascii(
    character: str,
) -> None:
    """Never emit a generated control, format, surrogate, or line separator."""
    rendered = models._display_text(  # ruff: ignore[private-member-access]
        f"public-before{character}public-after",
        io.StringIO(),
    )
    event(f"terminal-category={unicodedata.category(character)}")
    event(f"terminal-escape-width={len(rendered) - len('public-beforepublic-after')}")
    assert character not in rendered
    assert rendered.startswith("public-before\\")
    assert rendered.endswith("public-after")
    assert rendered.isascii()


def test_terminal_escape_width_is_unambiguous_for_each_unicode_range() -> None:
    """Use fixed-width Python-style escapes across all code-point ranges."""
    rendered = models._display_text(  # ruff: ignore[private-member-access]
        "\x1b\x9b\u202e\u2028\U000e0001",
        io.StringIO(),
    )
    assert rendered == r"\x1b\x9b\u202e\u2028\U000e0001"
