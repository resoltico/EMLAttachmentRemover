"""Raw MIME indexing, strict-header, and transport-fidelity regressions."""

from __future__ import annotations

import hashlib
from typing import TYPE_CHECKING

import pytest

from eml_attachment_remover.batch import BatchOptions, execute
from eml_attachment_remover.domain import (
    BatchLedger,
    DecisionAction,
    ItemStatus,
    Removal,
    RemovalReason,
)
from eml_attachment_remover.mime_policy import classify
from eml_attachment_remover.mime_raw import RawNode, parse_raw_mime
from eml_attachment_remover.mime_removals import RemovalIndex
from eml_attachment_remover.mime_validation import ContentSpec

if TYPE_CHECKING:
    from pathlib import Path


def _options() -> BatchOptions:
    return BatchOptions(
        dry_run=False,
        existing="error",
        fail_fast=False,
        output=None,
        output_dir=None,
    )


def _run(
    tmp_path: Path, raw: bytes, *, name: str = "message.eml"
) -> tuple[Path, BatchLedger]:
    source = tmp_path / name
    source.write_bytes(raw)
    return source, execute([str(source)], _options())


def test_leaf_payload_fingerprint_excludes_multipart_delimiter_line(
    tmp_path: Path,
) -> None:
    raw = (
        b"Content-Type: multipart/mixed; boundary=m\r\n\r\n"
        b"--m\r\nContent-Type: text/plain\r\n\r\nbody\r\n"
        b"--m\r\nContent-Type: application/octet-stream\r\n"
        b"Content-Disposition: attachment\r\n\r\nremove\r\n--m--\r\n"
    )
    _source, ledger = _run(tmp_path, raw)
    retained = ledger.items[0].transformation
    assert retained is not None
    assert retained.retained[0].decoded_sha256 == hashlib.sha256(b"body").hexdigest()


@pytest.mark.parametrize(
    "raw",
    [
        (
            b"Content-Type: multipart/mixed; boundary=m\r\n"
            b"Content-Transfer-Encoding: base64\r\n\r\n"
            b"--m\r\nContent-Type: text/plain\r\n\r\nbody\r\n--m--\r\n"
        ),
        (
            b"Content-Type: multipart/mixed; boundary=m\r\n\r\n"
            b"--m\r\nContent-Type: text/plain\r\n\r\nbody\r\n"
            b"--m\r\nContent-Type: application/octet-stream\r\n"
            b"Content-Disposition: attachment; filename=foo bar\r\n\r\nremoved\r\n"
            b"--m--\r\n"
        ),
    ],
)
def test_parser_or_structural_header_defect_rejects_before_policy(
    tmp_path: Path, raw: bytes
) -> None:
    _source, ledger = _run(tmp_path, raw)
    assert ledger.items[0].status is ItemStatus.FAILED


@pytest.mark.parametrize(
    "attachment",
    [
        (
            b"Content-Type: multipart/mixed\r\nContent-Disposition: attachment\r\n\r\n"
            b"unindexed malformed inner bytes"
        ),
        (
            b"Content-Type: message/rfc822\r\nContent-Disposition: attachment\r\n\r\n"
            b"Subject: forwarded\r\n\r\nforwarded text\r\n"
        ),
    ],
)
def test_explicit_attachment_subtree_is_opaque_to_inner_defects(
    tmp_path: Path, attachment: bytes
) -> None:
    raw = (
        b"Content-Type: multipart/mixed; boundary=m\r\n\r\n"
        b"--m\r\nContent-Type: text/plain\r\n\r\nretained\r\n"
        b"--m\r\n" + attachment + b"\r\n--m--\r\n"
    )
    _source, ledger = _run(tmp_path, raw)
    assert ledger.items[0].status is ItemStatus.CREATED
    assert b"retained" in (tmp_path / "message.mime-pruned.eml").read_bytes()


def test_headerless_text_root_uses_rfc_default_without_regeneration(
    tmp_path: Path,
) -> None:
    _source, ledger = _run(tmp_path, b"plain source bytes\r\n")
    assert ledger.items[0].status is ItemStatus.CREATED
    assert (
        tmp_path / "message.mime-pruned.eml"
    ).read_bytes() == b"plain source bytes\r\n"


def test_rfc2231_filename_continuation_is_role_ambiguous_without_inline(
    tmp_path: Path,
) -> None:
    raw = b"Content-Type: text/plain; name*0=amb; name*1=iguous.txt\r\n\r\nbody\r\n"
    _source, ledger = _run(tmp_path, raw)
    assert ledger.items[0].status is ItemStatus.FAILED


