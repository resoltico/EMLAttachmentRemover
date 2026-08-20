"""Project safe readable text from an HTML body without retaining markup."""

from __future__ import annotations

import re
from html.parser import HTMLParser
from typing import TYPE_CHECKING, Final

if TYPE_CHECKING:
    from .models import MimePath

BLOCK_ELEMENTS: Final = frozenset({
    "address",
    "article",
    "aside",
    "blockquote",
    "div",
    "dl",
    "dt",
    "dd",
    "fieldset",
    "figcaption",
    "figure",
    "footer",
    "form",
    "h1",
    "h2",
    "h3",
    "h4",
    "h5",
    "h6",
    "header",
    "main",
    "nav",
    "p",
    "section",
})
HIDDEN_ELEMENTS: Final = frozenset({"head", "script", "style", "template"})
LIST_ELEMENTS: Final = frozenset({"ol", "ul"})
WHITESPACE_PATTERN: Final = re.compile(r"[\t\f\v ]+")
ANY_WHITESPACE_PATTERN: Final = re.compile(r"\s+")
BLANK_LINES_PATTERN: Final = re.compile(r"\n{3,}")


class _ReadableTextParser(HTMLParser):
    """Render a bounded semantic subset of decoded HTML into plain text."""

    def __init__(self) -> None:
        """Initialize empty output and structural state."""
        super().__init__()
        self._pieces: list[str] = []
        self._hidden_depth = 0
        self._lists: list[tuple[str, int]] = []

    def _block(self) -> None:
        """Separate visible blocks without accumulating unbounded whitespace."""
        if self._pieces:
            self._pieces.append("\n\n")

    def _line(self) -> None:
        """Separate adjacent visible lines."""
        if self._pieces and not self._pieces[-1].endswith("\n"):
            self._pieces.append("\n")

    def _list_prefix(self) -> str:
        """Return the deterministic marker for the current list item.

        Returns:
            One plain-text ordered or unordered list marker.

        """
        if not self._lists:
            return "- "
        kind, ordinal = self._lists[-1]
        ordinal += 1
        self._lists[-1] = kind, ordinal
        return f"{ordinal}. " if kind == "ol" else "- "

    def handle_starttag(
        self,
        tag: str,
        attrs: list[tuple[str, str | None]],
    ) -> None:
        """Render one non-void opening tag's semantic boundary."""
        del attrs
        folded = tag.casefold()
        if folded in HIDDEN_ELEMENTS:
            self._hidden_depth += 1
        elif not self._hidden_depth:
            if folded in BLOCK_ELEMENTS or folded in {"table", "tr"}:
                self._block()
            elif folded == "br":
                self._line()
            elif folded in LIST_ELEMENTS:
                self._block()
                self._lists.append((folded, 0))
            elif folded == "li":
                self._line()
                self._pieces.append(self._list_prefix())
            elif folded in {"td", "th"}:
                self._pieces.append("\t")

    def handle_startendtag(
        self,
        tag: str,
        attrs: list[tuple[str, str | None]],
    ) -> None:
        """Render self-closing semantic tags without retaining resource data."""
        del attrs
        if not self._hidden_depth and tag.casefold() == "br":
            self._line()

    def handle_endtag(self, tag: str) -> None:
        """Close one semantic boundary and hidden-element scope."""
        folded = tag.casefold()
        if folded in HIDDEN_ELEMENTS:
            self._hidden_depth = max(0, self._hidden_depth - 1)
        elif not self._hidden_depth:
            if folded in LIST_ELEMENTS:
                if self._lists:
                    self._lists.pop()
                self._block()
            elif folded == "li":
                self._line()
            elif folded in BLOCK_ELEMENTS or folded == "tr":
                self._block()

    def handle_data(self, data: str) -> None:
        """Keep only decoded visible text nodes."""
        if not self._hidden_depth:
            self._pieces.append(data)

    def text(self) -> str:
        """Return canonical readable plain text ending in one newline.

        Returns:
            Visible text with normalized whitespace and block separation.

        """
        rendered = "".join(self._pieces).replace("\r\n", "\n").replace("\r", "\n")
        rendered = WHITESPACE_PATTERN.sub(" ", rendered)
        rendered = re.sub(r" *\n *", "\n", rendered)
        rendered = BLANK_LINES_PATTERN.sub("\n\n", rendered).strip()
        return f"{rendered}\n" if rendered else ""


def project_html_text(source: str) -> str:
    """Return the readable plaintext projection of one decoded HTML body.

    Returns:
        Visible semantic text with deterministic blocks and list markers.

    """
    parser = _ReadableTextParser()
    parser.feed(source)
    parser.close()
    return parser.text()


def text_equivalent(plain: str, projection: str) -> bool:
    """Return whether two text representations differ only in layout whitespace.

    Returns:
        ``True`` when the rendered HTML preserves every non-whitespace character.

    """
    return ANY_WHITESPACE_PATTERN.sub("", plain) == ANY_WHITESPACE_PATTERN.sub(
        "",
        projection,
    )


def equivalent_html_layout_source(
    plain: str,
    sources: tuple[tuple[MimePath, str], ...],
) -> tuple[MimePath, str] | None:
    """Return the one equivalent HTML source path and its readable projection.

    Returns:
        The source MIME path plus projected text, or ``None`` when ambiguous.

    """
    projections = tuple(
        (path, projection)
        for path, source in sources
        if text_equivalent(plain, projection := project_html_text(source))
    )
    return projections[0] if len(projections) == 1 else None
