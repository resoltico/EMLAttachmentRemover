"""Sanitize and atomically publish one pytest JUnit XML report."""

from __future__ import annotations

import base64
import binascii
import importlib
import os
import socket
import stat
import sys
import tempfile
from contextlib import suppress
from pathlib import Path
from typing import TYPE_CHECKING, Final

from defusedxml import ElementTree

if TYPE_CHECKING:
    from collections.abc import Sequence
    from xml.etree.ElementTree import Element

    from tools import hypothesis_observation_safety as path_safety
else:
    path_safety = importlib.import_module(
        "tools.hypothesis_observation_safety"
        if __package__
        else "hypothesis_observation_safety"
    )

REPORT_NAME: Final = "test-results.xml"
UTF8: Final = "utf-8"
ISOLATED_PLACEHOLDER: Final = "ISOLATED_TEST_ROOT"
PROJECT_PLACEHOLDER: Final = "PROJECT_ROOT"
HOME_PLACEHOLDER: Final = "USER_HOME"
PYTHON_PLACEHOLDER: Final = "PYTHON_PREFIX"
PYTHON_BASE_PLACEHOLDER: Final = "PYTHON_BASE_PREFIX"
TEMP_PLACEHOLDER: Final = "SYSTEM_TEMP"
HOST_PLACEHOLDER: Final = "MACHINE_HOST"
TEMPORARY_SUFFIX: Final = ".tmp"
HYPOTHESIS_STATISTICS_PREFIX: Final = "hypothesis-statistics-"
HYPOTHESIS_STATISTICS_NAMESPACE: Final = "hypothesis-statistics"
HYPOTHESIS_PROPERTY_ATTRIBUTES: Final = frozenset({"name", "value"})
policy = importlib.import_module(
    "tools.repository_hygiene_policy" if __package__ else "repository_hygiene_policy"
)
xunit_schema = importlib.import_module(
    f"{'tools.' if __package__ else ''}junit_xunit2_schema"
)
node_id = importlib.import_module(
    "tools.junit_node_id" if __package__ else "junit_node_id"
)


class JunitReportError(ValueError):
    """Report an unsafe, private, malformed, or unpublished JUnit report."""


def temporary_report_path(storage: Path) -> Path:
    """Return the report path beside, but outside, Hypothesis storage.

    Returns:
        A path within the already-authenticated isolated temporary root.

    """
    return storage.parent / REPORT_NAME


def _regular_source(path: Path) -> bytes:
    """Read a regular non-symbolic report source.

    Returns:
        The exact report bytes.

    Raises:
        JunitReportError: If the source is absent, unsafe, or unreadable.

    """
    try:
        mode = path.lstat().st_mode
    except OSError as error:
        message = f"cannot inspect JUnit report {path}: {error}"
        raise JunitReportError(message) from error
    if not stat.S_ISREG(mode):
        message = f"JUnit report must be a regular non-symbolic file: {path}"
        raise JunitReportError(message)
    try:
        return path.read_bytes()
    except OSError as error:
        message = f"cannot read JUnit report {path}: {error}"
        raise JunitReportError(message) from error


def _replacements(
    source: Path,
    project_root: Path,
) -> tuple[tuple[str, str], ...]:
    """Return longest-first private prefixes and XML-safe placeholders.

    Returns:
        Every known machine-specific prefix in deterministic order.

    """
    path_candidates = (
        (source.parent, ISOLATED_PLACEHOLDER),
        (project_root, PROJECT_PLACEHOLDER),
        (Path.home(), HOME_PLACEHOLDER),
        (Path(sys.prefix), PYTHON_PLACEHOLDER),
        (Path(sys.base_prefix), PYTHON_BASE_PLACEHOLDER),
        (Path(tempfile.gettempdir()), TEMP_PLACEHOLDER),
    )
    replacements: dict[str, str] = {}
    prefixes = (
        (prefix, replacement)
        for path, replacement in path_candidates
        for resolved in [str(path.resolve())]
        if resolved != os.sep
        for prefix in {resolved, *([str(path)] if path.is_absolute() else [])}
    )
    variants = (
        (variant, replacement)
        for prefix, replacement in prefixes
        for variant in path_safety.path_prefix_variants(prefix)
    )
    for variant, replacement in variants:
        replacements.setdefault(variant, replacement)
    hostname = socket.gethostname()
    if hostname:
        replacements.setdefault(hostname, HOST_PLACEHOLDER)
    return tuple(sorted(replacements.items(), key=lambda item: (-len(item[0]), item)))