def test_rfc2231_extended_attachment_filename_is_removed_without_decoding(
    tmp_path: Path,
) -> None:
    raw = (
        b"Content-Type: multipart/mixed; boundary=m\r\n\r\n"
        b"--m\r\nContent-Type: text/plain\r\n\r\nretained\r\n"
        b"--m\r\nContent-Type: application/octet-stream\r\n"
        b"Content-Disposition: attachment; filename*0*=utf-8''private; "
        b"filename*1*=.bin\r\n\r\nopaque\r\n--m--\r\n"
    )
    _source, ledger = _run(tmp_path, raw)
    assert ledger.items[0].status is ItemStatus.CREATED
    output = (tmp_path / "message.mime-pruned.eml").read_bytes()
    assert b"retained" in output
    assert b"private" not in output


def test_duplicate_rfc2231_continuation_segment_rejects_before_output(
    tmp_path: Path,
) -> None:
    raw = b"Content-Type: text/plain; name*0=first; name*0*=second\r\n\r\nbody\r\n"
    _source, ledger = _run(tmp_path, raw)
    assert ledger.items[0].status is ItemStatus.FAILED
    assert not (tmp_path / "message.mime-pruned.eml").exists()


def test_related_cfws_message_identifiers_select_the_exact_root(tmp_path: Path) -> None:
    raw = (
        b'Content-Type: multipart/related; boundary=r; start="(lead) <root@x> (tail)"; '
        b'start-info="(lead) <root@x> (tail)"\r\n\r\n'
        b"--r\r\nContent-Type: text/plain\r\n"
        b"Content-ID: (lead) <root@x> (tail)\r\n\r\nretained\r\n"
        b"--r\r\nContent-Type: image/png\r\n\r\nopaque\r\n--r--\r\n"
    )
    _source, ledger = _run(tmp_path, raw)
    assert ledger.items[0].status is ItemStatus.CREATED
    output = (tmp_path / "message.mime-pruned.eml").read_bytes()
    assert b"retained" in output
    assert b"image/png" not in output


def test_cr_only_wire_transport_is_indexed_and_pruned_byte_exactly(
    tmp_path: Path,
) -> None:
    raw = (
        b"Content-Type: multipart/mixed; boundary=m\r\r"
        b"--m\rContent-Type: text/plain\r\rretained\r"
        b"--m\rContent-Type: application/octet-stream\r"
        b"Content-Disposition: attachment\r\ropaque\r--m--\r"
    )
    _source, ledger = _run(tmp_path, raw)
    assert ledger.items[0].status is ItemStatus.CREATED
    assert (tmp_path / "message.mime-pruned.eml").read_bytes() == (
        b"Content-Type: multipart/mixed; boundary=m\r\r"
        b"--m\rContent-Type: text/plain\r\rretained\r"
        b"--m--\r"
    )


def test_removal_prefix_index_visits_only_retained_source_tree_nodes() -> None:
    raw = (
        b"Content-Type: multipart/mixed; boundary=m\r\n\r\n"
        b"--m\r\nContent-Type: text/plain\r\n\r\nfirst\r\n"
        b"--m\r\nContent-Type: multipart/mixed; boundary=n\r\n\r\n"
        b"--n\r\nContent-Type: application/octet-stream\r\n"
        b"Content-Disposition: attachment\r\n\r\nopaque\r\n--n--\r\n"
        b"--m\r\nContent-Type: text/html\r\n\r\nlast\r\n--m--\r\n"
    )
    tree = parse_raw_mime(raw)
    policy = classify(tree.root)
    index = RemovalIndex.from_roots({removal.path for removal in policy.removals})
    assert [node.path for node in index.retained_nodes(tree.root)] == [
        (),
        (0,),
        (1,),
        (2,),
    ]
    assert index.changed_ancestor_paths(tree.root) == {(), (1,)}


def test_mixed_policy_exposes_the_complete_action_and_removal_plan() -> None:
    """Require mixed classification to retain every observable decision field."""
    raw = (
        b"Content-Type: multipart/mixed; boundary=m\r\n\r\n"
        b"--m\r\nContent-Type: text/plain\r\n\r\nbody\r\n"
        b"--m\r\nContent-Type: application/octet-stream\r\n"
        b"Content-Disposition: attachment\r\n\r\nremoved\r\n--m--\r\n"
    )
    plan = classify(parse_raw_mime(raw).root)
    assert plan.actions == {
        (): DecisionAction.RECURSE,
        (0,): DecisionAction.KEEP,
        (1,): DecisionAction.REMOVE_SUBTREE,
    }
    assert plan.removals == (
        Removal((1,), "application/octet-stream", RemovalReason.EXPLICIT_ATTACHMENT),
    )


