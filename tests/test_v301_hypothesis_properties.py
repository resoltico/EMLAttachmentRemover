"""Generated v3.0.1 MIME and terminal-ledger exploration over synthetic inputs."""

from __future__ import annotations

import hashlib
import string
from pathlib import Path
from tempfile import TemporaryDirectory

import pytest
from hypothesis import given
from hypothesis import strategies as st
from hypothesis.stateful import RuleBasedStateMachine, invariant, rule

from eml_attachment_remover import batch
from eml_attachment_remover.domain import (
    AppError,
    BatchLedger,
    ExitCode,
    FileIdentity,
    ItemStatus,
    Removal,
    RemovalReason,
    SourceSnapshot,
    TransformationPlan,
)
from eml_attachment_remover.mime_execution import build_candidate
from eml_attachment_remover.mime_headers import line_end
from eml_attachment_remover.mime_policy import classify
from eml_attachment_remover.mime_raw import parse_raw_mime
from eml_attachment_remover.mime_verifier_policy import authorize_removals
from eml_attachment_remover.native_paths import (
    bind_destination,
    default_destination,
    path_value,
)
from eml_attachment_remover.staged_output import publish


def _ascii_bytes(value: str) -> bytes:
    """Return one explicitly ASCII synthetic MIME payload.

    Returns:
        The exact ASCII bytes for a generated constrained string.

    """
    return value.encode("ascii")


def _mixed_message(
    terminator: bytes, retained: bytes, removed: bytes
) -> tuple[bytes, bytes]:
    """Return one closed-policy mixed source and its independently assembled result.

    Returns:
        The source message and the expected raw-span-deletion result.

    """
    headers = b"Content-Type: multipart/mixed; boundary=m" + terminator + terminator
    retained_part = (
        b"--m"
        + terminator
        + b"Content-Type: text/plain"
        + terminator
        + terminator
        + retained
        + terminator
    )
    removed_part = (
        b"--m"
        + terminator
        + b"Content-Type: application/octet-stream"
        + terminator
        + b"Content-Disposition: attachment"
        + terminator
        + terminator
        + removed
        + terminator
    )
    final = b"--m--" + terminator
    return (
        headers + retained_part + removed_part + final,
        headers + retained_part + final,
    )


@given(
    st.lists(st.sampled_from((b"\r\n", b"\n", b"\r", b"x")), min_size=1, max_size=32),
)
def test_v301_property_wire_terminators_keep_exact_bounded_spans(  # type: ignore[misc]
    pieces: list[bytes],
) -> None:
    """The scanner's output is the independently selected first line boundary."""
    raw = b"".join(pieces)
    expected = len(raw)
    for index, byte in enumerate(raw):
        if byte in b"\r\n":
            expected = index + 1
            if (
                byte == ord("\r")
                and index + 1 < len(raw)
                and raw[index + 1] == ord("\n")
            ):
                expected += 1
            break
    assert line_end(raw, 0, len(raw)) == expected


@given(
    st.sampled_from((b"\r\n", b"\n", b"\r")),
    st.text(alphabet="abcdef", min_size=1, max_size=32).map(_ascii_bytes),
    st.text(alphabet="uvwxyz", min_size=1, max_size=32).map(_ascii_bytes),
)
def test_v301_property_multipart_span_edits_keep_wire_bytes_exact(  # type: ignore[misc]
    terminator: bytes, retained: bytes, removed: bytes
) -> None:
    """Raw-span deletion removes only the independently enumerated attachment span."""
    raw, expected = _mixed_message(terminator, retained, removed)
    tree = parse_raw_mime(raw)
    claim = Removal((1,), "application/octet-stream", RemovalReason.EXPLICIT_ATTACHMENT)
    candidate = build_candidate(tree, (claim,))
    assert candidate.raw == expected
    assert retained in candidate.raw
    assert line_end(b"x\r\n", 0, 2) == 2


@given(st.text(alphabet=string.ascii_lowercase, min_size=1, max_size=12))
def test_v301_property_structured_comments_preserve_content_type_semantics(  # type: ignore[misc]
    label: str,
) -> None:
    """Permitted trailing comments do not change a synthetic Content-Type token."""
    raw = (
        b"Content-Type: text/plain; charset=us-ascii ("
        + label.encode("ascii")
        + b")\r\n\r\nbody\r\n"
    )
    assert parse_raw_mime(raw).root.media_type == "text/plain"


