"""The version is resolved once, when something asks, and never at startup."""

from __future__ import annotations

import importlib
import subprocess
import sys
from typing import TYPE_CHECKING

import pytest

import eml_attachment_remover
from eml_attachment_remover import cli, cli_parser

if TYPE_CHECKING:
    from collections.abc import Iterator
    from pathlib import Path

METADATA_LOADED = "'importlib.metadata' in sys.modules"
_version = importlib.import_module("eml_attachment_remover._version")


def _run_python(code: str, *arguments: str) -> subprocess.CompletedProcess[str]:
    """Run a fresh interpreter, so module loading starts from nothing.

    Returns:
        The completed process with decoded output.

    """
    return subprocess.run(
        [sys.executable, "-c", code, *arguments],
        check=False,
        capture_output=True,
        text=True,
    )


@pytest.fixture(autouse=True)  # ruff: ignore[pytest-fixture-autouse] - no test may see another's cached version.
def _fresh_version_cache() -> Iterator[None]:
    """Keep one test's patched lookup from leaking into another's cached answer."""
    _version.program_version.cache_clear()
    yield
    _version.program_version.cache_clear()


def test_importing_the_command_does_not_load_packaging_metadata() -> None:
    """The metadata machinery costs more than the rest of startup, so it stays out."""
    result = _run_python(
        f"import sys, eml_attachment_remover.runtime\nprint({METADATA_LOADED})"
    )
    assert result.returncode == 0, result.stderr
    assert result.stdout.strip() == "False"


def test_a_human_report_run_does_not_load_packaging_metadata(tmp_path: Path) -> None:
    """A run that never prints a version never resolves one."""
    source = tmp_path / "message.eml"
    source.write_bytes(b"Content-Type: text/plain\r\n\r\nbody\r\n")
    result = _run_python(
        "import sys\n"
        "from eml_attachment_remover import cli\n"
        "status = cli.main(['--dry-run', sys.argv[1]])\n"
        f"print(status, {METADATA_LOADED})",
        str(source),
    )
    assert result.returncode == 0, result.stderr
    assert result.stdout.strip().splitlines()[-1] == "0 False"


def test_a_json_report_run_resolves_the_version_it_prints(tmp_path: Path) -> None:
    """The report carries the version, so producing one is when it is resolved."""
    source = tmp_path / "message.eml"
    source.write_bytes(b"Content-Type: text/plain\r\n\r\nbody\r\n")
    result = _run_python(
        "import sys\n"
        "from eml_attachment_remover import cli\n"
        "status = cli.main(['--dry-run', '--output-format', 'json', sys.argv[1]])\n"
        f"print(status, {METADATA_LOADED}, file=sys.stderr)",
        str(source),
    )
    assert result.returncode == 0, result.stderr
    assert result.stderr.strip().splitlines()[-1] == "0 True"
    assert f'"version": "{_version.program_version()}"' in result.stdout


def test_the_version_is_looked_up_once_however_often_it_is_asked_for(
    monkeypatch: pytest.MonkeyPatch,
) -> None:
    """A second request reuses the first answer instead of reading metadata again."""
    requested: list[str] = []

    def distribution_version(name: str) -> str:
        requested.append(name)
        return "7.7.7"

    monkeypatch.setattr(_version, "distribution_version", distribution_version)
    assert _version.program_version() == "7.7.7"
    assert _version.program_version() == "7.7.7"
    assert requested == [_version.DISTRIBUTION_NAME]


def test_every_route_to_the_version_gives_the_same_answer() -> None:
    """The function, both module attributes and the package attributes agree."""
    expected = _version.program_version()
    assert expected == _version.PROGRAM_VERSION
    assert expected == eml_attachment_remover.PROGRAM_VERSION
    assert eml_attachment_remover.__version__ == expected


@pytest.mark.parametrize("module", [_version, eml_attachment_remover])
def test_any_other_missing_attribute_is_still_an_attribute_error(
    module: object,
) -> None:
    """Only the version names are computed; nothing else silently exists."""
    with pytest.raises(AttributeError, match="has no attribute 'nonexistent'"):
        _ = module.nonexistent  # type: ignore[attr-defined]
    assert not hasattr(module, "nonexistent")


def test_building_the_parser_does_not_resolve_the_version(
    monkeypatch: pytest.MonkeyPatch,
) -> None:
    """Only --version needs the text, so constructing and using the parser must not."""

    def forbidden() -> str:
        message = "the version was resolved without being asked for"
        raise AssertionError(message)

    monkeypatch.setattr(cli_parser, "program_version", forbidden)
    namespace = cli_parser.build_parser().parse_args(["--dry-run", "message.eml"])
    assert namespace.dry_run is True
    assert not hasattr(namespace, "version")


def test_the_version_option_prints_the_program_and_version_and_exits_cleanly(
    monkeypatch: pytest.MonkeyPatch, capsys: pytest.CaptureFixture[str]
) -> None:
    """The option keeps argparse's output and status while resolving on use."""
    monkeypatch.setattr(cli_parser, "program_version", lambda: "9.9.9")
    with pytest.raises(SystemExit) as raised:
        cli.main(["--version"])
    assert raised.value.code == 0
    captured = capsys.readouterr()
    assert captured.out == "remove-eml-attachments 9.9.9\n"
    assert not captured.err


def test_the_version_option_is_listed_with_argparses_own_wording() -> None:
    """The help line for the lazy option is the one argparse's version action gives."""
    help_text = cli_parser.build_parser().format_help()
    assert f"--version{' ' * 13}{cli_parser.VERSION_HELP}" in help_text
