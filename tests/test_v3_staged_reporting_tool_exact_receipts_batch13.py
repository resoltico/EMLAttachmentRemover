"""Exact public receipts for the final reporting and raw-parser boundaries."""

from __future__ import annotations

import pytest

from eml_attachment_remover import cli_parser, reporting_v3
from eml_attachment_remover.domain import AppError, ExitCode


def test_canonical_json_passes_fixed_ascii_policy_to_the_json_boundary(
    monkeypatch: pytest.MonkeyPatch,
) -> None:
    """The JSON implementation receives a boolean schema policy, not falsey data."""
    calls: list[tuple[object, bool, bool, bool]] = []

    def dumps(
        document: object, *, ensure_ascii: bool, sort_keys: bool, allow_nan: bool
    ) -> str:
        calls.append((document, ensure_ascii, sort_keys, allow_nan))
        return "{}"

    monkeypatch.setattr(reporting_v3.__dict__["json"], "dumps", dumps)
    assert (
        reporting_v3._canonical_json(  # ruff: ignore[private-member-access] - JSON delegate receipt.
            {"public": "π"}
        )
        == "{}"
    )
    assert calls == [({"public": "π"}, True, True, False)]


def test_raw_migration_spellings_produce_the_exact_public_usage_documents() -> None:
    """Removed output policy and paths channels have distinct v3 migration receipts."""
    cases = (
        (["-f", "source.eml"], cli_parser.MIGRATION_EXISTING),
        (["--output-format=paths", "source.eml"], cli_parser.MIGRATION_PATHS),
    )
    for arguments, message in cases:
        with pytest.raises(AppError) as raised:
            cli_parser.validate_raw_arguments(arguments)
        assert raised.value == AppError(ExitCode.USAGE, message)


def test_raw_json_selection_requires_one_of_the_two_exact_supported_spellings() -> None:
    """Automation selects JSON only through documented inline or split forms."""
    assert cli_parser.raw_json_requested(["--output-format=json", "source.eml"])
    assert cli_parser.raw_json_requested(["--output-format", "json", "source.eml"])
    assert not cli_parser.raw_json_requested(["--output-format", "human", "source.eml"])
