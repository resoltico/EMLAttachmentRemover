"""Smoke-test an installed wheel or source distribution through its CLI."""

from __future__ import annotations

import os
import shutil
import subprocess
import tempfile
from email import policy
from email.message import EmailMessage
from email.parser import BytesParser
from importlib.metadata import version as distribution_version
from pathlib import Path
from typing import Final

DISTRIBUTION_NAME: Final = "eml-attachment-remover"
COMMAND_NAME: Final = "remove-eml-attachments"
SUBPROCESS_TIMEOUT_SECONDS: Final = 30
EXPECTED_SUBJECT: Final = "Installed distribution smoke test"
EXPECTED_BODY: Final = "Public body"
SUBJECT_HEADER: Final = "Subject"
ATTACHMENT_MAIN_TYPE: Final = "application"
ATTACHMENT_SUBTYPE: Final = "octet-stream"
SOURCE_FILE_NAME: Final = "source.eml"
OUTPUT_FILE_NAME: Final = "output.eml"


def _fixture() -> bytes:
    """Return a public message with one removable attachment.

    Returns:
        Serialized synthetic EML bytes.

    """
    message = EmailMessage()
    message[SUBJECT_HEADER] = EXPECTED_SUBJECT
    message.set_content(EXPECTED_BODY)
    message.add_attachment(
        b"public attachment",
        maintype=ATTACHMENT_MAIN_TYPE,
        subtype=ATTACHMENT_SUBTYPE,
        filename="public.bin",
    )
    return message.as_bytes(policy=policy.SMTP)


def _environment() -> dict[str, str]:
    """Return a strict environment without source-tree import overrides.

    Returns:
        The isolated child-process environment.

    """
    environment = {
        name: value for name, value in os.environ.items() if name != "PYTHONPATH"
    }
    environment["PYTHONDEVMODE"] = "1"
    environment["PYTHONNOUSERSITE"] = "1"
    environment["PYTHONWARNINGS"] = "error"
    return environment


def _installed_command() -> str:
    """Resolve the console script created by the installed distribution.

    Returns:
        The absolute or executable-search-path command.

    Raises:
        RuntimeError: If packaging did not install the declared console script.

    """
    command = shutil.which(COMMAND_NAME)
    if command is None:
        message = f"installed distribution did not provide {COMMAND_NAME!r}"
        raise RuntimeError(message)
    return command


def _run_command(command: str, *arguments: str) -> subprocess.CompletedProcess[str]:
    """Run the installed command under bounded development-mode checks.

    Returns:
        The successful completed process.

    Raises:
        RuntimeError: If the console script exits unsuccessfully.

    """
    result = subprocess.run(
        [command, *arguments],
        env=_environment(),
        text=True,
        capture_output=True,
        check=False,
        timeout=SUBPROCESS_TIMEOUT_SECONDS,
    )
    if result.returncode != 0:
        message = (
            f"installed command failed with status {result.returncode}: "
            f"{result.stderr or result.stdout}"
        )
        raise RuntimeError(message)
    return result


def _verify_output(source: Path, destination: Path, original: bytes) -> None:
    """Verify source preservation and the generated EML's retained semantics.

    Raises:
        RuntimeError: If the command damaged the source or emitted unsafe output.

    """
    if source.read_bytes() != original:
        message = "installed command modified its source EML"
        raise RuntimeError(message)
    if not destination.is_file() or destination.stat().st_size == 0:
        message = "installed command did not create a non-empty output EML"
        raise RuntimeError(message)
    parsed = BytesParser(policy=policy.default).parsebytes(destination.read_bytes())
    body = parsed.get_body(preferencelist=("plain",))
    body_text = body.get_content().strip() if body is not None else None
    remaining_attachments = [
        part.get_filename()
        for part in parsed.walk()
        if part.get_content_disposition() == "attachment"
    ]
    defects = [defect for part in parsed.walk() for defect in part.defects]
    if (
        parsed[SUBJECT_HEADER] != EXPECTED_SUBJECT
        or body_text != EXPECTED_BODY
        or remaining_attachments
        or defects
    ):
        message = "installed command did not preserve the expected EML semantics"
        raise RuntimeError(message)


def main() -> int:
    """Exercise the installed console entry point and its generated EML.

    Returns:
        Zero when the installed artifact performs the documented operation.

    Raises:
        RuntimeError: If packaging metadata, the entry point, or output is invalid.

    """
    command = _installed_command()
    expected_version = distribution_version(DISTRIBUTION_NAME)
    version_result = _run_command(command, "--version")
    if version_result.stdout.strip() != f"{COMMAND_NAME} {expected_version}":
        message = "installed console-script version does not match package metadata"
        raise RuntimeError(message)
    with tempfile.TemporaryDirectory() as directory:
        source = Path(directory) / SOURCE_FILE_NAME
        destination = Path(directory) / OUTPUT_FILE_NAME
        original = _fixture()
        source.write_bytes(original)
        _run_command(command, "--output", str(destination), "--", str(source))
        _verify_output(source, destination, original)
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