@given(st.text(alphabet=string.ascii_lowercase, min_size=1, max_size=8))
def test_v301_property_structured_grammar_keeps_comments_and_rejects_splices(  # type: ignore[misc]
    label: str,
) -> None:
    """Keep supported CFWS semantic-free and reject malformed token splices."""
    comment = label.encode("ascii")
    valid = (
        b"Content-Type: text/plain; charset=us-ascii (outer ("
        + comment
        + b"))\r\nContent-Transfer-Encoding: 7bit (ASCII)\r\n\r\nbody\r\n"
    )
    assert parse_raw_mime(valid).root.media_type == "text/plain"
    malformed = b"Content-Type: text/plain; charset=us-ascii (" + comment
    with pytest.raises(AppError):
        parse_raw_mime(malformed + b"\r\n\r\nbody\r\n")


@given(
    st.sampled_from((
        RemovalReason.EXPLICIT_ATTACHMENT,
        RemovalReason.RELATED_NONROOT_COMPONENT,
    ))
)
def test_v301_property_verifier_rejects_each_wrong_attachment_claim_reason(  # type: ignore[misc]
    reason: RemovalReason,
) -> None:
    """Reject an attachment claim with an independently wrong reason."""
    raw = (
        b"Content-Type: multipart/mixed; boundary=m\r\n\r\n"
        b"--m\r\nContent-Type: text/plain\r\n\r\nkeep\r\n"
        b"--m\r\nContent-Type: application/octet-stream\r\n"
        b"Content-Disposition: attachment\r\n\r\nremove\r\n--m--\r\n"
    )
    tree = parse_raw_mime(raw)
    correct = Removal(
        (1,), "application/octet-stream", RemovalReason.EXPLICIT_ATTACHMENT
    )
    if reason is RemovalReason.EXPLICIT_ATTACHMENT:
        authorize_removals(tree, (correct,))
        return
    wrong = Removal((1,), "application/octet-stream", reason)
    with pytest.raises(AppError) as rejected:
        authorize_removals(tree, (wrong,))
    assert rejected.value == AppError(
        ExitCode.VERIFICATION_ERROR, "removal authorization mismatch"
    )


@given(forged=st.booleans())
def test_v301_property_source_policy_rejects_forged_attachment_paths(  # type: ignore[misc]
    *,
    forged: bool,
) -> None:
    """The source oracle accepts the one permitted root and rejects altered claims."""
    raw, _expected = _mixed_message(b"\r\n", b"keep", b"drop")
    tree = parse_raw_mime(raw)
    authorized = Removal(
        (1,), "application/octet-stream", RemovalReason.EXPLICIT_ATTACHMENT
    )
    claim = (
        Removal((0,), "text/plain", RemovalReason.EXPLICIT_ATTACHMENT)
        if forged
        else authorized
    )
    if forged:
        with pytest.raises(AppError):
            authorize_removals(tree, (claim,))
    else:
        authorize_removals(tree, (claim,))


@given(st.text(alphabet=string.ascii_lowercase, min_size=1, max_size=32))
def test_v301_property_retained_text_bytes_are_candidate_identical(  # type: ignore[misc]
    body: str,
) -> None:
    """Keep every synthetic retained text octet when no removal is authorized."""
    raw = b"Content-Type: text/plain\r\n\r\n" + body.encode("ascii") + b"\r\n"
    tree = parse_raw_mime(raw)
    assert build_candidate(tree, ()).raw == raw


@given(st.text(alphabet="abcdef", min_size=1, max_size=48).map(_ascii_bytes))
def test_v301_property_retained_octets_and_structure_survive_attachment_removal(  # type: ignore[misc]
    retained: bytes,
) -> None:
    """A removal cannot alter the retained part's original payload or topology."""
    raw, expected = _mixed_message(b"\r\n", retained, b"attachment")
    tree = parse_raw_mime(raw)
    claim = Removal((1,), "application/octet-stream", RemovalReason.EXPLICIT_ATTACHMENT)
    candidate = build_candidate(tree, (claim,)).raw
    parsed = parse_raw_mime(candidate)
    assert candidate == expected
    assert retained in candidate
    assert len(parsed.root.children) == 1
    assert parsed.root.children[0].media_type == "text/plain"


