"""The intentionally small, breaking v3 command-line surface."""

from __future__ import annotations

import argparse
from typing import TYPE_CHECKING, Never, TypedDict, Unpack, override

from ._version import program_version
from .domain import PROGRAM_NAME, AppError, ExitCode

if TYPE_CHECKING:
    from collections.abc import Sequence

VERSION_HELP = "show program's version number and exit"


class Parser(argparse.ArgumentParser):
    """Raise a typed usage error instead of exiting from a reusable CLI boundary."""

    @override
    def error(self, message: str) -> Never:
        """Raise the parser's stable usage failure.

        Raises:
            AppError: Always, with the v3 usage exit code.

        """
        raise AppError(ExitCode.USAGE, message)


class _ActionKeywords(TypedDict, total=False):
    """The keyword argparse forwards to this option's action."""

    help: str | None


class _LazyVersion(argparse.Action):
    """Print ``PROG VERSION`` and exit, resolving the version only when asked.

    argparse's own version action needs the text when the parser is built, which made
    every run pay for the packaging-metadata lookup. Here the lookup happens when
    ``--version`` is used, through a throwaway parser that owns the printing and the
    exit, so the output and status are argparse's own.
    """

    def __init__(
        self,
        option_strings: Sequence[str],
        dest: str = argparse.SUPPRESS,
        default: str = argparse.SUPPRESS,
        **keywords: Unpack[_ActionKeywords],
    ) -> None:
        """Declare an option that takes no argument and stores nothing."""
        super().__init__(
            option_strings=option_strings,
            dest=dest,
            default=default,
            nargs=0,
            **keywords,
        )

    @override
    def __call__(
        self,
        parser: argparse.ArgumentParser,
        namespace: argparse.Namespace,
        values: str | Sequence[object] | None,
        option_string: str | None = None,
    ) -> None:
        """Print the version through argparse and exit successfully."""
        delegate = argparse.ArgumentParser(prog=parser.prog, add_help=False)
        delegate.add_argument(
            "--version",
            action="version",
            version=f"%(prog)s {program_version()}",
        )
        delegate.parse_args(["--version"])


def build_parser() -> Parser:
    """Construct help that states the MIME-pruned security boundary.

    Returns:
        The fully configured breaking-v3 command-line parser.

    """
    parser = Parser(
        prog=PROGRAM_NAME,
        allow_abbrev=False,
        description=(
            "Create structurally verified MIME-pruned EML working copies. "
            "Explicit MIME attachments and non-root multipart/related components are "
            "removed; retained plain and HTML bodies are preserved as original bytes. "
            "Retained HTML is not sanitized and may contain unresolved references."
        ),
    )
    parser.add_argument("source", nargs="+", help="one or more source EML files")
    destination = parser.add_mutually_exclusive_group()
    destination.add_argument("-o", "--output", help="destination; valid for one source")
    destination.add_argument(
        "--output-dir", help="destination directory for all sources"
    )
    parser.add_argument(
        "--existing",
        choices=("error", "verify"),
        default="error",
        help="existing output policy (default: error)",
    )
    parser.add_argument(
        "--dry-run", action="store_true", help="build and verify without writing"
    )
    parser.add_argument(
        "--fail-fast", action="store_true", help="stop at first expected failure"
    )
    parser.add_argument(
        "--output-format",
        choices=("human", "json", "paths0"),
        default="human",
        help="report format (default: human)",
    )
    parser.add_argument("--version", action=_LazyVersion, help=VERSION_HELP)
    return parser


def validate_arguments(namespace: argparse.Namespace) -> None:
    """Validate combinations whose safety cannot be represented by argparse alone.

    Raises:
        AppError: If one-source output or dry-run path-channel rules are violated.

    """
    if namespace.output is not None and len(namespace.source) != 1:
        raise AppError(
            ExitCode.USAGE, "--output may be used only with exactly one source"
        )
    if namespace.dry_run and namespace.output_format == "paths0":
        raise AppError(
            ExitCode.USAGE, "--output-format=paths0 cannot be used with --dry-run"
        )


def raw_json_requested(arguments: list[str]) -> bool:
    """Return whether pre-parse options selected the JSON automation channel.

    Returns:
        Whether JSON is selected before an end-of-options marker.

    """
    options = _raw_options(arguments)
    return any(
        option == "--output-format=json"
        or (
            option == "--output-format"
            and index + 1 < len(options)
            and options[index + 1] == "json"
        )
        for index, option in enumerate(options)
    )


def raw_source_candidates(arguments: list[str]) -> list[str]:
    """Conservatively retain raw source candidates for a pre-parse JSON failure.

    Returns:
        Non-option arguments except values consumed by known value-taking options.

    """
    candidates: list[str] = []
    options = _raw_options(arguments)
    consumed = _consumed_option_values(options)
    candidates.extend(
        argument
        for index, argument in enumerate(options)
        if index not in consumed and not argument.startswith("-")
    )
    if "--" in arguments:
        candidates.extend(arguments[arguments.index("--") + 1 :])
    return candidates


def _raw_options(arguments: list[str]) -> list[str]:
    return arguments[: arguments.index("--")] if "--" in arguments else arguments


def _consumed_option_values(arguments: list[str]) -> set[int]:
    value_options = {"-o", "--output", "--output-dir", "--existing", "--output-format"}
    return {
        index + 1
        for index, argument in enumerate(arguments[:-1])
        if argument in value_options
    }
