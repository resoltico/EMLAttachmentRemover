"""Collect actual URI-bearing values from decoded HTML markup."""

from __future__ import annotations

import re
from html.parser import HTMLParser
from typing import Final

URI_ATTRIBUTE_ELEMENTS: Final = {
    "background": frozenset({"body", "table", "td", "th"}),
    "data": frozenset({"object"}),
    "poster": frozenset({"video"}),
    "src": frozenset({
        "audio",
        "embed",
        "iframe",
        "img",
        "input",
        "script",
        "source",
        "track",
        "video",
    }),
}
SRCSET_ATTRIBUTE_ELEMENTS: Final = {
    "imagesrcset": frozenset({"link"}),
    "srcset": frozenset({"img", "source"}),
}
RENDERING_HREF_ELEMENTS: Final = frozenset({
    "feimage",
    "image",
    "link",
    "use",
})
RENDERING_LINK_RELATIONS: Final = frozenset({
    "icon",
    "modulepreload",
    "preload",
    "stylesheet",
})
VML_NAMESPACE: Final = "urn:schemas-microsoft-com:vml"
VML_SOURCE_ELEMENTS: Final = frozenset({"background", "fill", "imagedata"})
HTML_VOID_ELEMENTS: Final = frozenset({
    "area",
    "base",
    "br",
    "col",
    "embed",
    "hr",
    "img",
    "input",
    "link",
    "meta",
    "param",
    "source",
    "track",
    "wbr",
})
type NamespaceFrame = tuple[str, dict[str, str | None]]
MAX_MSO_CONDITIONAL_COMMENT_LENGTH: Final = 65_536
MAX_MSO_CONDITIONAL_DEPTH: Final = 4
QUALIFIED_NAME_COMPONENTS: Final = 2
EMPTY_ATTRIBUTE_VALUE: Final = ""
STYLE_INSIDE: Final = object()
MSO_HIDDEN_CONDITIONAL_PATTERN: Final = re.compile(
    r"\A\s*\[if\s+(?:mso(?:\s+[0-9]+(?:\.[0-9]+)?)?|"
    r"(?:gt|gte|lt|lte)\s+mso\s+[0-9]+(?:\.[0-9]+)?)\s*\]>"
    r"([\s\S]*)<!\[endif\]\s*\Z",
    re.IGNORECASE,
)


def _link_can_render(attributes: dict[str, str]) -> bool:
    """Return whether one link relation can load a rendered resource.

    Returns:
        ``True`` when at least one relation is rendering-relevant.

    """
    relation_value = attributes.get("rel")
    if relation_value is None:
        return False
    relations = frozenset(relation_value.casefold().split())
    return bool(RENDERING_LINK_RELATIONS & relations)


def _is_uri_attribute(
    tag: str,
    name: str,
    attributes: dict[str, str],
) -> bool:
    """Return whether this element/attribute pair bears a rendered URI.

    Returns:
        ``True`` when the attribute can load rendered content.

    """
    if tag == "input" and name == "src":
        input_type = attributes.get("type")
        return input_type is not None and input_type.casefold() == "image"
    if tag == "link" and name in {"href", "xlink:href"}:
        return _link_can_render(attributes)
    return tag in URI_ATTRIBUTE_ELEMENTS.get(name, ()) or (
        name in {"href", "xlink:href"} and tag in RENDERING_HREF_ELEMENTS
    )


def _is_srcset_attribute(
    tag: str,
    name: str,
    attributes: dict[str, str],
) -> bool:
    """Return whether this pair defines rendered source candidates.

    Returns:
        ``True`` when the source set applies to a rendering element.

    """
    if tag == "link" and not _link_can_render(attributes):
        return False
    return tag in SRCSET_ATTRIBUTE_ELEMENTS.get(name, ())


def _qualified_name(value: str) -> tuple[str, str] | None:
    """Return an exact two-component qualified name.

    Returns:
        The nonempty prefix and local name, or ``None`` for malformed syntax.

    """
    components = value.split(":")
    if len(components) != QUALIFIED_NAME_COMPONENTS or any(
        not component for component in components
    ):
        return None
    return components[0], components[1]


