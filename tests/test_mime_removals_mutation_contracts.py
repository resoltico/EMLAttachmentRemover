"""Focused behavioral mutation contracts for MIME removal traversal."""

from __future__ import annotations

from eml_attachment_remover.mime_policy import classify
from eml_attachment_remover.mime_raw import parse_raw_mime
from eml_attachment_remover.mime_removals import RemovalIndex


def test_removal_prefix_index_records_every_changed_ancestor_branch() -> None:
    """Independent removed branches each dirty their retained ancestry."""
    raw = (
        b"Content-Type: multipart/mixed; boundary=outer\r\n\r\n"
        b"--outer\r\nContent-Type: multipart/mixed; boundary=left\r\n\r\n"
        b"--left\r\nContent-Type: text/plain\r\n\r\nleft\r\n"
        b"--left\r\nContent-Type: application/octet-stream\r\n"
        b"Content-Disposition: attachment\r\n\r\nremove left\r\n--left--\r\n"
        b"--outer\r\nContent-Type: multipart/mixed; boundary=right\r\n\r\n"
        b"--right\r\nContent-Type: text/plain\r\n\r\nright\r\n"
        b"--right\r\nContent-Type: application/octet-stream\r\n"
        b"Content-Disposition: attachment\r\n\r\nremove right\r\n--right--\r\n"
        b"--outer--\r\n"
    )
    tree = parse_raw_mime(raw)
    index = RemovalIndex.from_roots({
        removal.path for removal in classify(tree.root).removals
    })
    assert index.changed_ancestor_paths(tree.root) == {(), (0,), (1,)}
