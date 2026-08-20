# ruff: file-ignore[private-member-access]
"""Exact mutation contracts for v2 body planning and classification."""

from __future__ import annotations

from email.message import EmailMessage
from unittest.mock import call, patch

import pytest

from eml_attachment_remover import mime_text_only
from eml_attachment_remover.mime_text_plan import TextProjection
from eml_attachment_remover.mime_text_resources import ResourceScan, RootInclusion
from eml_attachment_remover.models import (
    CliError,
    DiscardedBodyRepresentation,
    ExitCode,
    ReferenceIndex,
    SelectedPlainTextBody,
)


def _plain() -> EmailMessage:
    """Return one safe plain body.

    Returns:
        The configured plain-text leaf.

    """
    part = EmailMessage()
    part.set_content("PUBLIC PLAIN BODY")
    return part


def _html() -> EmailMessage:
    """Return one HTML representation.

    Returns:
        The configured HTML leaf.

    """
    part = EmailMessage()
    part.set_content("<p>PUBLIC HTML BODY</p>", subtype="html")
    return part


def _container(kind: str, *children: EmailMessage) -> EmailMessage:
    """Return one populated multipart container.

    Returns:
        The configured container.

    """
    part = EmailMessage()
    getattr(part, f"make_{kind}")()
    for child in children:
        part.attach(child)
    return part


def _binary() -> EmailMessage:
    """Return one anonymous binary leaf.

    Returns:
        The configured binary part.

    """
    part = EmailMessage()
    part.set_content(
        b"PUBLIC BINARY",
        maintype="application",
        subtype="octet-stream",
    )
    return part


def _seed_plan(path: tuple[int, ...] = (9,)) -> mime_text_only.TextOnlyPlan:
    """Return one minimal immutable plan for helper-boundary tests.

    Returns:
        The synthetic selected-body plan.

    """
    return mime_text_only.TextOnlyPlan(
        projection=TextProjection(
            selected_body=SelectedPlainTextBody(path, "text/plain"),
            selected_payload_sha256="0" * 64,
        ),
    )


def test_transformation_error_binds_exact_prefix_detail_and_nested_path() -> None:
    """Keep the public transformation diagnostic stable and path-specific."""
    error = mime_text_only._transformation_unavailable((2, 4), "PUBLIC DETAIL")

    assert error.code is ExitCode.TRANSFORMATION_UNAVAILABLE
    assert error.message == (
        "cannot produce a text-only EML: PUBLIC DETAIL at MIME path 3.5"
    )


def test_each_container_reports_its_exact_nontraversable_nested_path() -> None:
    """Distinguish each malformed container without losing its source path."""
    cases = (
        (
            "alternative",
            mime_text_only._plan_alternative,
            "multipart/alternative has no traversable representations",
        ),
        (
            "related",
            mime_text_only._plan_related,
            "multipart/related has no traversable root entity",
        ),
        (
            "mixed",
            mime_text_only._plan_mixed,
            "multipart/mixed has no traversable body entity",
        ),
    )
    for kind, planner, detail in cases:
        malformed = EmailMessage()
        malformed["Content-Type"] = f"multipart/{kind}"
        malformed.set_payload("PUBLIC NON-LIST")

        with pytest.raises(CliError) as raised:
            planner(malformed, (2,), "PUBLIC BASE")

        assert raised.value.message == (
            f"cannot produce a text-only EML: {detail} at MIME path 3"
        )


def test_ambiguous_alternative_and_mixed_diagnostics_are_exact() -> None:
    """Report the precise nested container that cannot choose one body."""
    alternative = _container("alternative", _plain(), _plain())
    mixed = _container("mixed", _plain(), _plain())

    with pytest.raises(CliError) as alternative_error:
        mime_text_only._plan_alternative(alternative, (1,), None)
    with pytest.raises(CliError) as mixed_error:
        mime_text_only._plan_mixed(mixed, (1,), None)

    assert alternative_error.value.message == (
        "cannot produce a text-only EML: multipart/alternative does not contain "
        "exactly one resource-free text/plain representation at MIME path 2"
    )
    assert mixed_error.value.message == (
        "cannot produce a text-only EML: multipart/mixed does not contain one "
        "unique plain-text body at MIME path 2"
    )


