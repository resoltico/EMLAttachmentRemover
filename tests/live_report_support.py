"""Public synthetic emails for complete CLI and facade report-boundary tests."""

from __future__ import annotations

from typing import TYPE_CHECKING

if TYPE_CHECKING:
    from pathlib import Path

MESSAGE = (
    b"From: sender@example.test\r\nTo: receiver@example.test\r\n"
    b"Subject: public report fixture\r\nMIME-Version: 1.0\r\n"
    b"Content-Type: multipart/mixed; boundary=public-boundary\r\n\r\n"
    b"--public-boundary\r\nContent-Type: text/plain; charset=utf-8\r\n\r\n"
    b"public body\r\n--public-boundary\r\nContent-Type: application/octet-stream\r\n"
    b"Content-Disposition: attachment; filename=public.bin\r\n\r\n"
    b"PUBLIC-ATTACHMENT\r\n--public-boundary--\r\n"
)


def inputs(root: Path) -> list[str]:
    """Create two synthetic inputs.

    Returns:
        Their native argument spellings.

    """
    paths = [root / "first.eml", root / "second.eml"]
    for path in paths:
        path.write_bytes(MESSAGE)
    return [str(path) for path in paths]
