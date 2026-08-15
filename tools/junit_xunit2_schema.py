"""Validate the stable outer hierarchy of pytest 9.1.1 xUnit2 XML.

The boundary intentionally validates element kinds, placement, mixed content,
and stable container ordering only. Attribute values and failure/output prose
remain opaque because pytest settings and compatible plugins may extend them;
the publisher applies its independent privacy policy to every retained value.
"""

from __future__ import annotations

from typing import TYPE_CHECKING, Final

if TYPE_CHECKING:
    from xml.etree.ElementTree import Element

ROOT_TAG: Final = "testsuites"
STRUCTURAL_TAGS: Final = frozenset({
    ROOT_TAG,
    "testsuite",
    "properties",
    "testcase",
})
PROPERTY_CONTAINERS: Final = frozenset({"testsuite", "testcase"})
OUTCOME_TAGS: Final = frozenset({"failure", "error", "skipped"})
ALLOWED_CHILDREN: Final = {
    ROOT_TAG: frozenset({"testsuite"}),
    "testsuite": frozenset({"properties", "testcase"}),
    "properties": frozenset({"property"}),
    "testcase": frozenset({
        "properties",
        *OUTCOME_TAGS,
        "system-out",
        "system-err",
    }),
    "property": frozenset(),
    "failure": frozenset(),
    "error": frozenset(),
    "skipped": frozenset(),
    "system-out": frozenset(),
    "system-err": frozenset(),
}


def validation_issue(root: Element) -> str | None:
    """Return one stable structural issue, or ``None`` for valid hierarchy.

    Returns:
        A value-free diagnostic suitable for public error reporting.

    """
    if root.tag != ROOT_TAG:
        return "root element must be testsuites"
    if not len(root):
        return "testsuites must contain at least one testsuite"
    return _element_issue(root)


def _element_issue(element: Element) -> str | None:
    """Return a recursive child-kind or text-placement issue.

    Returns:
        The first value-free structural issue, or ``None``.

    """
    tag = str(element.tag)
    children = tuple(element)
    if tag in STRUCTURAL_TAGS and element.text is not None and element.text.strip():
        return f"{tag} cannot contain non-whitespace text"
    if tag == "properties" and not children:
        return "properties must contain at least one property"
    order_issue = _container_order_issue(tag, children)
    if order_issue is not None:
        return order_issue
    allowed = ALLOWED_CHILDREN[tag]
    for child in children:
        child_issue = _child_issue(tag, allowed, child)
        if child_issue is not None:
            return child_issue
    return None


def _child_issue(
    parent_tag: str, allowed: frozenset[str], child: Element
) -> str | None:
    """Return one child-kind, tail, or descendant issue.

    Returns:
        The first value-free structural issue, or ``None``.

    """
    child_tag = str(child.tag)
    if child_tag not in allowed:
        return f"{parent_tag} cannot contain {child_tag}"
    if child.tail is not None and child.tail.strip():
        return f"{child_tag} cannot have non-whitespace tail"
    return _element_issue(child)


def _container_order_issue(tag: str, children: tuple[Element, ...]) -> str | None:
    """Return a property-order or multiple-outcome issue.

    Returns:
        The first value-free ordering issue, or ``None``.

    """
    if tag in PROPERTY_CONTAINERS:
        property_positions = tuple(
            index for index, child in enumerate(children) if child.tag == "properties"
        )
        if len(property_positions) > 1:
            return f"{tag} cannot contain duplicate properties"
        if property_positions and property_positions[0] != 0:
            return f"{tag} properties must be first"
    if tag == "testcase" and sum(child.tag in OUTCOME_TAGS for child in children) > 1:
        return "testcase cannot contain multiple outcomes"
    return None