def test_removal_prefix_index_has_linear_operation_receipt_at_ten_thousand_parts() -> (
    None
):
    text = ContentSpec("text/plain", {})
    root = RawNode((), 0, 0, 0, (), ContentSpec("multipart/mixed", {}), None, "7bit")
    root.children = [
        RawNode((index,), 0, 0, 0, (), text, None, "7bit") for index in range(10_000)
    ]
    index = RemovalIndex.from_roots({(part,) for part in range(0, 10_000, 2)})
    traversal = index.traverse(root)
    assert traversal.node_visits == 10_001
    assert traversal.child_edge_lookups == 10_000
    assert [node.path for node in traversal.nodes[:3]] == [(), (1,), (3,)]
    assert len(traversal.nodes) == 5_001


def test_headerless_first_line_before_blank_line_is_preserved_as_body(
    tmp_path: Path,
) -> None:
    raw = b"plain body line\r\n\r\nsecond body line\r\n"
    _source, ledger = _run(tmp_path, raw)
    assert ledger.items[0].status is ItemStatus.CREATED
    assert (tmp_path / "message.mime-pruned.eml").read_bytes() == raw


def test_headerless_prose_with_a_colon_is_preserved_as_body(tmp_path: Path) -> None:
    raw = b"Meeting at 10:00\r\n\r\nDetails\r\n"
    _source, ledger = _run(tmp_path, raw)
    assert ledger.items[0].status is ItemStatus.CREATED
    assert (tmp_path / "message.mime-pruned.eml").read_bytes() == raw


def test_header_like_child_without_separator_fails_closed(tmp_path: Path) -> None:
    raw = (
        b"Content-Type: multipart/mixed; boundary=m\r\n\r\n"
        b"--m\r\nContent-Type: text/plain\r\n\r\nretained\r\n"
        b"--m\r\nContent-Type: application/octet-stream\r\n"
        b"SECRET-WITHOUT-SEPARATOR\r\n--m--\r\n"
    )
    _source, ledger = _run(tmp_path, raw)
    assert ledger.items[0].status is ItemStatus.FAILED
    assert not (tmp_path / "message.mime-pruned.eml").exists()


def test_related_nonroot_malformed_multipart_stays_opaque_and_is_removed(
    tmp_path: Path,
) -> None:
    raw = (
        b"Content-Type: multipart/related; boundary=r\r\n\r\n"
        b"--r\r\nContent-Type: text/html\r\n\r\n<body>retained</body>\r\n"
        b"--r\r\nContent-Type: multipart/mixed; boundary=broken\r\n\r\n"
        b"unparseable nested resource bytes\r\n--r--\r\n"
    )
    _source, ledger = _run(tmp_path, raw)
    assert ledger.items[0].status is ItemStatus.CREATED
    output = (tmp_path / "message.mime-pruned.eml").read_bytes()
    assert b"retained" in output
    assert b"broken" not in output


def test_invalid_rfc2231_attachment_header_cannot_authorize_opaque_removal(
    tmp_path: Path,
) -> None:
    raw = (
        b"Content-Type: multipart/mixed; boundary=m\r\n\r\n"
        b"--m\r\nContent-Type: text/plain\r\n\r\nretained\r\n"
        b"--m\r\nContent-Type: application/octet-stream\r\n"
        b"Content-Disposition: attachment; filename*=nonsense\r\n\r\nopaque\r\n"
        b"--m--\r\n"
    )
    _source, ledger = _run(tmp_path, raw)
    assert ledger.items[0].status is ItemStatus.FAILED
    assert not (tmp_path / "message.mime-pruned.eml").exists()


@pytest.mark.parametrize("control", [b"\v", b"\f"])
def test_retained_base64_rejects_non_transport_ascii_white_space(
    tmp_path: Path, control: bytes
) -> None:
    raw = (
        b"Content-Type: text/plain\r\nContent-Transfer-Encoding: base64\r\n\r\n"
        b"YQ==" + control + b"\r\n"
    )
    _source, ledger = _run(tmp_path, raw)
    assert ledger.items[0].status is ItemStatus.FAILED
    assert not (tmp_path / "message.mime-pruned.eml").exists()


@pytest.mark.parametrize("count", [500, 1_000])
def test_opaque_attachment_stdlib_validation_has_linear_work_receipt(
    count: int,
) -> None:
    attachment = (
        b"--m\r\nContent-Type: application/octet-stream\r\n"
        b"Content-Disposition: attachment\r\n\r\nopaque\r\n"
    )
    raw = (
        b"Content-Type: multipart/mixed; boundary=m\r\n\r\n"
        b"--m\r\nContent-Type: text/plain\r\n\r\nretained\r\n"
        + attachment * count
        + b"--m--\r\n"
    )
    work = parse_raw_mime(raw).stdlib_work
    assert work.node_visits == count + 2
    assert work.child_edge_visits == count + 1
