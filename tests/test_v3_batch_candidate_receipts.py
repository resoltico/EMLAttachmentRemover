"""Complete candidate-construction receipts for v3 batch mutation contracts."""

from __future__ import annotations

from typing import TYPE_CHECKING

from eml_attachment_remover import batch
from eml_attachment_remover.domain import (
    BatchLedger,
    ItemPhase,
    TransformationPlan,
)
from eml_attachment_remover.mime_encoding import fingerprint_retained
from eml_attachment_remover.mime_execution import build_candidate
from eml_attachment_remover.mime_policy import classify
from eml_attachment_remover.mime_raw import parse_raw_mime
from eml_attachment_remover.mime_removals import RemovalIndex
from eml_attachment_remover.mime_verification import verify_candidate
from eml_attachment_remover.native_paths import (
    inspect_source_identity,
    path_value,
    read_source,
)

if TYPE_CHECKING:
    from pathlib import Path


def _attachment_message() -> bytes:
    return (
        b"Content-Type: multipart/mixed; boundary=m\r\n\r\n"
        b"--m\r\nContent-Type: text/plain\r\n\r\nkeep\r\n"
        b"--m\r\nContent-Type: application/octet-stream\r\n"
        b"Content-Disposition: attachment\r\n\r\nremove\r\n--m--\r\n"
    )


def test_candidate_records_every_independently_recomputed_fact(tmp_path: Path) -> None:
    """Candidate construction is a complete immutable transformation receipt."""
    source = tmp_path / "message.eml"
    source.write_bytes(_attachment_message())
    item = BatchLedger.from_requests([path_value(str(source))]).items[0]
    snapshot = read_source(str(source))
    tree = parse_raw_mime(snapshot.raw)
    policy = classify(tree.root)
    roots = {removal.path for removal in policy.removals}
    retained = fingerprint_retained(
        tree.raw, list(RemovalIndex.from_roots(roots).retained_nodes(tree.root))
    )
    candidate = build_candidate(tree, policy.removals)
    receipt, recomputed = verify_candidate(tree, candidate, roots)

    batch._candidate(  # ruff: ignore[private-member-access] - full batch candidate receipt.
        item, inspect_source_identity(str(source))
    )

    assert item.source == snapshot
    assert item.phase is ItemPhase.CANDIDATE
    assert item.transformation == TransformationPlan(
        policy.removals,
        recomputed,
        candidate.stripped_headers,
        candidate.digest,
        len(candidate.raw),
        candidate.raw,
    )
    assert item.verification == receipt
    assert retained == recomputed
    assert item.warnings == []


def test_candidate_records_each_opaque_charset_warning_without_codec_lookup(
    tmp_path: Path,
) -> None:
    """An opaque charset label is retained and reported from raw source bytes only."""
    source = tmp_path / "charset.eml"
    source.write_bytes(b"Content-Type: text/plain; charset=x-opaque\r\n\r\nbody\r\n")
    item = BatchLedger.from_requests([path_value(str(source))]).items[0]
    batch._candidate(  # ruff: ignore[private-member-access] - exact opaque-charset warning receipt.
        item, inspect_source_identity(str(source))
    )
    assert item.warnings == [
        {
            "code": "CHARSET_PRESERVED_OPAQUE",
            "mime_path": (),
            "charset_base64": "eC1vcGFxdWU=",
            "message": "charset label was preserved without codec lookup",
        }
    ]