@given(
    root=st.text(alphabet=string.ascii_lowercase, min_size=1, max_size=16),
    resource=st.text(alphabet=string.ascii_lowercase, min_size=1, max_size=32),
)
def test_v301_property_related_root_keeps_only_the_declared_representation(  # type: ignore[misc]
    *, root: str, resource: str
) -> None:
    """A generated related root remains while every non-root resource is pruned."""
    root_id = root.encode("ascii") + b"@example"
    resource_id = b"resource-" + resource.encode("ascii") + b"@example"
    resource_body = b"resource-body-" + resource.encode("ascii")
    raw = (
        b'Content-Type: multipart/related; boundary=r; start="<'
        + root_id
        + b'>"\r\n\r\n--r\r\nContent-Type: text/html\r\nContent-ID: <'
        + root_id
        + b">\r\n\r\n<html>retained</html>\r\n--r\r\nContent-Type: image/png\r\n"
        + b"Content-ID: <"
        + resource_id
        + b">\r\n\r\n"
        + resource_body
        + b"\r\n--r--\r\n"
    )
    tree = parse_raw_mime(raw)
    plan = classify(tree.root)
    assert plan.removals == (
        Removal((1,), "image/png", RemovalReason.RELATED_NONROOT_COMPONENT),
    )
    candidate = build_candidate(tree, plan.removals).raw
    assert b"<html>retained</html>" in candidate
    assert resource_body not in candidate


@given(
    first=st.text(alphabet=string.ascii_lowercase, min_size=1, max_size=16),
    second=st.text(alphabet=string.ascii_lowercase, min_size=1, max_size=16),
)
def test_v301_property_rfc2231_continuations_are_contiguous_or_rejected(  # type: ignore[misc]
    *, first: str, second: str
) -> None:
    """Generated RFC 2231 continuation ownership cannot silently tolerate a gap."""
    initial = first.encode("ascii")
    continuation = second.encode("ascii")
    headers = (
        b"Content-Disposition: attachment; filename*0*=utf-8''"
        + initial
        + b"; filename*1*="
        + continuation
    )
    content_type = b"Content-Type: application/octet-stream\r\n"
    valid = content_type + headers + b"\r\n\r\nbody\r\n"
    parsed = parse_raw_mime(valid)
    assert parsed.root.disposition is not None
    assert parsed.root.disposition.parameters[b"filename"] == (
        b"utf-8''" + initial + continuation
    )
    gapped = headers.replace(b"filename*1*", b"filename*2*")
    with pytest.raises(AppError):
        parse_raw_mime(content_type + gapped + b"\r\n\r\nbody\r\n")


@given(
    st.lists(
        st.sampled_from((ItemStatus.FAILED, ItemStatus.NOT_RUN)), min_size=1, max_size=8
    )
)
def test_v301_property_batch_terminal_rows_remain_input_ordered(  # type: ignore[misc]
    statuses: list[ItemStatus],
) -> None:
    """Keep every synthetic batch row terminal and stable in original argument order."""
    ledger = BatchLedger.from_requests([
        path_value(f"synthetic-{index}.eml") for index in range(len(statuses))
    ])
    for item, status in zip(ledger.items, statuses, strict=True):
        item.finish(status, AppError(ExitCode.PARSE_ERROR, "synthetic"))
    assert [item.index for item in ledger.items] == list(range(len(statuses)))
    assert all(item.terminalized and item.status is not None for item in ledger.items)