class _ReferenceValueParser(HTMLParser):
    """Collect URI, source-set, CSS, and base values from actual HTML syntax."""

    def __init__(
        self,
        *,
        conditional_ancestors: tuple[str, ...] = (),
        namespace_bindings: dict[str, str] | None = None,
    ) -> None:
        """Initialize empty value collections with HTML entity decoding enabled."""
        super().__init__()
        self.uris: list[str] = []
        self.srcsets: list[str] = []
        self.css_attributes: list[str] = []
        self.style_elements: list[str] = []
        self.base_href: str | None = None
        self._style_state: set[object] = set()
        self._template_depth = 0
        self._conditional_ancestors = conditional_ancestors
        self._namespace_bindings = dict(namespace_bindings or {})
        self._namespace_frames: list[NamespaceFrame] = []

    def handle_starttag(
        self,
        tag: str,
        attrs: list[tuple[str, str | None]],
    ) -> None:
        """Collect relevant values from one actual start tag."""
        folded_tag = self._handle_starttag(tag, attrs)
        if folded_tag in HTML_VOID_ELEMENTS:
            self._leave_namespace_scope(folded_tag)

    def _handle_starttag(
        self,
        tag: str,
        attrs: list[tuple[str, str | None]],
    ) -> str:
        """Collect a start tag within its effective namespace scope.

        Returns:
            The case-folded tag name whose namespace frame was entered.

        """
        folded_tag = tag.casefold()
        first_attributes: dict[str, str | None] = {}
        for name, value in attrs:
            first_attributes.setdefault(name.casefold(), value)
        attributes = {
            name: value if value is not None else EMPTY_ATTRIBUTE_VALUE
            for name, value in first_attributes.items()
        }
        self._enter_namespace_scope(folded_tag, attributes)
        if folded_tag == "template":
            self._template_depth += 1
        elif not self._template_depth and folded_tag == "style":
            self._style_state.add(STYLE_INSIDE)
        if not self._template_depth:
            for name, value in first_attributes.items():
                self._collect_attribute(
                    folded_tag,
                    name,
                    value,
                    attributes,
                )
        return folded_tag

    def _enter_namespace_scope(self, tag: str, attributes: dict[str, str]) -> None:
        """Apply namespace declarations for one element and remember prior values."""
        previous: dict[str, str | None] = {}
        for name, value in attributes.items():
            qualified = _qualified_name(name)
            if qualified is None or qualified[0] != "xmlns":
                continue
            prefix = qualified[1]
            previous[prefix] = self._namespace_bindings.get(prefix)
            self._namespace_bindings[prefix] = value
        self._namespace_frames.append((tag, previous))

    def _leave_namespace_scope(self, tag: str) -> None:
        """Restore namespace declarations when a lexical element scope closes."""
        matching = next(
            (
                index
                for index, (frame_tag, _previous) in reversed(
                    tuple(enumerate(self._namespace_frames)),
                )
                if frame_tag == tag
            ),
            None,
        )
        if matching is None:
            return
        for _tag, previous in reversed(self._namespace_frames[matching:]):
            for prefix, value in previous.items():
                if value is None:
                    del self._namespace_bindings[prefix]
                else:
                    self._namespace_bindings[prefix] = value
        del self._namespace_frames[matching:]

    def _is_vml_source_element(self, tag: str) -> bool:
        """Return whether an element has a bound VML name that accepts ``src``.

        Returns:
            ``True`` for a rendering VML element in an exact namespace binding.

        """
        qualified = _qualified_name(tag)
        if qualified is None:
            return False
        prefix, local_name = qualified
        binding = self._namespace_bindings.get(prefix)
        return (
            local_name in VML_SOURCE_ELEMENTS
            and binding is not None
            and binding.casefold() == VML_NAMESPACE
        )

    def _collect_attribute(
        self,
        tag: str,
        name: str,
        value: str | None,
        attributes: dict[str, str],
    ) -> None:
        """Collect one populated resource-bearing attribute value."""
        if value is None:
            return
        if tag == "base" and name == "href":
            if self.base_href is None:
                self.base_href = value
            return
        if (name == "src" and self._is_vml_source_element(tag)) or _is_uri_attribute(
            tag,
            name,
            attributes,
        ):
            self.uris.append(value)
        elif _is_srcset_attribute(tag, name, attributes):
            self.srcsets.append(value)
        elif name == "style":
            self.css_attributes.append(value)

    def handle_startendtag(
        self,
        tag: str,
        attrs: list[tuple[str, str | None]],
    ) -> None:
        """Collect relevant values from one self-closing start tag."""
        folded_tag = self._handle_starttag(tag, attrs)
        self._leave_namespace_scope(folded_tag)

    def handle_endtag(self, tag: str) -> None:
        """Stop CSS collection at the end of an actual style element."""
        folded_tag = tag.casefold()
        if folded_tag == "template" and self._template_depth:
            self._template_depth -= 1
        elif folded_tag == "style" and not self._template_depth:
            self._style_state.discard(STYLE_INSIDE)
        self._leave_namespace_scope(folded_tag)

    def handle_data(self, data: str) -> None:
        """Collect raw CSS text only while inside an actual style element."""
        if STYLE_INSIDE in self._style_state and not self._template_depth:
            self.style_elements.append(data)

    def handle_comment(self, data: str) -> None:
        """Parse one bounded, positive, downlevel-hidden MSO conditional fragment."""
        if (
            self._template_depth
            or STYLE_INSIDE in self._style_state
            or len(self._conditional_ancestors) >= MAX_MSO_CONDITIONAL_DEPTH
            or len(data) > MAX_MSO_CONDITIONAL_COMMENT_LENGTH
        ):
            return
        match = MSO_HIDDEN_CONDITIONAL_PATTERN.fullmatch(data)
        if match is None:
            return
        nested = _ReferenceValueParser(
            conditional_ancestors=(*self._conditional_ancestors, data),
            namespace_bindings=self._namespace_bindings,
        )
        nested.feed(match.group(1))
        nested.close()
        self.uris.extend(nested.uris)
        self.srcsets.extend(nested.srcsets)
        self.css_attributes.extend(nested.css_attributes)
        self.style_elements.extend(nested.style_elements)
        if self.base_href is None:
            self.base_href = nested.base_href


def _reference_values(source: str) -> _ReferenceValueParser:
    """Parse one HTML source into actual URI-bearing value collections.

    Returns:
        The populated tolerant standard-library parser.

    """
    parser = _ReferenceValueParser()
    parser.feed(source)
    parser.close()
    return parser
