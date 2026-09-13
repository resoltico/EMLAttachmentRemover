"""Complete observable-policy receipts for MIME-pruned v3 messages."""

from __future__ import annotations

import pytest

from eml_attachment_remover.domain import (
    AppError,
    DecisionAction,
    ExitCode,
    Removal,
    RemovalReason,
)
from eml_attachment_remover.mime_policy import classify
from eml_attachment_remover.mime_raw import RawNode, parse_raw_mime
from eml_attachment_remover.mime_validation import ContentSpec


def _plan(
    raw: bytes,
) -> tuple[dict[tuple[int, ...], DecisionAction], tuple[Removal, ...]]:
    """Return the complete public policy receipt for one wire message.

    Returns:
        The complete decision map and ordered deletion roots.

    """
    policy = classify(parse_raw_mime(raw).root)
    return policy.actions, policy.removals


def test_text_body_roles_have_exact_retention_receipts() -> None:
    """Keep supported text when, and only when, its body role is explicit enough."""
    plain_actions, plain_removals = _plan(
        b"Content-Type: text/plain\r\n\r\nplain body\r\n"
    )
    inline_actions, inline_removals = _plan(
        b"Content-Type: text/html; name=body.html\r\n"
        b"Content-Disposition: inline; filename=body.html\r\n\r\n<html/>\r\n"
    )
    assert plain_actions == {(): DecisionAction.KEEP}
    assert plain_removals == ()
    assert inline_actions == {(): DecisionAction.KEEP}
    assert inline_removals == ()


@pytest.mark.parametrize(
    ("raw", "message", "path"),
    [
        (
            b"Content-Type: text/plain; name=body.txt\r\n\r\nbody\r\n",
            "filename-only body role is ambiguous",
            (),
        ),
        (
            (
                b"Content-Type: text/html\r\n"
                b"Content-Disposition: attachment\r\n\r\nbody\r\n"
            ),
            "attachment body role conflict",
            (),
        ),
        (
            (
                b"Content-Type: text/plain\r\n"
                b"Content-Disposition: form-data\r\n\r\nbody\r\n"
            ),
            "unknown MIME disposition",
            (),
        ),
        (
            b"Content-Type: application/json\r\n\r\n{}\r\n",
            "unsupported body MIME type application/json",
            (),
        ),
        (
            b"Content-Type: application/pkcs7-signature\r\n\r\nsignature\r\n",
            "protected MIME content",
            (),
        ),
    ],
)
def test_leaf_role_rejections_have_exact_safe_receipts(
    raw: bytes, message: str, path: tuple[int, ...]
) -> None:
    """Make every leaf-role guard independently observable to callers."""
    with pytest.raises(AppError) as raised:
        classify(parse_raw_mime(raw).root)
    assert raised.value.code is ExitCode.TRANSFORMATION_UNAVAILABLE
    assert raised.value.message == message
    assert raised.value.mime_path == path


def test_alternative_preserves_every_supported_representation_and_nested_plan() -> None:
    """Expose all decisions when alternatives contain a related representation."""
    actions, removals = _plan(
        b"Content-Type: multipart/alternative; boundary=outer\r\n\r\n"
        b"--outer\r\nContent-Type: text/plain\r\n\r\nplain\r\n"
        b"--outer\r\nContent-Type: multipart/related; boundary=related; "
        b'start="<html@x>"\r\n\r\n'
        b"--related\r\nContent-Type: image/png\r\n"
        b"Content-ID: <image@x>\r\n\r\nimage\r\n"
        b"--related\r\nContent-Type: text/html\r\n"
        b"Content-ID: <html@x>\r\n\r\n<html/>\r\n"
        b"--related--\r\n"
        b"--outer\r\nContent-Type: multipart/alternative; boundary=inner\r\n\r\n"
        b"--inner\r\nContent-Type: text/plain\r\n\r\ninner plain\r\n"
        b"--inner\r\nContent-Type: text/html\r\n\r\ninner html\r\n"
        b"--inner--\r\n--outer--\r\n"
    )
    assert actions == {
        (): DecisionAction.RECURSE,
        (0,): DecisionAction.KEEP,
        (1,): DecisionAction.RECURSE,
        (1, 0): DecisionAction.REMOVE_SUBTREE,
        (1, 1): DecisionAction.KEEP,
        (2,): DecisionAction.RECURSE,
        (2, 0): DecisionAction.KEEP,
        (2, 1): DecisionAction.KEEP,
    }
    assert removals == (
        Removal((1, 0), "image/png", RemovalReason.RELATED_NONROOT_COMPONENT),
    )


@pytest.mark.parametrize(
    ("raw", "message", "path"),
    [
        (
            (
                b"Content-Type: multipart/alternative; boundary=a\r\n\r\n"
                b"--a\r\nContent-Type: multipart/mixed; boundary=m\r\n\r\n"
                b"--m\r\nContent-Type: text/plain\r\n\r\nbody\r\n--m--\r\n"
                b"--a--\r\n"
            ),
            "unsupported multipart/alternative branch",
            (0,),
        ),
        (
            (
                b"Content-Type: multipart/alternative; boundary=a\r\n"
                b"Content-Disposition: attachment\r\n\r\n"
                b"--a\r\nContent-Type: text/plain\r\n\r\nbody\r\n--a--\r\n"
            ),
            "attachment alternative role conflict",
            (),
        ),
    ],
)
def test_alternative_rejections_name_the_exact_failed_role(
    raw: bytes, message: str, path: tuple[int, ...]
) -> None:
    """Reject attachment-role and unsupported alternative branches."""
    with pytest.raises(AppError) as raised:
        classify(parse_raw_mime(raw).root)
    assert raised.value.code is ExitCode.TRANSFORMATION_UNAVAILABLE
    assert raised.value.message == message
    assert raised.value.mime_path == path


