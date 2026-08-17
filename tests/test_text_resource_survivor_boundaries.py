# ruff: file-ignore[private-member-access]
"""Mutation-complete boundaries for discarded body-resource auditing."""

from __future__ import annotations

from email.message import EmailMessage

from eml_attachment_remover import mime_text_resources
from eml_attachment_remover.models import ReferenceIndex

BASE = "https://public.example.test/base/"
IMAGE = f"{BASE}image.png"


def _html(uri: str = "image.png") -> EmailMessage:
    """Return one HTML leaf containing a rendering reference.

    Returns:
        The configured HTML entity.

    """
    part = EmailMessage()
    part.set_content(f'<img src="{uri}">', subtype="html")
    return part


def _binary(*, location: str | None = None) -> EmailMessage:
    """Return one binary resource with optional location identity.

    Returns:
        The configured resource.

    """
    part = EmailMessage()
    part.set_content(b"PUBLIC RESOURCE", maintype="image", subtype="png")
    if location is not None:
        part["Content-Location"] = location
    return part


def _container(kind: str, *children: EmailMessage) -> EmailMessage:
    """Return one populated multipart aggregate.

    Returns:
        The configured aggregate.

    """
    part = EmailMessage()
    getattr(part, f"make_{kind}")()
    for child in children:
        part.attach(child)
    return part


def _location_sources(
    path: tuple[int, ...] = (7,),
) -> tuple[tuple[tuple[int, ...], ReferenceIndex], ...]:
    """Return one absolute location-reference source.

    Returns:
        A single path-bound reference index.

    """
    return ((path, ReferenceIndex(frozenset(), frozenset({IMAGE}))),)


def _scan(
    *,
    inherited_base: str | None = BASE,
    include_text: bool = True,
    root_inclusion: mime_text_resources.RootInclusion = (
        mime_text_resources.RootInclusion.INCLUDE
    ),
) -> mime_text_resources.ResourceScan:
    """Return a resource scan with one absolute location edge.

    Returns:
        The configured scan.

    """
    return mime_text_resources.ResourceScan(
        reference_sources=_location_sources(),
        inherited_base=inherited_base,
        include_text=include_text,
        root_inclusion=root_inclusion,
    )


def test_body_resource_requires_a_matching_identity_and_keeps_disposition() -> None:
    """Reject nonmatching CID/location unions and retain public metadata."""
    resource = _binary(location="different.png")
    resource["Content-ID"] = "<different@example.test>"
    resource.add_header("Content-Disposition", "inline", filename="public.png")
    sources = (
        (
            (3,),
            ReferenceIndex(
                frozenset({"expected@example.test"}),
                frozenset({IMAGE}),
            ),
        ),
    )

    record = mime_text_resources._body_resource(resource, (9,), BASE, sources)

    assert record.referenced_by == ()
    assert record.disposition == "inline"
    assert record.filename == "public.png"


def test_body_resource_resolves_relative_location_against_inherited_base() -> None:
    """Audit the actual absolute location represented by a relative label."""
    record = mime_text_resources._body_resource(
        _binary(location="image.png"), (2,), BASE, _location_sources()
    )

    assert record.referenced_by == ((7,),)


def test_related_root_selection_honors_start_and_defaults_to_first_child() -> None:
    """Scan only the uniquely resolved root of a related aggregate."""
    first = _html("first.png")
    first["Content-ID"] = "<first@example.test>"
    second = _html("second.png")
    second["Content-ID"] = "<second@example.test>"
    related = _container("related", first, second)

    default_sources = mime_text_resources.html_reference_sources(related, (4,), BASE)
    related.set_param("start", "<second@example.test>")
    selected_sources = mime_text_resources.html_reference_sources(related, (4,), BASE)
    related.set_param("start", "<missing@example.test>", replace=True)

    assert [path for path, _references in default_sources] == [(4, 0)]
    assert default_sources[0][1].locations == frozenset({f"{BASE}first.png"})
    assert [path for path, _references in selected_sources] == [(4, 1)]
    assert selected_sources[0][1].locations == frozenset({f"{BASE}second.png"})
    assert mime_text_resources.html_reference_sources(related, (4,), BASE) == ()


def test_reference_scan_propagates_container_base_to_html_leaf() -> None:
    """Resolve leaf references through the nearest absolute container base."""
    message = _container("mixed", _html())
    message["Content-Location"] = BASE

    sources = mime_text_resources.html_reference_sources(message, (5,), None)

    assert [path for path, _references in sources] == [(5, 0)]
    assert sources[0][1].locations == frozenset({IMAGE})


def test_reference_scan_excludes_opaque_attached_and_nested_scopes() -> None:
    """Never invent outer reference edges from structurally excluded subtrees."""
    visible = _html("visible.png")
    attached = _html("attached.png")
    attached["Content-Disposition"] = "attachment"
    protected = EmailMessage()
    protected.set_type("multipart/signed")
    protected.set_payload([_html("protected.png")])
    protected.set_param("protocol", "application/pgp-signature")
    nested_related = _container("related", _html("nested.png"))
    wrapped = EmailMessage()
    wrapped.set_type("message/rfc822")
    wrapped.set_payload([_html("wrapped.png")])
    message = _container(
        "mixed",
        visible,
        attached,
        protected,
        nested_related,
        wrapped,
    )

    sources = mime_text_resources.html_reference_sources(message, (), BASE)

    assert [path for path, _references in sources] == [(0,)]
    assert sources[0][1].locations == frozenset({f"{BASE}visible.png"})


def test_resource_leaves_propagate_base_through_descendant_scan() -> None:
    """Keep descendant path, base, and inclusion state across recursion."""
    message = _container("mixed", _binary(location="image.png"))

    records = mime_text_resources.resource_leaves(
        message,
        (2,),
        _scan(),
    )

    assert [record.path for record in records] == [(2, 0)]
    assert records[0].referenced_by == ((7,),)


def test_resource_leaves_bind_protected_and_empty_roots_to_base() -> None:
    """Audit opaque and empty atomic roots without losing location context."""
    protected = EmailMessage()
    protected.set_type(
        next(iter(sorted(mime_text_resources.PROTECTED_MIME_TYPES))),
    )
    protected["Content-Location"] = "image.png"
    protected.set_payload("PUBLIC OPAQUE")
    empty = EmailMessage()
    empty.make_mixed()
    empty.set_payload([])
    empty["Content-Location"] = "image.png"

    protected_records = mime_text_resources.resource_leaves(protected, (1,), _scan())
    empty_records = mime_text_resources.resource_leaves(empty, (2,), _scan())

    assert protected_records[0].referenced_by == ((7,),)
    assert empty_records[0].referenced_by == ((7,),)


def test_resource_leaves_omit_text_and_explicit_root_only_when_requested() -> None:
    """Apply text filtering and root omission independently."""
    plain = EmailMessage()
    plain.set_content("PUBLIC TEXT")
    binary = _binary(location="image.png")

    assert (
        mime_text_resources.resource_leaves(
            plain,
            (1,),
            _scan(include_text=False),
        )
        == ()
    )
    assert (
        mime_text_resources.resource_leaves(
            binary,
            (2,),
            _scan(root_inclusion=mime_text_resources.RootInclusion.OMIT),
        )
        == ()
    )