@given(body=st.text(alphabet=string.ascii_lowercase, min_size=1, max_size=16))
def test_v301_property_batch_good_bad_good_and_existing_idempotence(  # type: ignore[misc]
    *, body: str
) -> None:
    """Keep processing peers after expected parsing failures and verify exact reuse."""
    case = hashlib.sha256(body.encode("ascii")).hexdigest()
    with TemporaryDirectory() as directory:
        root = Path(directory)
        first = root / f"{case}-first.eml"
        bad = root / f"{case}-bad.eml"
        third = root / f"{case}-third.eml"
        raw = b"Content-Type: text/plain\r\n\r\n" + body.encode("ascii") + b"\r\n"
        first.write_bytes(raw)
        bad.write_bytes(b"Content-Type: text/plain; charset=(broken\r\n\r\nbody\r\n")
        third.write_bytes(raw)
        dry = batch.execute(
            [str(first), str(bad), str(third)],
            batch.BatchOptions(
                dry_run=True,
                existing="verify",
                fail_fast=False,
                output=None,
                output_dir=None,
            ),
        )
        assert [item.status for item in dry.items] == [
            ItemStatus.WOULD_CREATE,
            ItemStatus.FAILED,
            ItemStatus.WOULD_CREATE,
        ]
        created = batch.execute(
            [str(first)],
            batch.BatchOptions(
                dry_run=False,
                existing="error",
                fail_fast=False,
                output=None,
                output_dir=None,
            ),
        )
        verified = batch.execute(
            [str(first)],
            batch.BatchOptions(
                dry_run=False,
                existing="verify",
                fail_fast=False,
                output=None,
                output_dir=None,
            ),
        )
        assert created.items[0].status is ItemStatus.CREATED
        assert verified.items[0].status is ItemStatus.EXISTING_VERIFIED


@given(st.text(alphabet=string.ascii_lowercase, min_size=1, max_size=24))
def test_v301_property_native_destination_keeps_the_requested_basename(  # type: ignore[misc]
    basename: str,
) -> None:
    """Derive a portable output name without normalizing the request's basename."""
    destination = default_destination(f"folder/{basename}.eml")
    assert destination.endswith(f"{basename}.mime-pruned.eml")


@given(candidate=st.text(alphabet="abcdef", min_size=1, max_size=32).map(_ascii_bytes))
def test_v301_property_no_replace_publication_keeps_one_regular_identity(  # type: ignore[misc]
    *, candidate: bytes
) -> None:
    """One bound destination can be published once and never silently replaced."""
    with TemporaryDirectory() as directory:
        destination = Path(directory) / hashlib.sha256(candidate).hexdigest()
        receipt = publish(bind_destination(str(destination)), candidate)
        assert receipt.identity is not None
        assert receipt.identity.is_regular()
        with pytest.raises(AppError) as conflict:
            publish(bind_destination(str(destination)), candidate)
        assert conflict.value.code is ExitCode.OUTPUT_CONFLICT


@given(st.binary(min_size=1, max_size=128))
def test_v301_property_terminal_receipts_release_active_payloads(  # type: ignore[misc]
    payload: bytes,
) -> None:
    """Terminal ledger items retain hashes and receipts, never active message bytes."""
    source = path_value("synthetic.eml")
    snapshot = SourceSnapshot(
        source,
        source,
        source,
        b"synthetic.eml",
        source,
        FileIdentity(1, 2, "-rw-------", 3),
        0o600,
        payload,
        hashlib.sha256(payload).hexdigest(),
        len(payload),
    )
    plan = TransformationPlan((), (), (), snapshot.digest, len(payload), payload)
    item = BatchLedger.from_requests([source]).items[0]
    item.source = snapshot
    item.transformation = plan
    item.finish(ItemStatus.WOULD_CREATE)
    assert item.source.raw == b""
    assert item.transformation.candidate == b""


class _TerminalLedgerMachine(RuleBasedStateMachine):
    """Explore one terminal ledger invariant through generated state transitions."""

    def __init__(self) -> None:
        super().__init__()
        self.ledger = BatchLedger.from_requests([path_value("synthetic.eml")])

    @rule(status=st.sampled_from((ItemStatus.FAILED, ItemStatus.NOT_RUN)))
    def terminalize_once(self, status: ItemStatus) -> None:  # type: ignore[misc]
        """Terminalize the preallocated synthetic item at most once."""
        item = self.ledger.items[0]
        if item.status is None:
            item.finish(status, AppError(ExitCode.PARSE_ERROR, "synthetic"))

    @invariant()
    def terminal_state_is_monotonic(self) -> None:
        """Require a terminal receipt to carry its matching public status flag."""
        item = self.ledger.items[0]
        assert item.terminalized is (item.status is not None)


TestV301TerminalLedgerMachine = _TerminalLedgerMachine.TestCase