def _public_text(
    value: str,
    replacements: Sequence[tuple[str, str]],
    source: Path,
    label: str,
    *,
    pytest_node_id: bool = False,
) -> str:
    """Replace known prefixes and reject private or absolute retained text.

    Returns:
        The validated public text.

    Raises:
        JunitReportError: If the value retains private or absolute content.

    """
    replacement_tuple = tuple(replacements)
    public = path_safety.public_text(value, replacement_tuple)
    if path_safety.private_prefix_remains(public, replacement_tuple):
        message = f"private path remains in JUnit report {source}"
        raise JunitReportError(message)
    privacy_issues = policy.public_content_messages(public)
    if privacy_issues:
        message = f"JUnit report contains private content: {'; '.join(privacy_issues)}"
        raise JunitReportError(message)
    if pytest_node_id:
        public = node_id.redact(public)
    path_value = public.replace("\\\\", "\\") if pytest_node_id else public
    if path_safety.contains_absolute_path(path_value):
        message = f"machine-specific absolute path remains in {label} from {source}"
        raise JunitReportError(message)
    return public


def _validate_xml_name(
    name: str,
    replacements: Sequence[tuple[str, str]],
    source: Path,
    label: str,
) -> None:
    if _public_text(name, replacements, source, label) != name:
        message = f"private prefix appears in {label} from {source}"
        raise JunitReportError(message)


def _statistics_properties(root: Element, source: Path) -> tuple[Element, ...]:
    """Return strictly shaped Hypothesis statistics properties.

    Returns:
        Every unique expected Hypothesis statistics property.

    Raises:
        JunitReportError: If a prefixed element has an unexpected XML schema.

    """
    parents = {child: parent for parent in root.iter() for child in parent}
    statistics: list[Element] = []
    names: set[str] = set()
    for element in root.iter():
        name = element.attrib.get("name")
        if name is None or not name.lstrip().casefold().startswith(
            HYPOTHESIS_STATISTICS_NAMESPACE
        ):
            continue
        parent = parents.get(element)
        grandparent = parents.get(parent) if parent is not None else None
        expected = (
            root.tag == "testsuites"
            and element.tag == "property"
            and name.startswith(HYPOTHESIS_STATISTICS_PREFIX)
            and parent is not None
            and parent.tag == "properties"
            and grandparent is not None
            and grandparent.tag == "testsuite"
            and parents.get(grandparent) is root
            and frozenset(element.attrib) == HYPOTHESIS_PROPERTY_ATTRIBUTES
            and len(element) == 0
            and element.text is None
            and len(name) > len(HYPOTHESIS_STATISTICS_PREFIX)
            and bool(element.attrib["value"])
            and name not in names
        )
        if not expected:
            message = f"invalid Hypothesis statistics property schema in {source}"
            raise JunitReportError(message)
        names.add(name)
        statistics.append(element)
    return tuple(statistics)


def _public_statistics(
    encoded: str,
    node_id: str,
    replacements: Sequence[tuple[str, str]],
    source: Path,
) -> tuple[str, str]:
    """Decode, sanitize, and canonically re-encode Hypothesis statistics.

    Returns:
        The public node ID and canonical base64 for its validated statistics.

    Raises:
        JunitReportError: If the value is malformed or unsafe.

    """
    try:
        decoded = base64.b64decode(encoded, validate=True)
    except (binascii.Error, ValueError) as error:
        message = f"invalid Hypothesis statistics base64 in {source}"
        raise JunitReportError(message) from error
    if base64.b64encode(decoded).decode() != encoded:
        message = f"noncanonical Hypothesis statistics base64 in {source}"
        raise JunitReportError(message)
    try:
        statistics = decoded.decode(UTF8)
    except UnicodeError as error:
        message = f"Hypothesis statistics are not valid UTF-8 in {source}"
        raise JunitReportError(message) from error
    expected_header = f"{node_id}:\n"
    if not statistics.startswith(expected_header):
        message = f"Hypothesis statistics node ID does not match property in {source}"
        raise JunitReportError(message)
    public_node_id = _public_text(
        node_id,
        replacements,
        source,
        "Hypothesis statistics property name",
        pytest_node_id=True,
    )
    public_body = _public_text(
        statistics[len(expected_header) :],
        replacements,
        source,
        "encoded Hypothesis statistics",
    )
    public = f"{public_node_id}:\n{public_body}"
    return public_node_id, base64.b64encode(public.encode(UTF8)).decode()