def test_related_selects_declared_root_and_prunes_both_nonroot_sides() -> None:
    """Prove selection is by exact start ID rather than first-child position."""
    actions, removals = _plan(
        b'Content-Type: multipart/related; boundary=r; type="TEXT/HTML"; '
        b'start="<root@x>"; start-info="<root@x>"\r\n\r\n'
        b"--r\r\nContent-Type: image/png\r\n"
        b"Content-ID: <before@x>\r\n\r\nbefore\r\n"
        b"--r\r\nContent-Type: text/html\r\n"
        b"Content-ID: <root@x>\r\n\r\n<html/>\r\n"
        b"--r\r\nContent-Type: image/gif\r\n"
        b"Content-ID: <after@x>\r\n\r\nafter\r\n--r--\r\n"
    )
    assert actions == {
        (): DecisionAction.RECURSE,
        (0,): DecisionAction.REMOVE_SUBTREE,
        (1,): DecisionAction.KEEP,
        (2,): DecisionAction.REMOVE_SUBTREE,
    }
    assert removals == (
        Removal((0,), "image/png", RemovalReason.RELATED_NONROOT_COMPONENT),
        Removal((2,), "image/gif", RemovalReason.RELATED_NONROOT_COMPONENT),
    )


@pytest.mark.parametrize(
    ("raw", "message", "path"),
    [
        (
            (
                b'Content-Type: multipart/related; boundary=r; type="text/html"\r\n\r\n'
                b"--r\r\nContent-Type: text/plain\r\n\r\nbody\r\n--r--\r\n"
            ),
            "related type does not match root",
            (),
        ),
        (
            (
                b"Content-Type: multipart/related; boundary=r; "
                b'start-info="<resource@x>"\r\n\r\n'
                b"--r\r\nContent-Type: text/plain\r\n"
                b"Content-ID: <root@x>\r\n\r\nbody\r\n"
                b"--r\r\nContent-Type: image/png\r\n"
                b"Content-ID: <resource@x>\r\n\r\nbytes\r\n--r--\r\n"
            ),
            "related start-info does not target a retained component",
            (),
        ),
    ],
)
def test_related_metadata_rejections_are_precise(
    raw: bytes, message: str, path: tuple[int, ...]
) -> None:
    """Protect the declared related root and its retained-only metadata targets."""
    with pytest.raises(AppError) as raised:
        classify(parse_raw_mime(raw).root)
    assert raised.value.code is ExitCode.TRANSFORMATION_UNAVAILABLE
    assert raised.value.message == message
    assert raised.value.mime_path == path


def test_direct_policy_guards_reach_empty_alternative_and_missing_start() -> None:
    """Make policy-only guards observable independently of parser preconditions."""
    text = ContentSpec("text/plain", {})
    empty_alternative = RawNode(
        (), 0, 0, 0, (), ContentSpec("multipart/alternative", {}), None, "7bit"
    )
    child = RawNode((0,), 0, 0, 0, (), text, None, "7bit")
    missing_start = RawNode(
        (),
        0,
        0,
        0,
        (),
        ContentSpec("multipart/related", {b"start": b"<missing@x>"}),
        None,
        "7bit",
        [child],
    )
    for root, message in (
        (empty_alternative, "empty multipart/alternative"),
        (missing_start, "related start has no direct root"),
    ):
        with pytest.raises(AppError) as raised:
            classify(root)
        assert raised.value.code is ExitCode.TRANSFORMATION_UNAVAILABLE
        assert raised.value.message == message
        assert raised.value.mime_path == ()


def test_mixed_only_prunes_explicit_attachment_children() -> None:
    """Give mixed messages a full direct-child action and removal receipt."""
    actions, removals = _plan(
        b"Content-Type: multipart/mixed; boundary=m\r\n\r\n"
        b"--m\r\nContent-Type: text/plain\r\n\r\nbody\r\n"
        b"--m\r\nContent-Type: text/html; name=body.html\r\n"
        b"Content-Disposition: inline; filename=body.html\r\n\r\n<html/>\r\n"
        b"--m\r\nContent-Type: text/plain\r\n"
        b"Content-Disposition: attachment\r\n\r\nattached text\r\n"
        b"--m\r\nContent-Type: application/octet-stream\r\n"
        b"Content-Disposition: attachment; filename=data.bin\r\n\r\nbytes\r\n--m--\r\n"
    )
    assert actions == {
        (): DecisionAction.RECURSE,
        (0,): DecisionAction.KEEP,
        (1,): DecisionAction.KEEP,
        (2,): DecisionAction.REMOVE_SUBTREE,
        (3,): DecisionAction.REMOVE_SUBTREE,
    }
    assert removals == (
        Removal((2,), "text/plain", RemovalReason.EXPLICIT_ATTACHMENT),
        Removal((3,), "application/octet-stream", RemovalReason.EXPLICIT_ATTACHMENT),
    )
