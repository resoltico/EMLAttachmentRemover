"""Property-test HTML resource extraction across independent representations."""

from __future__ import annotations

import hashlib
import string
import tempfile
from collections import Counter
from dataclasses import dataclass
from email import policy
from email.message import EmailMessage
from operator import add
from pathlib import Path
from typing import Final

from hypothesis import event, example, given, target
from hypothesis import strategies as st

from eml_attachment_remover import mime_locations, mime_references, process_file
from eml_attachment_remover.models import KeepReason
from tests.test_support import decoded_hash, leaf_rows, parse

LOCATION_CHARACTERS: Final = string.ascii_letters + string.digits + "./?&=_-:#žā漢字"
UNRESERVED: Final = frozenset(string.ascii_letters + string.digits + "-._~")
LOCATION_BASE: Final[st.SearchStrategy[str]] = st.builds(
    add,
    st.sampled_from(string.ascii_letters + string.digits),
    st.text(alphabet=LOCATION_CHARACTERS, max_size=48),
)
PAYLOAD: Final[st.SearchStrategy[bytes]] = st.integers(
    min_value=0,
    max_value=1_024,
).flatmap(lambda size: st.binary(min_size=size, max_size=size))


@dataclass(frozen=True, slots=True)
class ReferenceCase:
    """Describe one semantic location and its independent HTML representation."""

    base: str
    kind: str
    quote_form: str
    encoding: str
    uppercase: bool
    whitespace: str

    @property
    def location(self) -> str:
        """The semantic referenced Content-Location."""
        return f"public/{self.base}-target-image.png?variant=one&theme=light#hero"

    @property
    def near_location(self) -> str:
        """A guaranteed normalized-distinct near match."""
        return f"public/{self.base}-targetx-image.png?variant=one&theme=light#hero"


REFERENCE_CASES: Final[st.SearchStrategy[ReferenceCase]] = st.builds(
    ReferenceCase,
    base=LOCATION_BASE,
    kind=st.sampled_from((
        "background",
        "css-escaped",
        "css-import",
        "css-inline",
        "css-style",
        "data",
        "href",
        "imagesrcset",
        "poster",
        "src",
        "srcset",
    )),
    quote_form=st.sampled_from(("double", "single", "unquoted")),
    encoding=st.sampled_from(("direct", "percent", "percent-all")),
    uppercase=st.booleans(),
    whitespace=st.sampled_from(("", " ", "\t")),
)


def _resource(location: str, payload: bytes, filename: str) -> EmailMessage:
    """Create a file whose only retention metadata is Content-Location.

    Returns:
        The configured MIME entity.

    """
    part = EmailMessage()
    part.set_content(payload, maintype="image", subtype="png", cte="base64")
    part.add_header("Content-Disposition", "attachment", filename=filename)
    part["Content-Location"] = location
    return part


def _cid_resource(identifier: str, filename: str) -> EmailMessage:
    """Create an attachment whose only retention metadata is Content-ID.

    Returns:
        The configured MIME entity.

    """
    part = EmailMessage()
    part.set_content(b"public CID payload", maintype="image", subtype="png")
    part.add_header("Content-Disposition", "attachment", filename=filename)
    part["Content-ID"] = identifier
    return part


def _represented_location(case: ReferenceCase) -> tuple[str, str]:
    """Return encoded HTML text and the effective representation category.

    Returns:
        Encoded location text and the encoding category actually used.

    """
    encoding = case.encoding
    location = case.location
    encoded = False
    represented: list[str] = []
    for character in location:
        should_encode = character in UNRESERVED and (
            encoding == "percent-all" or (encoding == "percent" and not encoded)
        )
        if should_encode:
            represented.append(f"%{ord(character):02X}")
            encoded = True
        else:
            represented.append(character)
    return "".join(represented), encoding


def _quoted_value(value: str, quote_form: str) -> str:
    """Apply one generated HTML or CSS quoting representation.

    Returns:
        The represented attribute or function value.

    """
    if quote_form == "double":
        return f'"{value}"'
    if quote_form == "single":
        return f"'{value}'"
    return value


def _srcset_reference(case: ReferenceCase, value: str) -> str:
    """Build one srcset or imagesrcset element.

    Returns:
        HTML containing a data-URL decoy and the represented resource candidate.

    """
    attribute_quote = "'" if case.quote_form == "single" else '"'
    tag = 'link rel="preload"' if case.kind == "imagesrcset" else "img"
    attribute = case.kind.upper() if case.uppercase else case.kind
    srcset = f"data:image/png;base64,AAAA 1x, {value} 2x"
    return f"<{tag} {attribute}={attribute_quote}{srcset}{attribute_quote}>"


