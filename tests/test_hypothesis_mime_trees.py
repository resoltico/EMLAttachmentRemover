"""Property-test MIME traversal against an independent recursive model."""

from __future__ import annotations

import tempfile
from collections import Counter
from dataclasses import dataclass
from email import policy
from email.message import EmailMessage
from pathlib import Path
from typing import Final, Literal

from hypothesis import event, example, given, target
from hypothesis import strategies as st

from eml_attachment_remover import process_file
from eml_attachment_remover.models import (
    KeepReason,
    PreservedFilePart,
    ProcessResult,
    RemovedPart,
)
from tests.test_support import decoded_hash, parse

type LeafKind = Literal["attachment", "cid", "inline", "legacy", "location", "unnamed"]
type MediaKind = Literal["binary", "html", "plain"]
type ContainerKind = Literal["alternative", "mixed", "related"]
type MimeNode = LeafNode | ContainerNode
type LeafSnapshot = tuple[
    str,
    str | None,
    str | None,
    str | None,
    str | None,
    str,
]


@dataclass(frozen=True, slots=True)
class LeafNode:
    """Describe one generated non-multipart MIME entity."""

    kind: LeafKind
    media: MediaKind
    payload: bytes


@dataclass(frozen=True, slots=True)
class ContainerNode:
    """Describe one generated multipart MIME entity."""

    kind: ContainerKind
    children: tuple[MimeNode, ...]


@dataclass(frozen=True, slots=True)
class ModeledCase:
    """Store one generated wire message and its independent exact oracle."""

    raw: bytes
    removed: tuple[RemovedPart, ...]
    preserved: tuple[PreservedFilePart, ...]


LEAF: Final[st.SearchStrategy[MimeNode]] = st.builds(
    LeafNode,
    kind=st.sampled_from((
        "attachment",
        "cid",
        "inline",
        "legacy",
        "location",
        "unnamed",
    )),
    media=st.sampled_from(("binary", "html", "plain")),
    payload=st.integers(min_value=0, max_value=256).flatmap(
        lambda size: st.binary(min_size=size, max_size=size)
    ),
)
MIME_TREE: Final[st.SearchStrategy[MimeNode]] = st.recursive(
    LEAF,
    lambda children: st.builds(
        ContainerNode,
        kind=st.sampled_from(("alternative", "mixed", "related")),
        children=st.lists(children, min_size=1, max_size=3).map(tuple),
    ),
    max_leaves=8,
)


def _multipart(kind: ContainerKind) -> EmailMessage:
    """Create one multipart container from the independent model.

    Returns:
        The configured multipart entity.

    """
    part = EmailMessage()
    if kind == "alternative":
        part.make_alternative()
    elif kind == "mixed":
        part.make_mixed()
    else:
        part.make_related()
    return part


def _leaf_part(node: LeafNode, path: tuple[int, ...]) -> EmailMessage:
    """Create one uniquely identifiable leaf from the independent model.

    Returns:
        The configured non-multipart entity.

    """
    part = EmailMessage()
    maintype, subtype, extension = {
        "binary": ("application", "octet-stream", "bin"),
        "html": ("text", "html", "html"),
        "plain": ("text", "plain", "txt"),
    }[node.media]
    part.set_content(node.payload, maintype=maintype, subtype=subtype, cte="base64")
    identity = "-".join(map(str, path))
    filename = f"public-leaf-{identity}.{extension}"
    if node.kind == "attachment":
        part.add_header("Content-Disposition", "attachment", filename=filename)
    elif node.kind == "inline":
        part.add_header("Content-Disposition", "inline", filename=filename)
    elif node.kind != "unnamed":
        part.set_param("name", filename, header="Content-Type")
    if node.kind == "cid":
        part["Content-ID"] = f"<public-leaf-{identity}@example.test>"
    elif node.kind == "location":
        part["Content-Location"] = f"public/leaf-{identity}.{extension}"
    return part


def _tree_part(node: MimeNode, path: tuple[int, ...]) -> EmailMessage:
    """Build a canonical MIME entity while retaining model path identity.

    Returns:
        The recursively configured MIME entity.

    """
    if isinstance(node, LeafNode):
        return _leaf_part(node, path)
    container = _multipart(node.kind)
    for index, child in enumerate(node.children):
        container.attach(_tree_part(child, (*path, index)))
    return container


def _leaf_decision(
    node: LeafNode,
    *,
    parent: ContainerKind,
    under_related: bool,
) -> tuple[bool, KeepReason | None]:
    """Return the model's removal decision and optional preservation reason.

    Returns:
        Whether to remove the leaf and any reason to report when retaining it.

    """
    if node.kind == "attachment":
        return True, None
    if node.kind == "inline":
        return False, KeepReason.INLINE_DISPOSITION
    if under_related:
        reason = None if node.kind == "unnamed" else KeepReason.RELATED_RESOURCE
        return False, reason
    if node.kind == "cid":
        return False, KeepReason.CONTENT_ID
    if node.kind == "location":
        return False, KeepReason.CONTENT_LOCATION
    alternative_body = parent == "alternative" and node.media in {"html", "plain"}
    removable = node.kind == "legacy" and not alternative_body
    return removable, None


