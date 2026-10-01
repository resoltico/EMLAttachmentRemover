"""Parser-error inference follows final known choices and real option boundaries."""

from __future__ import annotations

import json

import pytest

from eml_attachment_remover import cli
from eml_attachment_remover.cli_parser import raw_json_requested


@pytest.mark.parametrize(
    ("arguments", "expected"),
    [
        (["--output-format=json", "--output-format=human"], False),
        (["--output-format", "json", "--output-format", "human"], False),
        (["--output-format=human", "--output-format", "json"], True),
        (["--output-format", "human", "--output-format=json"], True),
        (["--output-format=json", "--output-format=paths0"], False),
        (["--output=--output-format=json", "source.eml"], False),
        (["-o--output-format=json", "source.eml"], False),
        (["--output", "json", "source.eml"], False),
        (["--output-format=json", "--", "--output-format=human"], True),
        (["--", "--output-format=json"], False),
        (["--output", "--output-format=json"], True),
        (["--output", "-1", "--output-format=json"], True),
        (["--output", "-.5", "--output-format=json"], True),
        (["--output", "-", "--output-format=json"], True),
        (["--output-format=human", "--output-format=unknown"], False),
    ],
)
def test_raw_format_selection_uses_last_effective_known_occurrence(
    arguments: list[str], *, expected: bool
) -> None:
    assert raw_json_requested(arguments) is expected


@pytest.mark.parametrize("last", ["json", "human", "paths0"])
@pytest.mark.parametrize("separated", [False, True])
def test_actual_usage_error_obeys_the_last_format_choice(
    capsys: pytest.CaptureFixture[str],
    last: str,
    *,
    separated: bool,
) -> None:
    last_option = (
        ["--output-format", last] if separated else ["--output-format=" + last]
    )
    assert (
        cli.main([
            "--output-format=json",
            *last_option,
            "--invalid",
            "--",
            "source.eml",
        ])
        == 2
    )
    output = capsys.readouterr()
    if last == "json":
        assert json.loads(output.out)["exit_code"] == 2
        assert not output.err
    else:
        assert not output.out
        assert "USAGE" in output.err
