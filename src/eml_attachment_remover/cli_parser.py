"""Define command-line syntax and argument-combination validation."""

from __future__ import annotations

from pathlib import Path
from typing import Final

from ._version import PROGRAM_VERSION
from .models import PROGRAM_NAME, ArgumentParser, CliError, ExitCode, OutputFormat

APPLICATION_DESCRIPTION: Final = (
    "Remove ordinary file attachments from EML messages while preserving "
    "inline images and other message-body resources."
)
NO_ARGUMENTS_LINES: Final = (
    "EML Attachment Remover",
    "",
    "Remove ordinary file attachments from EML messages while preserving",
    "inline images and other message-body resources.",
    "",
    "Error: no source files were provided.",
    "",
    "Usage:",
    f"  {PROGRAM_NAME} [OPTIONS] <SOURCE>...",
    "",
    "Example:",
    f"  {PROGRAM_NAME} message.eml",
    "",
    f"Run '{PROGRAM_NAME} --help' to see all options.",
)


def build_parser() -> ArgumentParser:
    """Create the command-line parser.

    Returns:
        The configured argument parser.

    """
    parser = ArgumentParser(
        prog=PROGRAM_NAME,
        allow_abbrev=False,
        description=APPLICATION_DESCRIPTION,
        epilog=(
            "Exit codes: 0 success; 2 usage; 3 input; 4 output conflict; "
            "5 MIME parse; 6 signed/protected content; 7 write; "
            "8 verification; 9 partial batch failure; 70 internal; "
            "130 interrupted. For a filename beginning with '-', place '--' "
            "before the filenames."
        ),
    )
    parser.add_argument(
        "source",
        nargs="+",
        type=Path,
        help="one or more source EML files",
    )
    destination_group = parser.add_mutually_exclusive_group()
    destination_group.add_argument(
        "-o",
        "--output",
        type=Path,
        help="destination EML; valid only with one source",
    )
    destination_group.add_argument(
        "--output-dir",
        type=Path,
        help="existing directory for all generated EML files",
    )
    existing_group = parser.add_mutually_exclusive_group()
    existing_group.add_argument(
        "-f",
        "--force",
        action="store_true",
        help="replace existing destinations, never any selected source file",
    )
    existing_group.add_argument(
        "--skip-existing",
        action="store_true",
        help="leave existing destination files unchanged and report them as skipped",
    )
    parser.add_argument(
        "--dry-run",
        action="store_true",
        help="report what would be removed without writing output files",
    )
    parser.add_argument(
        "--fail-fast",
        action="store_true",
        help="stop at the first failed input instead of completing the batch",
    )
    parser.add_argument(
        "--output-format",
        choices=tuple(OutputFormat),
        default=OutputFormat.HUMAN,
        type=OutputFormat,
        help=(
            "report format: human, json, newline-delimited paths, or NUL-delimited "
            "paths0 (default: human)"
        ),
    )
    parser.add_argument(
        "--version",
        action="version",
        version=f"%(prog)s {PROGRAM_VERSION}",
    )
    return parser


def validate_cli_arguments(
    sources: list[Path],
    explicit_output: Path | None,
    output_format: OutputFormat,
    *,
    dry_run: bool,
) -> None:
    """Validate combinations that ``argparse`` cannot express directly.

    Raises:
        CliError: If a requested combination is ambiguous or contradictory.

    """
    if explicit_output is not None and len(sources) != 1:
        raise CliError(
            ExitCode.USAGE,
            "--output may be used only when exactly one source file is supplied",
        )
    if dry_run and output_format in {OutputFormat.PATHS, OutputFormat.PATHS0}:
        raise CliError(
            ExitCode.USAGE,
            "path-only output formats cannot be combined with --dry-run",
        )


def json_output_requested(arguments: list[str]) -> bool:
    """Return whether raw arguments request JSON output.

    Returns:
        ``True`` for either accepted spelling of the JSON output option.

    """
    requested = False
    argument_iterator = iter(arguments)
    for argument in argument_iterator:
        if argument == "--":
            break
        if argument.startswith("--output-format="):
            requested = argument.partition("=")[2] == "json"
        elif argument == "--output-format":
            value = next(argument_iterator, None)
            if value == "--":
                break
            requested = value == "json"
    return requested