def test_related_root_and_type_diagnostics_retain_the_nested_path() -> None:
    """Differentiate unresolved roots from declared-type mismatches exactly."""
    empty = EmailMessage()
    empty.make_related()
    empty.set_payload([])
    mismatch = _container("related", _plain())
    mismatch.set_param("type", "text/html")

    with pytest.raises(CliError) as root_error:
        mime_text_only._plan_related(empty, (3,), None)
    with pytest.raises(CliError) as type_error:
        mime_text_only._plan_related(mismatch, (4,), None)

    assert root_error.value.message == (
        "cannot produce a text-only EML: multipart/related does not resolve one "
        "unique root entity at MIME path 4"
    )
    assert type_error.value.message == (
        "cannot produce a text-only EML: multipart/related type does not match its "
        "resolved root entity at MIME path 5"
    )


def test_body_metadata_and_unsupported_type_diagnostics_are_exact() -> None:
    """Keep file-like, protected, and ordinary unsupported failures distinct."""
    file_like = _container("alternative", _plain(), _html())
    file_like["Content-Disposition"] = 'attachment; filename="body.mime"'
    unsupported = _html()
    protected = EmailMessage()
    protected.set_type("application/pkcs7-mime")
    protected.set_payload("PUBLIC PROTECTED")

    with pytest.raises(CliError) as file_error:
        mime_text_only._plan_body(file_like, (0, 2), None)
    with pytest.raises(CliError) as unsupported_error:
        mime_text_only._plan_body(unsupported, (1, 2), None)
    with pytest.raises(CliError) as protected_error:
        mime_text_only._plan_body(protected, (2, 2), None)

    assert file_error.value.message == (
        "cannot produce a text-only EML: multipart/alternative body container has "
        "file-like metadata at MIME path 1.3"
    )
    assert unsupported_error.value.message == (
        "cannot produce a text-only EML: MIME entity 'text/html' is not a safe "
        "plain-text body at MIME path 2.3"
    )
    assert protected_error.value.message == (
        "cannot produce a text-only EML: protected MIME entity "
        "'application/pkcs7-mime' is not a safe plain-text body at MIME path 3.3"
    )


def test_body_dispatch_forwards_the_inherited_base_to_every_container() -> None:
    """Keep alternative, related, and mixed base propagation symmetric."""
    cases = (
        ("alternative", mime_text_only._plan_alternative),
        ("related", mime_text_only._plan_related),
        ("mixed", mime_text_only._plan_mixed),
    )
    for index, (kind, planner) in enumerate(cases):
        container = _container(kind, _plain())
        seed = _seed_plan((index, 0))
        with patch.object(
            mime_text_only,
            planner.__name__,
            return_value=seed,
        ) as delegated:
            result = mime_text_only._plan_body(
                container,
                (index,),
                "PUBLIC INHERITED BASE",
            )

        assert result is seed
        delegated.assert_called_once_with(
            container,
            (index,),
            "PUBLIC INHERITED BASE",
        )


