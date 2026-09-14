"""Generated v3.0.1 MIME and terminal-ledger exploration over synthetic inputs."""

from __future__ import annotations

import string

import pytest
from hypothesis import given
from hypothesis import strategies as st
from hypothesis.stateful import RuleBasedStateMachine, invariant, rule

from eml_attachment_remover.domain import (
    AppError,
    BatchLedger,
    ExitCode,
    ItemStatus,
    Removal,
    RemovalReason,
)
from eml_attachment_remover.mime_headers import line_end
from eml_attachment_remover.mime_raw import parse_raw_mime
from eml_attachment_remover.mime_verifier_policy import authorize_removals
from eml_attachment_remover.native_paths import path_value


@given(
    st.lists(st.sampled_from((b"\r\n", b"\n", b"\r", b"x")), min_size=1, max_size=32),
)
def test_v301_property_wire_terminators_keep_exact_bounded_spans(
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


@given(st.text(alphabet=string.ascii_lowercase, min_size=1, max_size=12))
def test_v301_property_structured_comments_preserve_content_type_semantics(
    label: str,
) -> None:
    """Permitted trailing comments do not change a synthetic Content-Type token."""
    raw = (
        b"Content-Type: text/plain; charset=us-ascii ("
        + label.encode("ascii")
        + b")\r\n\r\nbody\r\n"
    )
    assert parse_raw_mime(raw).root.media_type == "text/plain"


@given(
    st.sampled_from((
        RemovalReason.EXPLICIT_ATTACHMENT,
        RemovalReason.RELATED_NONROOT_COMPONENT,
    ))
)
def test_v301_property_verifier_rejects_each_wrong_attachment_claim_reason(
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


class _TerminalLedgerMachine(RuleBasedStateMachine):
    """Explore one terminal ledger invariant through generated state transitions."""

    def __init__(self) -> None:
        super().__init__()
        self.ledger = BatchLedger.from_requests([path_value("synthetic.eml")])

    @rule(status=st.sampled_from((ItemStatus.FAILED, ItemStatus.NOT_RUN)))
    def terminalize_once(self, status: ItemStatus) -> None:
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