def _css_reference(
    case: ReferenceCase,
    value: str,
    represented: str,
    whitespace: str,
) -> str:
    """Build one CSS import, escaped URL, style block, or style attribute.

    Returns:
        HTML containing the requested CSS representation.

    """
    if case.kind == "css-import":
        css_quote = "'" if case.quote_form == "single" else '"'
        separator = whitespace or " "
        return f"<style>@import{separator}{css_quote}{value}{css_quote};</style>"
    if case.kind == "css-escaped":
        escaped_value = f"\\{ord(value[0]):x} {value[1:]}"
        return f"<style>.public{{background:url({escaped_value})}}</style>"
    function = "URL" if case.uppercase else "url"
    css = (
        f"background-image:{whitespace}{function}"
        f"({whitespace}{represented}{whitespace})"
    )
    if case.kind == "css-style":
        raw_css = css.replace("&amp;", "&").replace("&quot;", '"')
        return f"<style>.public{{{raw_css}}}</style>"
    outer_quote = "'" if case.quote_form == "double" else '"'
    return f"<div style={outer_quote}{css}{outer_quote}></div>"


def _html_reference(case: ReferenceCase) -> tuple[str, str]:
    """Build HTML whose representation independently denotes the location.

    Returns:
        The generated body and the effective encoding category.

    """
    value, effective_encoding = _represented_location(case)
    html_value = value.replace("&", "&amp;").replace('"', "&quot;")
    represented = _quoted_value(html_value, case.quote_form)
    whitespace = case.whitespace
    if case.kind.startswith("css-"):
        return (
            _css_reference(case, value, represented, whitespace),
            effective_encoding,
        )
    if case.kind in {"imagesrcset", "srcset"}:
        return _srcset_reference(case, html_value), effective_encoding
    attribute = case.kind.upper() if case.uppercase else case.kind
    tag = {
        "background": "body",
        "data": "object",
        "href": 'link rel="stylesheet"',
        "poster": "video",
    }.get(case.kind, "img")
    return (
        f"<{tag} {attribute}{whitespace}={whitespace}{represented}>",
        effective_encoding,
    )


def test_header_and_html_reference_boundaries_decode_entities_once() -> None:
    """Keep literal entity-like text identical across MIME and HTML syntax."""
    normalize_header_cid = (
        mime_references._normalize_content_id_header  # ruff: ignore[private-member-access]
    )
    normalize_header_location = (
        mime_locations._normalize_content_location_header  # ruff: ignore[private-member-access]
    )
    assert normalize_header_cid("<a&#0>") == "a&#0"
    assert normalize_header_location("a&#0") == "a&#0"
    assert normalize_header_location("%26amp%3B") == "%26amp%3B"
    html_references = mime_references._references_from_html(  # ruff: ignore[private-member-access]
        '<img src="cid:&lt;a&amp;#0&gt;"><img src="a&amp;amp;"><img src="%26amp%3B">'
    )
    assert html_references.content_ids == frozenset({"a&#0"})
    assert html_references.locations == frozenset({"%26amp%3B", "a&amp;"})
    srcset = mime_references._references_from_html(  # ruff: ignore[private-member-access]
        '<img srcset="a&amp;amp; 1x, ax&amp;amp; 2x">'
    )
    assert srcset.locations == frozenset({"a&amp;", "ax&amp;"})


def test_entity_like_cid_matches_across_header_and_html_boundaries() -> None:
    """Retain a CID whose literal text resembles a numeric HTML entity."""
    message = EmailMessage()
    message["Subject"] = "Public entity-like CID fixture"
    message.set_content(
        '<html><img src="cid:a&amp;#0@example.test"></html>',
        subtype="html",
    )
    message.make_mixed()
    message.attach(_cid_resource("<a&#0@example.test>", "referenced.png"))
    message.attach(_cid_resource("<a&#0x@example.test>", "near-match.png"))
    raw = message.as_bytes(policy=policy.SMTP)

    with tempfile.TemporaryDirectory() as directory:
        source = Path(directory) / "source.eml"
        destination = Path(directory) / "output.eml"
        source.write_bytes(raw)

        result = process_file(source, destination, force=False, dry_run=False)

        assert source.read_bytes() == raw
        assert [part.filename for part in result.removed] == ["near-match.png"]
        assert [part.reason for part in result.preserved_file_parts] == [
            KeepReason.BODY_CID_REFERENCE
        ]