def test_alternative_forwards_base_scope_and_resource_scan_exactly() -> None:
    """Bind discarded representation metadata and inherited location scope."""
    plain = _plain()
    html = _html()
    alternative = _container("alternative", plain, html)
    sources = (((8,), ReferenceIndex(frozenset(), frozenset({"image.png"}))),)
    with (
        patch.object(
            mime_text_only,
            "_content_location_base",
            return_value="PUBLIC CHILD BASE",
        ) as location_base,
        patch.object(
            mime_text_only,
            "_html_reference_sources",
            return_value=sources,
        ) as references,
        patch.object(mime_text_only, "_resource_leaves", return_value=()) as resources,
    ):
        plan = mime_text_only._plan_alternative(
            alternative,
            (4,),
            "PUBLIC INHERITED BASE",
        )

    location_base.assert_called_once_with(alternative, "PUBLIC INHERITED BASE")
    references.assert_called_once_with(html, (4, 1), "PUBLIC CHILD BASE")
    resources.assert_called_once_with(
        html,
        (4, 1),
        ResourceScan(
            reference_sources=sources,
            inherited_base="PUBLIC CHILD BASE",
            include_text=False,
            root_inclusion=RootInclusion.OMIT,
        ),
    )
    assert plan.discarded_representations == (
        DiscardedBodyRepresentation((4, 1), "text/html"),
    )


def test_related_forwards_root_base_parameter_and_resource_scope_exactly() -> None:
    """Bind every nested planner and resource-audit input for related MIME."""
    root = _plain()
    resource = _binary()
    related = _container("related", root, resource)
    related.set_param("type", "text/plain")
    sources = (((7,), ReferenceIndex(frozenset(), frozenset())),)
    seed = _seed_plan((6, 0))
    with (
        patch.object(
            mime_text_only,
            "_content_location_base",
            return_value="PUBLIC CHILD BASE",
        ) as location_base,
        patch.object(related, "get_param", wraps=related.get_param) as get_param,
        patch.object(mime_text_only, "_plan_body", return_value=seed) as body,
        patch.object(
            mime_text_only,
            "_html_reference_sources",
            return_value=sources,
        ) as references,
        patch.object(mime_text_only, "_resource_leaves", return_value=()) as leaves,
    ):
        plan = mime_text_only._plan_related(
            related,
            (6,),
            "PUBLIC INHERITED BASE",
        )

    assert plan.discard_paths == ((6, 1),)
    location_base.assert_called_once_with(related, "PUBLIC INHERITED BASE")
    assert get_param.call_args_list == [call("start"), call("type")]
    body.assert_called_once_with(root, (6, 0), "PUBLIC CHILD BASE")
    references.assert_called_once_with(root, (6, 0), "PUBLIC CHILD BASE")
    leaves.assert_called_once_with(
        resource,
        (6, 1),
        ResourceScan(
            reference_sources=sources,
            inherited_base="PUBLIC CHILD BASE",
            include_text=True,
            root_inclusion=RootInclusion.INCLUDE,
        ),
    )


def test_mixed_forwards_selected_and_sibling_scope_exactly() -> None:
    """Keep one location base across selected analysis and sibling planning."""
    selected = _plain()
    sibling = _binary()
    mixed = _container("mixed", selected, sibling)
    sources = (((5,), ReferenceIndex(frozenset(), frozenset())),)
    seed = _seed_plan((5, 0))
    with (
        patch.object(
            mime_text_only,
            "_content_location_base",
            return_value="PUBLIC CHILD BASE",
        ) as location_base,
        patch.object(
            mime_text_only,
            "_html_reference_sources",
            return_value=sources,
        ) as references,
        patch.object(mime_text_only, "_plan_body", return_value=seed) as body,
        patch.object(
            mime_text_only,
            "_plan_mixed_sibling",
            return_value=seed,
        ) as sibling_plan,
    ):
        plan = mime_text_only._plan_mixed(
            mixed,
            (5,),
            "PUBLIC INHERITED BASE",
        )

    assert plan is seed
    location_base.assert_called_once_with(mixed, "PUBLIC INHERITED BASE")
    references.assert_called_once_with(selected, (5, 0), "PUBLIC CHILD BASE")
    body.assert_called_once_with(selected, (5, 0), "PUBLIC CHILD BASE")
    sibling_plan.assert_called_once_with(
        seed,
        sibling,
        (5, 1),
        "PUBLIC CHILD BASE",
        sources,
    )
