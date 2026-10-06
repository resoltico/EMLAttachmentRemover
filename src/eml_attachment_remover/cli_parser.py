"""The intentionally small, supported command-line surface."""

from __future__ import annotations

import argparse
import re
from typing import TYPE_CHECKING, Never, TypedDict, Unpack, overload, override

from ._version import program_version
from .domain import PROGRAM_NAME, AppError, ExitCode

if TYPE_CHECKING:
    from collections.abc import Iterable, Sequence

VERSION_HELP = "show program's version number and exit"
NEGATIVE_NUMBER = re.compile(r"^-\d+$|^-\d*\.\d+$")


class Parser(argparse.ArgumentParser):
    """Raise a typed usage error instead of exiting from a reusable CLI boundary."""

    @overload
    def parse_args(
        self, args: Iterable[str] | None = None, namespace: None = None
    ) -> argparse.Namespace: ...

    @overload
    def parse_args[Parsed](
        self, args: Iterable[str] | None = None, namespace: Parsed = ...
    ) -> Parsed: ...

    @override
    def parse_args(
        self,
        args: Iterable[str] | None = None,
        namespace: object | None = None,
    ) -> object:
        """Require exactly one source channel after argparse resolves all options.

        Returns:
            The complete namespace, preserving supplied namespace subclasses.

        Raises:
            AppError: If neither source channel or both source channels were selected.

        """
        parsed = super().parse_args(args, namespace)
        requested = getattr(parsed, "request_stdin", False)
        selected = getattr(parsed, "source", None)
        if requested and selected:
            raise AppError(
                ExitCode.USAGE, "request stdin cannot be combined with source arguments"
            )
        if not selected and not requested:
            raise AppError(
                ExitCode.USAGE, "the following arguments are required: source"
            )
        return parsed

    @override
    def error(self, message: str) -> Never:
        """Raise the parser's stable usage failure.

        Raises:
            AppError: Always, with the usage exit code.

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
        The fully configured supported command-line parser.

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
    parser.add_argument("source", nargs="*", help="one or more source EML files")
    parser.add_argument(
        "--progress-fd",
        type=int,
        help="write advisory schema-1 progress lines to a pipe descriptor above stdio",
    )
    parser.add_argument(
        "--request-stdin",
        action="store_true",
        help=(
            "read a framed native-path request; "
            "keep its stdin pipe open until completion"
        ),
    )
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
    if not namespace.source:
        raise AppError(ExitCode.USAGE, "the following arguments are required: source")
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
        Whether the last effective known format selects JSON before ``--``.

    """
    options = _raw_options(arguments)
    consumed = _consumed_option_values(options)
    selected = "human"
    for index, option in enumerate(options):
        if index in consumed:
            continue
        if option.startswith("--output-format="):
            value = option.partition("=")[2]
        elif option == "--output-format" and index + 1 < len(options):
            value = options[index + 1]
        else:
            continue
        if value in {"human", "json", "paths0"}:
            selected = value
    return selected == "json"


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


def raw_dry_run_requested(arguments: list[str]) -> bool:
    """Infer dry-run only from an option before the end-of-options boundary.

    Returns:
        Whether raw options contain a dry-run flag rather than an option value.

    """
    options = _raw_options(arguments)
    consumed = _consumed_option_values(options)
    return any(
        value == "--dry-run" and index not in consumed
        for index, value in enumerate(options)
    )


def _raw_options(arguments: list[str]) -> list[str]:
    return arguments[: arguments.index("--")] if "--" in arguments else arguments


def _consumed_option_values(arguments: list[str]) -> set[int]:
    value_options = {"-o", "--output", "--output-dir", "--existing", "--output-format"}
    return {
        index + 1
        for index, argument in enumerate(arguments[:-1])
        if argument in value_options
        and (
            not arguments[index + 1].startswith("-")
            or arguments[index + 1] == "-"
            or NEGATIVE_NUMBER.fullmatch(arguments[index + 1]) is not None
        )
    }