def _expected_records(
    node: MimeNode,
    *,
    path: tuple[int, ...],
    parent: ContainerKind,
    under_related: bool,
) -> tuple[list[RemovedPart], list[PreservedFilePart]]:
    """Calculate exact expected traversal records without product helpers.

    Returns:
        Exact removal and preservation records in traversal order.

    """
    if isinstance(node, LeafNode):
        part = _leaf_part(node, path)
        remove, reason = _leaf_decision(
            node,
            parent=parent,
            under_related=under_related,
        )
        if remove:
            return [
                RemovedPart(
                    path,
                    part.get_content_type(),
                    part.get_filename(),
                    part.get_content_disposition(),
                )
            ], []
        if reason is None:
            return [], []
        return [], [
            PreservedFilePart(
                path, part.get_content_type(), part.get_filename(), reason
            )
        ]
    removed: list[RemovedPart] = []
    preserved: list[PreservedFilePart] = []
    descendants_under_related = under_related or node.kind == "related"
    for index, child in enumerate(node.children):
        child_removed, child_preserved = _expected_records(
            child,
            path=(*path, index),
            parent=node.kind,
            under_related=descendants_under_related,
        )
        removed.extend(child_removed)
        preserved.extend(child_preserved)
    return removed, preserved


def _snapshot(part: EmailMessage) -> LeafSnapshot:
    """Return all policy-bearing leaf metadata plus its decoded digest.

    Returns:
        The leaf metadata and content digest.

    """
    return (
        part.get_content_type(),
        part.get_content_disposition(),
        part.get_filename(),
        part.get("Content-ID"),
        part.get("Content-Location"),
        decoded_hash(part),
    )


def _leaf_snapshots(message: EmailMessage) -> Counter[LeafSnapshot]:
    """Return an exact multiset of every leaf in one MIME message.

    Returns:
        The multiset of leaf snapshots.

    """
    return Counter(
        _snapshot(part) for part in message.walk() if not part.is_multipart()
    )


def _metrics(node: MimeNode) -> tuple[int, int, frozenset[str]]:
    """Return leaf count, depth, and semantic kinds for observability.

    Returns:
        Leaf count, tree depth, and represented semantic kinds.

    """
    if isinstance(node, LeafNode):
        return 1, 1, frozenset({node.kind, node.media})
    child_metrics = [_metrics(child) for child in node.children]
    return (
        sum(metric[0] for metric in child_metrics),
        1 + max(metric[1] for metric in child_metrics),
        frozenset({node.kind}).union(*(metric[2] for metric in child_metrics)),
    )


def _modeled_case(tree: MimeNode) -> ModeledCase:
    """Build one source and exact result oracle from the recursive model.

    Returns:
        The serialized source and expected traversal records.

    """
    message = EmailMessage()
    message["Subject"] = "Generated recursive public MIME tree"
    message["X-Public-Invariant"] = "retained-root-header"
    message.set_content("Always-retained root body")
    message.make_mixed()
    message.attach(_tree_part(tree, (1,)))
    removed, preserved = _expected_records(
        tree,
        path=(1,),
        parent="mixed",
        under_related=False,
    )
    return ModeledCase(
        message.as_bytes(policy=policy.SMTP),
        tuple(removed),
        tuple(preserved),
    )


def _observe_tree(tree: MimeNode, removable_count: int) -> None:
    """Publish semantic distribution and targeting data for one tree."""
    leaves, depth, kinds = _metrics(tree)
    event(f"tree-depth={min(depth, 5)}")
    event(f"leaf-count={leaves}")
    for kind in sorted(kinds):
        event(f"semantic-kind={kind}")
    target(depth, label="MIME tree depth")
    target(leaves, label="MIME leaf count")
    target(removable_count, label="removable leaf count")


def _expected_snapshots(
    source: Path,
    expected_removed: list[RemovedPart],
) -> Counter[LeafSnapshot]:
    """Calculate output leaves after excluding exactly modeled removals.

    Returns:
        The expected retained leaf multiset.

    """
    removed_names = {record.filename for record in expected_removed}
    return Counter(
        _snapshot(part)
        for part in parse(source).walk()
        if not part.is_multipart() and part.get_filename() not in removed_names
    )


def _run_twice(
    source: Path,
    first_output: Path,
    second_output: Path,
) -> tuple[ProcessResult, ProcessResult]:
    """Process one source twice for semantic and byte idempotence checks.

    Returns:
        The first and second process results.

    """
    first = process_file(source, first_output, force=False, dry_run=False)
    second = process_file(first_output, second_output, force=False, dry_run=False)
    return first, second


@example(
    ContainerNode(
        "related",
        (
            LeafNode("attachment", "binary", b"remove"),
            LeafNode("inline", "binary", b"retain"),
        ),
    )
)
@given(tree=MIME_TREE)
def test_recursive_tree_matches_the_model_and_is_idempotent(
    tree: MimeNode,
) -> None:
    """Check recursive MIME behavior against an exact independent oracle."""
    modeled = _modeled_case(tree)
    _observe_tree(tree, len(modeled.removed))
    with tempfile.TemporaryDirectory() as directory:
        source = Path(directory) / "source.eml"
        first_output = Path(directory) / "first-output.eml"
        second_output = Path(directory) / "second-output.eml"
        source.write_bytes(modeled.raw)
        expected_snapshots = _expected_snapshots(source, list(modeled.removed))
        first, second = _run_twice(source, first_output, second_output)
        first_message = parse(first_output)
        assert source.read_bytes() == modeled.raw
        assert first.removed == modeled.removed
        assert first.preserved_file_parts == modeled.preserved
        assert _leaf_snapshots(first_message) == expected_snapshots
        assert first_message["X-Public-Invariant"] == "retained-root-header"
        assert [
            defect for part in first_message.walk() for defect in part.defects
        ] == []
        assert second.removed == ()
        assert second_output.read_bytes() == first_output.read_bytes()
