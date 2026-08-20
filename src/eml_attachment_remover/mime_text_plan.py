"""Define immutable source-path plans for text-only MIME transformations."""

from __future__ import annotations

import hashlib
from dataclasses import dataclass, replace
from typing import TYPE_CHECKING

from .mime_text_execution import canonical_text_payload
from .models import (
    CliError,
    DiscardedBodyRepresentation,
    DiscardedBodyResource,
    ExitCode,
    MimePath,
    RemovedPart,
    SelectedPlainTextBody,
    _format_mime_path,
)

if TYPE_CHECKING:
    from email.message import EmailMessage

HTML_LAYOUT_PAIR_REQUIRED = "an HTML layout source and rendered text must be paired"


@dataclass(frozen=True, slots=True)
class TextProjection:
    """Bind semantic text and an optional HTML layout source to one output payload."""

    selected_body: SelectedPlainTextBody
    selected_payload_sha256: str
    layout_source: MimePath | None = None
    rendered_text: str | None = None

    def __post_init__(self) -> None:
        """Require both halves of an optional HTML formatting projection.

        Raises:
            ValueError: If only one HTML layout value is supplied.

        """
        if (self.layout_source is None) is not (self.rendered_text is None):
            raise ValueError(HTML_LAYOUT_PAIR_REQUIRED)

    @property
    def expected_payload_sha256(self) -> str:
        """Digest required of the stored canonical plain text.

        Returns:
            The source digest for direct promotion or the projected UTF-8 digest.

        """
        if self.rendered_text is None:
            return self.selected_payload_sha256
        return hashlib.sha256(self.rendered_text.encode("utf-8")).hexdigest()


@dataclass(frozen=True, slots=True)
class TextOnlyPlan:
    """Store one projection and all immutable source-tree discard actions."""

    projection: TextProjection
    removed_attachments: tuple[RemovedPart, ...] = ()
    discarded_representations: tuple[DiscardedBodyRepresentation, ...] = ()
    discarded_resources: tuple[DiscardedBodyResource, ...] = ()
    discard_paths: tuple[MimePath, ...] = ()

    @property
    def selected_body(self) -> SelectedPlainTextBody:
        """Semantic source body selected for this plan."""
        return self.projection.selected_body

    @property
    def selected_payload_sha256(self) -> str:
        """Immutable semantic-source payload digest."""
        return self.projection.selected_payload_sha256

    @property
    def rendered_text(self) -> str | None:
        """Optional text rendered from the equivalent HTML layout source."""
        return self.projection.rendered_text

    @property
    def modified(self) -> bool:
        """Root-rewrite status.

        Returns:
            ``True`` for body promotion or any planned discard.

        """
        return bool(self.selected_body.path or self.discard_paths)

    @property
    def changed_paths(self) -> tuple[MimePath, ...]:
        """Source paths proving that the root message changed.

        Returns:
            Discard roots plus the promoted body path when it was nested.

        """
        if not self.selected_body.path:
            return self.discard_paths
        return (*self.discard_paths, self.selected_body.path)


def transformation_unavailable(path: MimePath, detail: str) -> CliError:
    """Return the stable fail-closed text-only transformation error.

    Returns:
        An expected exit-code-six error naming the unsafe MIME path.

    """
    return CliError(
        ExitCode.TRANSFORMATION_UNAVAILABLE,
        "cannot produce a text-only EML: "
        f"{detail} at MIME path {_format_mime_path(path)}",
    )


def selected_plain(part: EmailMessage, path: MimePath) -> TextOnlyPlan:
    """Bind one selected plain-text leaf to its original decoded payload.

    Returns:
        A plan containing its public identity and private SHA-256 precondition.

    """
    return TextOnlyPlan(
        projection=TextProjection(
            selected_body=SelectedPlainTextBody(path, part.get_content_type()),
            selected_payload_sha256=hashlib.sha256(
                canonical_text_payload(part),
            ).hexdigest(),
        ),
    )


def with_html_layout(
    plan: TextOnlyPlan,
    *,
    source: MimePath,
    rendered_text: str,
) -> TextOnlyPlan:
    """Return a plan whose output is rendered from one equivalent HTML source.

    Returns:
        A plan retaining the semantic body binding and recording the layout source.

    """
    return replace(
        plan,
        projection=replace(
            plan.projection,
            layout_source=source,
            rendered_text=rendered_text,
        ),
    )


def extend_plan(
    plan: TextOnlyPlan,
    *,
    attachments: tuple[RemovedPart, ...] = (),
    representations: tuple[DiscardedBodyRepresentation, ...] = (),
    resources: tuple[DiscardedBodyResource, ...] = (),
    discard_paths: tuple[MimePath, ...] = (),
) -> TextOnlyPlan:
    """Return a plan extended with complete immutable discard actions.

    Returns:
        A new plan preserving every action and audit category.

    """
    return TextOnlyPlan(
        projection=plan.projection,
        removed_attachments=(*plan.removed_attachments, *attachments),
        discarded_representations=(
            *plan.discarded_representations,
            *representations,
        ),
        discarded_resources=(*plan.discarded_resources, *resources),
        discard_paths=(*plan.discard_paths, *discard_paths),
    )


def sorted_plan(plan: TextOnlyPlan) -> TextOnlyPlan:
    """Return one source-path-sorted plan for deterministic reporting.

    Returns:
        A plan with every public audit record in MIME path order.

    """
    return TextOnlyPlan(
        projection=plan.projection,
        removed_attachments=tuple(
            sorted(plan.removed_attachments, key=lambda item: item.path),
        ),
        discarded_representations=tuple(
            sorted(plan.discarded_representations, key=lambda item: item.path),
        ),
        discarded_resources=tuple(
            sorted(plan.discarded_resources, key=lambda item: item.path),
        ),
        discard_paths=tuple(sorted(plan.discard_paths)),
    )
