"""Public synthetic fixtures and subprocess helpers shared by regression tests."""

from __future__ import annotations

import hashlib
import io
import os
import subprocess
import sys
from email import policy
from email.message import EmailMessage
from email.parser import BytesParser
from pathlib import Path
from unittest.mock import patch

from eml_attachment_remover import cli

PROJECT_ROOT = Path(__file__).resolve().parents[1]
SOURCE_ROOT = PROJECT_ROOT / "src"
SUBPROCESS_TIMEOUT_SECONDS = 30
EXPECTED_NO_ARGUMENTS = """\
EML Attachment Remover

Create verified text-only EML working copies.
Retain a safe plain-text body. An equivalent local HTML alternative may
restore readable layout; resources and ordinary attachments are discarded.

Error: no source files were provided.

Usage:
  remove-eml-attachments [OPTIONS] <SOURCE>...

Example:
  remove-eml-attachments message.eml

Run 'remove-eml-attachments --help' to see all options.
"""


def subprocess_environment(*, source_tree: bool = False) -> dict[str, str]:
    """Return an isolated environment for strict Python subprocess tests.

    Returns:
        An environment that converts child warnings to errors, enables Python's
        development mode, and optionally imports the project from ``src``.

    """
    environment = os.environ.copy()
    environment["PYTHONDEVMODE"] = "1"
    environment["PYTHONNOUSERSITE"] = "1"
    environment["PYTHONWARNINGS"] = "error"
    if source_tree:
        environment["PYTHONPATH"] = str(SOURCE_ROOT)
    else:
        environment.pop("PYTHONPATH", None)
    return environment


def parse(path: Path) -> EmailMessage:
    """Parse one generated EML fixture with the default policy.

    Returns:
        The parsed public fixture.

    """
    return BytesParser(policy=policy.default).parsebytes(path.read_bytes())


def decoded_hash(part: EmailMessage) -> str:
    """Return a SHA-256 digest of one decoded leaf payload.

    Returns:
        The lower-case hexadecimal digest.

    """
    data = part.get_payload(decode=True)
    if not isinstance(data, bytes):
        payload = part.get_payload()
        data = (
            payload.encode("utf-8", "surrogateescape")
            if isinstance(payload, str)
            else b""
        )
    return hashlib.sha256(data).hexdigest()


def leaf_rows(
    message: EmailMessage,
) -> list[tuple[str, str | None, str | None, str | None, str]]:
    """Return comparable metadata and digests for every leaf.

    Returns:
        Content type, disposition, filename, ID, and digest for each leaf.

    """
    rows = []
    for part in message.walk():
        if part.is_multipart():
            continue
        rows.append((
            part.get_content_type(),
            part.get_content_disposition(),
            part.get_filename(),
            part.get("Content-ID"),
            decoded_hash(part),
        ))
    return rows


def run_cli(*args: str, cwd: Path | None = None) -> subprocess.CompletedProcess[str]:
    """Run the public module entry point with the local source package.

    Returns:
        The completed text-mode command result.

    """
    if cwd is not None:
        return subprocess.run(
            [
                sys.executable,
                "-X",
                "dev",
                "-W",
                "error",
                "-m",
                "eml_attachment_remover",
                *args,
            ],
            cwd=cwd,
            env=subprocess_environment(source_tree=True),
            text=True,
            capture_output=True,
            check=False,
            timeout=SUBPROCESS_TIMEOUT_SECONDS,
        )
    stdout = io.StringIO()
    stderr = io.StringIO()
    with patch.object(sys, "stdout", stdout), patch.object(sys, "stderr", stderr):
        returncode = cli.main(list(args))
    return subprocess.CompletedProcess(
        ["remove-eml-attachments", *args],
        returncode,
        stdout.getvalue(),
        stderr.getvalue(),
    )


def run_cli_bytes(*args: str) -> subprocess.CompletedProcess[bytes]:
    """Run the public module entry point with byte-oriented output.

    Returns:
        The completed byte-mode command result.

    """
    stdout = io.TextIOWrapper(io.BytesIO(), encoding="utf-8")
    stderr = io.TextIOWrapper(io.BytesIO(), encoding="utf-8")
    with patch.object(sys, "stdout", stdout), patch.object(sys, "stderr", stderr):
        returncode = cli.main(list(args))
    stdout.flush()
    stderr.flush()
    stdout_bytes = stdout.buffer.getvalue()
    stderr_bytes = stderr.buffer.getvalue()
    return subprocess.CompletedProcess(
        ["remove-eml-attachments", *args],
        returncode,
        stdout_bytes,
        stderr_bytes,
    )


def simple_message() -> EmailMessage:
    """Create a synthetic message with an inline resource and PDF attachment.

    Returns:
        A public MIME fixture suitable for attachment-removal tests.

    """
    msg = EmailMessage()
    msg["From"] = "sender@example.test"
    msg["To"] = "recipient@example.test"
    msg["Subject"] = "Synthetic message"
    msg.set_content("Plain body")
    msg.add_alternative(
        '<html><body>HTML body<img src="cid:signature@example.test"></body></html>',
        subtype="html",
    )
    payload = msg.get_payload()
    assert isinstance(payload, list)
    html = payload[1]
    assert isinstance(html, EmailMessage)
    html.add_related(
        b"PNG-SIGNATURE",
        maintype="image",
        subtype="png",
        cid="<signature@example.test>",
        filename="signature.png",
        disposition="inline",
    )
    msg.add_attachment(
        b"%PDF-attachment",
        maintype="application",
        subtype="pdf",
        filename="invoice.pdf",
    )
    return msg