@example(
    case=ReferenceCase(
        base="a&#0",
        kind="background",
        quote_form="double",
        encoding="direct",
        uppercase=False,
        whitespace="",
    ),
    payload=b"",
)
@example(
    case=ReferenceCase(
        base="images/public-logo",
        kind="css-style",
        quote_form="single",
        encoding="percent",
        uppercase=True,
        whitespace=" ",
    ),
    payload=b"\x00\xff",
)
@example(
    case=ReferenceCase(
        base="public-srcset",
        kind="srcset",
        quote_form="double",
        encoding="percent",
        uppercase=False,
        whitespace=" ",
    ),
    payload=b"public srcset payload",
)
@example(
    case=ReferenceCase(
        base="a&#0",
        kind="srcset",
        quote_form="double",
        encoding="direct",
        uppercase=False,
        whitespace=" ",
    ),
    payload=b"public entity-like srcset payload",
)
@example(
    case=ReferenceCase(
        base="public-imagesrcset",
        kind="imagesrcset",
        quote_form="single",
        encoding="percent-all",
        uppercase=True,
        whitespace="\t",
    ),
    payload=b"public imagesrcset payload",
)
@example(
    case=ReferenceCase(
        base="public-import",
        kind="css-import",
        quote_form="double",
        encoding="direct",
        uppercase=False,
        whitespace=" ",
    ),
    payload=b"public CSS import payload",
)
@example(
    case=ReferenceCase(
        base="public-css-escape",
        kind="css-escaped",
        quote_form="unquoted",
        encoding="direct",
        uppercase=False,
        whitespace="",
    ),
    payload=b"public CSS escape payload",
)
@given(case=REFERENCE_CASES, payload=PAYLOAD)
def test_representations_retain_only_the_exact_location(
    case: ReferenceCase,
    payload: bytes,
) -> None:
    """Check exact Content-Location matching across HTML and CSS syntax."""
    body, effective_encoding = _html_reference(case)
    message = EmailMessage()
    message["Subject"] = "Generated public location-reference fixture"
    message.set_content(body, subtype="html")
    message.make_mixed()
    message.attach(_resource(case.location, payload, "referenced.png"))
    message.attach(
        _resource(case.near_location, b"public near match", "near-match.png")
    )
    raw = message.as_bytes(policy=policy.SMTP)
    event(f"location-kind={case.kind}")
    event(f"location-quote={case.quote_form}")
    event(f"location-encoding={effective_encoding}")
    event(f"location-uppercase={case.uppercase}")
    target(len(case.location), label="Content-Location length")
    with tempfile.TemporaryDirectory() as directory:
        source = Path(directory) / "source.eml"
        destination = Path(directory) / "output.eml"
        source.write_bytes(raw)
        expected_rows = Counter(
            row for row in leaf_rows(parse(source)) if row[2] != "near-match.png"
        )
        result = process_file(source, destination, force=False, dry_run=False)
        parsed_output = parse(destination)
        retained = next(
            part
            for part in parsed_output.walk()
            if part.get_filename() == "referenced.png"
        )
        assert source.read_bytes() == raw
        assert [part.filename for part in result.removed] == ["near-match.png"]
        assert [part.reason for part in result.preserved_file_parts] == [
            KeepReason.BODY_LOCATION_REFERENCE
        ]
        assert Counter(leaf_rows(parsed_output)) == expected_rows
        assert retained["Content-Location"] == case.location
        assert decoded_hash(retained) == hashlib.sha256(payload).hexdigest()


def test_base_resolution_and_fragments_retain_only_the_rendered_resource() -> None:
    """Resolve HTML and MIME bases and ignore fetch-irrelevant URI fragments."""
    message = EmailMessage()
    message["Subject"] = "Public base-resolution fixture"
    message.set_content(
        '<base href="../assets/"><img src="hero.png#public-view">',
        subtype="html",
    )
    message["Content-Location"] = "https://public.example/mail/body/index.html"
    message.make_mixed()
    message.attach(
        _resource(
            "https://public.example/mail/assets/hero.png",
            b"public resolved payload",
            "referenced.png",
        )
    )
    message.attach(
        _resource(
            "https://public.example/mail/assets/herox.png",
            b"public near match",
            "near-match.png",
        )
    )
    raw = message.as_bytes(policy=policy.SMTP)
    with tempfile.TemporaryDirectory() as directory:
        source = Path(directory) / "source.eml"
        destination = Path(directory) / "output.eml"
        source.write_bytes(raw)

        result = process_file(source, destination, force=False, dry_run=False)

        assert source.read_bytes() == raw
        assert [part.filename for part in result.removed] == ["near-match.png"]
        assert [part.filename for part in result.preserved_file_parts] == [
            "referenced.png"
        ]
