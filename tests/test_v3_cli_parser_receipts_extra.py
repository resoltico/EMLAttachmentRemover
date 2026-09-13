"""Complete public-parser receipts that resist surface-level CLI regressions."""

from __future__ import annotations

import argparse

import pytest

from eml_attachment_remover.cli_parser import build_parser
from eml_attachment_remover.domain import AppError, ExitCode


def test_parser_help_and_version_are_complete_public_documents(
    capsys: pytest.CaptureFixture[str],
) -> None:
    """Preserve the complete introductory and machine-discoverable CLI documents."""
    parser = build_parser()
    expected_help = """usage: remove-eml-attachments [-h] [-o OUTPUT | \
--output-dir OUTPUT_DIR]
                              [--existing {error,verify}] [--dry-run]
                              [--fail-fast]
                              [--output-format {human,json,paths0}]
                              [--version]
                              source [source ...]

Create structurally verified MIME-pruned EML working copies. Explicit MIME
attachments and non-root multipart/related components are removed; retained
plain and HTML bodies are preserved as original bytes. Retained HTML is not
sanitized and may contain unresolved references.

positional arguments:
  source                one or more source EML files

options:
  -h, --help            show this help message and exit
  -o, --output OUTPUT   destination; valid for one source
  --output-dir OUTPUT_DIR
                        destination directory for all sources
  --existing {error,verify}
                        existing output policy (default: error)
  --dry-run             build and verify without writing
  --fail-fast           stop at first expected failure
  --output-format {human,json,paths0}
                        report format (default: human)
  --version             show program's version number and exit
"""
    assert parser.format_help() == expected_help
    with pytest.raises(SystemExit) as help_exit:
        parser.parse_args(["--help"])
    assert help_exit.value.code == 0
    help_output = capsys.readouterr()
    assert (help_output.out, help_output.err) == (expected_help, "")
    with pytest.raises(SystemExit) as version_exit:
        parser.parse_args(["--version"])
    assert version_exit.value.code == 0
    version_output = capsys.readouterr()
    assert (version_output.out, version_output.err) == (
        "remove-eml-attachments 3.0.0\n",
        "",
    )


def test_every_parser_action_and_exclusive_group_has_an_exact_receipt() -> None:
    """Assert all argparse metadata, including the mutually-exclusive ownership."""
    parser = build_parser()
    actions = parser._actions  # ruff: ignore[private-member-access] - action receipts are public CLI behavior.
    assert [
        (
            action.dest,
            type(action).__name__,
            action.option_strings,
            action.nargs,
            action.const,
            action.default,
            action.type,
            action.choices,
            action.required,
            action.help,
            action.metavar,
            getattr(action, "version", None),
        )
        for action in actions
    ] == [
        (
            "help",
            "_HelpAction",
            ["-h", "--help"],
            0,
            None,
            argparse.SUPPRESS,
            None,
            None,
            False,
            "show this help message and exit",
            None,
            None,
        ),
        (
            "source",
            "_StoreAction",
            [],
            "+",
            None,
            None,
            None,
            None,
            True,
            "one or more source EML files",
            None,
            None,
        ),
        (
            "output",
            "_StoreAction",
            ["-o", "--output"],
            None,
            None,
            None,
            None,
            None,
            False,
            "destination; valid for one source",
            None,
            None,
        ),
        (
            "output_dir",
            "_StoreAction",
            ["--output-dir"],
            None,
            None,
            None,
            None,
            None,
            False,
            "destination directory for all sources",
            None,
            None,
        ),
        (
            "existing",
            "_StoreAction",
            ["--existing"],
            None,
            None,
            "error",
            None,
            ("error", "verify"),
            False,
            "existing output policy (default: error)",
            None,
            None,
        ),
        (
            "dry_run",
            "_StoreTrueAction",
            ["--dry-run"],
            0,
            True,
            False,
            None,
            None,
            False,
            "build and verify without writing",
            None,
            None,
        ),
        (
            "fail_fast",
            "_StoreTrueAction",
            ["--fail-fast"],
            0,
            True,
            False,
            None,
            None,
            False,
            "stop at first expected failure",
            None,
            None,
        ),
        (
            "output_format",
            "_StoreAction",
            ["--output-format"],
            None,
            None,
            "human",
            None,
            ("human", "json", "paths0"),
            False,
            "report format (default: human)",
            None,
            None,
        ),
        (
            "version",
            "_VersionAction",
            ["--version"],
            0,
            None,
            argparse.SUPPRESS,
            None,
            None,
            False,
            "show program's version number and exit",
            None,
            "%(prog)s 3.0.0",
        ),
    ]
    groups = parser._mutually_exclusive_groups  # ruff: ignore[private-member-access] - group topology is public CLI behavior.
    group_actions = [
        (group.required, [action.dest for action in group._group_actions])  # ruff: ignore[private-member-access] - group topology is public CLI behavior.
        for group in groups
    ]
    assert group_actions == [(False, ["output", "output_dir"])]