def _sanitize_tree(
    root: Element,
    replacements: Sequence[tuple[str, str]],
    source: Path,
) -> None:
    """Sanitize every retained XML value, including encoded statistics.

    Raises:
        JunitReportError: If sanitized statistics node IDs collide.

    """
    statistics = frozenset(_statistics_properties(root, source))
    public_statistics_names: set[str] = set()
    for element in statistics:
        node_id = element.attrib["name"][len(HYPOTHESIS_STATISTICS_PREFIX) :]
        public_node_id, public_statistics = _public_statistics(
            element.attrib["value"], node_id, replacements, source
        )
        public_name = f"{HYPOTHESIS_STATISTICS_PREFIX}{public_node_id}"
        if public_name in public_statistics_names:
            message = f"duplicate sanitized Hypothesis statistics node ID in {source}"
            raise JunitReportError(message)
        public_statistics_names.add(public_name)
        element.set("name", public_name)
        element.set("value", public_statistics)
    for element in root.iter():
        _validate_xml_name(
            str(element.tag), replacements, source, "JUnit XML element name"
        )
        if element.text is not None:
            element.text = _public_text(
                element.text,
                replacements,
                source,
                "JUnit XML text",
            )
        if element.tail is not None:
            element.tail = _public_text(
                element.tail,
                replacements,
                source,
                "JUnit XML text",
            )
        for name, value in tuple(element.attrib.items()):
            _validate_xml_name(name, replacements, source, "JUnit XML attribute name")
            if element not in statistics or name != "value":
                element.set(
                    name,
                    _public_text(
                        value,
                        replacements,
                        source,
                        "JUnit XML attribute value",
                        pytest_node_id=name == "name"
                        and (element.tag == "testcase" or element in statistics),
                    ),
                )


def _validate_replacements(
    replacements: Sequence[tuple[str, str]], source: Path
) -> None:
    """Reject replacement tokens that cannot be inserted as plain XML values.

    Raises:
        JunitReportError: If a replacement contains XML syntax characters.

    """
    if any(
        character in replacement
        for _prefix, replacement in replacements
        for character in "<>&\"'"
    ):
        message = f"sanitized JUnit report is invalid XML: {source}: unsafe replacement"
        raise JunitReportError(message)


def _sanitize(
    content: bytes,
    replacements: Sequence[tuple[str, str]],
    source: Path,
) -> bytes:
    """Return valid public UTF-8 XML with known private prefixes replaced.

    Returns:
        Sanitized XML bytes.

    Raises:
        JunitReportError: If the report is malformed or retains private content.

    """
    try:
        text = content.decode(UTF8)
        root = ElementTree.fromstring(text)
    except (UnicodeError, ElementTree.ParseError) as error:
        message = f"JUnit report is not valid UTF-8 XML: {source}: {error}"
        raise JunitReportError(message) from error
    _validate_replacements(replacements, source)
    _sanitize_tree(root, replacements, source)
    schema_issue = xunit_schema.validation_issue(root)
    if schema_issue is not None:
        message = f"invalid pytest xUnit2 structure in {source}: {schema_issue}"
        raise JunitReportError(message)
    sanitized = bytes(
        ElementTree.tostring(
            root,
            encoding="utf-8",
            xml_declaration=True,
        )
    )
    try:
        ElementTree.fromstring(sanitized)
    except ElementTree.ParseError as error:
        message = f"sanitized JUnit report is invalid XML: {source}: {error}"
        raise JunitReportError(message) from error
    return sanitized


def _validate_destination(destination: Path) -> None:
    """Require a real parent and a safe absent-or-regular destination.

    Raises:
        JunitReportError: If the destination or its parent is unsafe.

    """
    try:
        parent_mode = destination.parent.lstat().st_mode
    except OSError as error:
        message = f"cannot inspect JUnit report directory {destination.parent}: {error}"
        raise JunitReportError(message) from error
    if not stat.S_ISDIR(parent_mode):
        message = f"JUnit report directory must be real: {destination.parent}"
        raise JunitReportError(message)
    try:
        destination_mode = destination.lstat().st_mode
    except FileNotFoundError:
        return
    except OSError as error:
        message = f"cannot inspect JUnit report destination {destination}: {error}"
        raise JunitReportError(message) from error
    if not stat.S_ISREG(destination_mode):
        message = f"JUnit report destination must be regular: {destination}"
        raise JunitReportError(message)


def publish(source: Path, destination: Path, project_root: Path) -> Path:
    """Sanitize and atomically publish one JUnit report.

    Returns:
        The published report path.

    Raises:
        JunitReportError: If sanitization or atomic publication fails.

    """
    content = _sanitize(
        _regular_source(source), _replacements(source, project_root), source
    )
    try:
        destination.parent.mkdir(exist_ok=True)
    except OSError as error:
        message = f"cannot prepare JUnit report directory {destination.parent}: {error}"
        raise JunitReportError(message) from error
    _validate_destination(destination)
    try:
        descriptor, temporary_name = tempfile.mkstemp(
            dir=destination.parent,
            prefix=f".{destination.name}.",
            suffix=TEMPORARY_SUFFIX,
        )
        temporary = Path(temporary_name)
    except OSError as error:
        message = f"cannot reserve JUnit report publication: {error}"
        raise JunitReportError(message) from error
    try:
        with os.fdopen(descriptor, "wb") as output:
            output.write(content)
            output.flush()
            os.fsync(output.fileno())
        temporary.replace(destination)
    except OSError as error:
        message = f"cannot publish JUnit report {destination}: {error}"
        raise JunitReportError(message) from error
    finally:
        with suppress(OSError):
            temporary.unlink()
    return destination
