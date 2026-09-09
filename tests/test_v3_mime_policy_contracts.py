"""Exact action-plan contracts for the closed v3 MIME policy."""

from __future__ import annotations

from eml_attachment_remover.domain import DecisionAction, Removal, RemovalReason
from eml_attachment_remover.mime_policy import classify
from eml_attachment_remover.mime_raw import parse_raw_mime


def test_related_policy_exposes_exact_root_and_resource_actions() -> None:
    """Require related classification to retain only its selected body root."""
    raw = (
        b'Content-Type: multipart/related; boundary=r; start="<root@x>"\r\n\r\n'
        b"--r\r\nContent-Type: text/html\r\nContent-ID: <root@x>\r\n\r\nbody\r\n"
        b"--r\r\nContent-Type: image/png\r\nContent-ID: <image@x>\r\n\r\nbytes\r\n"
        b"--r--\r\n"
    )
    plan = classify(parse_raw_mime(raw).root)
    assert plan.actions == {
        (): DecisionAction.RECURSE,
        (0,): DecisionAction.KEEP,
        (1,): DecisionAction.REMOVE_SUBTREE,
    }
    assert plan.removals == (
        Removal((1,), "image/png", RemovalReason.RELATED_NONROOT_COMPONENT),
    )


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