@pytest.mark.parametrize(
    ("arguments", "expected"),
    [
        (
            ["one.eml"],
            {
                "source": ["one.eml"],
                "output": None,
                "output_dir": None,
                "existing": "error",
                "dry_run": False,
                "fail_fast": False,
                "output_format": "human",
            },
        ),
        (
            ["--output=copy.eml", "--existing", "error", "one.eml"],
            {
                "source": ["one.eml"],
                "output": "copy.eml",
                "output_dir": None,
                "existing": "error",
                "dry_run": False,
                "fail_fast": False,
                "output_format": "human",
            },
        ),
        (
            ["--output-dir=copies", "--output-format", "paths0", "one.eml", "two.eml"],
            {
                "source": ["one.eml", "two.eml"],
                "output": None,
                "output_dir": "copies",
                "existing": "error",
                "dry_run": False,
                "fail_fast": False,
                "output_format": "paths0",
            },
        ),
        (
            ["--", "-literal-source.eml"],
            {
                "source": ["-literal-source.eml"],
                "output": None,
                "output_dir": None,
                "existing": "error",
                "dry_run": False,
                "fail_fast": False,
                "output_format": "human",
            },
        ),
    ],
)
def test_parser_receives_each_value_form_and_end_of_options(
    arguments: list[str], expected: dict[str, object]
) -> None:
    """Preserve option ownership, defaults, and literal positional boundary."""
    assert vars(build_parser().parse_args(arguments)) == expected


@pytest.mark.parametrize(
    ("arguments", "message"),
    [
        (["--output"], "argument -o/--output: expected one argument"),
        (["--output-dir"], "argument --output-dir: expected one argument"),
        (["--existing"], "argument --existing: expected one argument"),
        (["--output-format"], "argument --output-format: expected one argument"),
        (["--dry-run"], "the following arguments are required: source"),
        (["--output", "copy.eml"], "the following arguments are required: source"),
        (
            ["--existing="],
            "argument --existing: invalid choice: '' (choose from 'error', 'verify')",
        ),
        (
            ["--output-format="],
            (
                "argument --output-format: invalid choice: '' "
                "(choose from 'human', 'json', 'paths0')"
            ),
        ),
        (
            ["--output-dir", "copies", "-o", "copy.eml", "source.eml"],
            "argument -o/--output: not allowed with argument --output-dir",
        ),
    ],
)
def test_parser_rejects_every_missing_value_and_conflict_without_stderr(
    arguments: list[str], message: str, capsys: pytest.CaptureFixture[str]
) -> None:
    """Keep parser failures typed, exact, and silent for the runtime layer."""
    with pytest.raises(AppError) as captured:
        build_parser().parse_args(arguments)
    assert (captured.value.code, captured.value.message) == (ExitCode.USAGE, message)
    assert (capsys.readouterr().out, capsys.readouterr().err) == ("", "")
