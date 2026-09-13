"""Independent mechanical verification of a source-bound MIME candidate."""

from __future__ import annotations

import hashlib
from typing import TYPE_CHECKING

from .domain import (
    AppError,
    ExitCode,
    RetainedFingerprint,
    VerificationReceipt,
)
from .mime_encoding import fingerprint_retained
from .mime_policy import classify
from .mime_raw import iter_nodes, parse_raw_mime
from .mime_removals import RemovalIndex

if TYPE_CHECKING:
    from .mime_execution import Candidate
    from .mime_raw import RawMimeTree, RawNode


# This deliberately does not import the executor's corresponding policy.  Candidate
# verification is a separate proof: an executor change that accidentally retains a
# stale field must cause verification to fail rather than changing both sides of the
# comparison together.
_VERIFIER_TRANSPORT_HEADERS = frozenset({
    b"dkim-signature",
    b"domainkey-signature",
    b"arc-seal",
    b"arc-message-signature",
    b"arc-authentication-results",
})
_VERIFIER_CHANGED_HEADERS = frozenset({b"content-md5", b"content-length", b"lines"})


def _kept_nodes(tree: RawMimeTree, roots: set[tuple[int, ...]]) -> list[RawNode]:
    """Return retained nodes in source order, excluding removed subtrees.

    Returns:
        Source-order nodes outside every planned removal root.

    """
    return list(RemovalIndex.from_roots(roots).retained_nodes(tree.root))


def _shape(
    nodes: list[RawNode],
) -> tuple[tuple[str, str, tuple[tuple[bytes, bytes], ...]], ...]:
    """Capture retained semantic content headers without relying on source paths.

    Returns:
        Media type, CTE, and semantic parameter facts in retained source order.

    """
    return tuple(
        (node.media_type, node.cte, tuple(sorted(node.content_type.parameters.items())))
        for node in nodes
    )


def _fingerprint_shape(
    entries: tuple[RetainedFingerprint, ...],
) -> tuple[tuple[str, str, tuple[tuple[bytes, bytes], ...], str, str], ...]:
    """Discard source paths while preserving payload order and evidence.

    Returns:
        Ordered MIME metadata and encoded/decoded evidence independent of paths.

    """
    return tuple(
        (
            entry.content_type,
            entry.cte,
            entry.content_type_parameters,
            entry.encoded_sha256,
            entry.decoded_sha256,
        )
        for entry in entries
    )


def verify_candidate(
    source: RawMimeTree,
    candidate: Candidate,
    removal_roots: set[tuple[int, ...]],
) -> tuple[VerificationReceipt, tuple[RetainedFingerprint, ...]]:
    """Reparse and verify a candidate without consuming planner-built evidence.

    Returns:
        Mechanical receipt and independently recomputed retained source fingerprints.

    Raises:
        AppError: If parsing, policy idempotence, structure, or fidelity proof fails.

    """
    output = parse_raw_mime(candidate.raw)
    independently_rebuilt = _expected_raw(source, removal_roots)
    expected_nodes = _kept_nodes(source, removal_roots)
    actual_nodes = list(iter_nodes(output.root))
    expected = fingerprint_retained(source.raw, expected_nodes)
    actual = fingerprint_retained(output.raw, actual_nodes)
    payloads_match = _fingerprint_shape(expected) == _fingerprint_shape(actual)
    structure_matches = candidate.raw == independently_rebuilt and _shape(
        expected_nodes
    ) == _shape(actual_nodes)
    second = classify(output.root)
    idempotent = not second.removals
    digest_matches = hashlib.sha256(candidate.raw).hexdigest() == candidate.digest
    receipt = VerificationReceipt(
        output_parses=True,
        retained_payloads_match=payloads_match,
        structure_matches=structure_matches,
        policy_is_idempotent=idempotent,
        digest_matches=digest_matches,
    )
    if not all((
        receipt.retained_payloads_match,
        receipt.structure_matches,
        receipt.policy_is_idempotent,
        receipt.digest_matches,
    )):
        raise AppError(
            ExitCode.VERIFICATION_ERROR, "candidate MIME verification failed"
        )
    return receipt, expected


def _expected_raw(source: RawMimeTree, roots: set[tuple[int, ...]]) -> bytes:
    """Independently prove that only policy-authorized source spans were deleted.

    Returns:
        Source bytes after independent removal and stale-header span deletion.

    Raises:
        AppError: If a root lacks a span or independent edits overlap.

    """
    nodes = source.by_path
    edits: list[tuple[int, int]] = []
    index = RemovalIndex.from_roots(roots)
    for root in roots:
        node = nodes[root]
        if node.delete_start is None or node.delete_end is None:
            raise AppError(ExitCode.VERIFICATION_ERROR, "removal root lacks raw span")
        edits.append((node.delete_start, node.delete_end))
    for path in index.changed_ancestor_paths(source.root):
        allowed = set(_VERIFIER_CHANGED_HEADERS)
        if path == ():
            allowed.update(_VERIFIER_TRANSPORT_HEADERS)
            allowed.add(b"x-ms-has-attach")
        edits.extend(
            (field.start, field.end)
            for field in nodes[path].headers
            if field.name in allowed
        )
    position = 0
    chunks: list[bytes] = []
    for start, end in sorted(edits):
        if start < position or end <= start:
            raise AppError(
                ExitCode.VERIFICATION_ERROR, "invalid verifier deletion spans"
            )
        chunks.append(source.raw[position:start])
        position = end
    chunks.append(source.raw[position:])
    return b"".join(chunks)
