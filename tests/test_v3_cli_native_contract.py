"""Focused v3 raw-argument and cancellation contracts."""

from __future__ import annotations

import pytest

from eml_attachment_remover.cancellation import CancellationSignal
from eml_attachment_remover.cli_parser import (
    MIGRATION_PATHS,
    build_parser,
    raw_json_requested,
    raw_source_candidates,
    validate_arguments,
    validate_raw_arguments,
)
from eml_attachment_remover.domain import AppError, BatchLedger, ExitCode, ItemStatus
from eml_attachment_remover.native_paths import path_value


def test_raw_end_of_options_prevents_removed_switch_interpretation() -> None:
    validate_raw_arguments(["--", "--force"])
    assert raw_source_candidates(["--", "--force"]) == ["--force"]


def test_preparse_json_recognizes_separate_output_format_value() -> None:
    arguments = ["--output-format", "json", "--force", "source.eml"]
    assert raw_json_requested(arguments) is True
    assert raw_source_candidates(arguments) == ["source.eml"]


def test_removed_newline_paths_has_exact_preparse_migration_error() -> None:
    with pytest.raises(AppError) as captured:
        validate_raw_arguments(["--output-format", "paths", "source.eml"])
    assert captured.value.code is ExitCode.USAGE
    assert captured.value.message == MIGRATION_PATHS


def test_cancellation_signal_preserves_exception_arguments() -> None:
    cancellation = CancellationSignal(15, "SIGTERM")
    assert cancellation.args == (15, "SIGTERM")
    assert cancellation.number == 15
    assert cancellation.name == "SIGTERM"


def test_parser_usage_and_postparse_safety_combinations_are_typed() -> None:
    parser = build_parser()
    with pytest.raises(AppError) as parser_error:
        parser.parse_args([])
    assert parser_error.value.code is ExitCode.USAGE
    with pytest.raises(AppError) as output_error:
        validate_arguments(parser.parse_args(["-o", "out.eml", "one.eml", "two.eml"]))
    assert output_error.value.code is ExitCode.USAGE
    with pytest.raises(AppError) as dry_error:
        validate_arguments(
            parser.parse_args(["--dry-run", "--output-format", "paths0", "one.eml"])
        )
    assert dry_error.value.code is ExitCode.USAGE
    with pytest.raises(AppError) as existing_error:
        validate_raw_arguments(["--force", "one.eml"])
    assert existing_error.value.code is ExitCode.USAGE
    validate_arguments(parser.parse_args(["one.eml"]))


def test_domain_error_and_single_terminal_ledger_rules_are_enforced() -> None:
    error = AppError(ExitCode.PARSE_ERROR, "bad message")
    assert str(error) == "bad message"
    ledger = BatchLedger.from_requests([path_value("one.eml")])
    item = ledger.items[0]
    item.finish(ItemStatus.FAILED, error)
    with pytest.raises(RuntimeError):
        item.finish(ItemStatus.NOT_RUN)
    ledger.record_interruption("SIGTERM", "candidate")
    with pytest.raises(RuntimeError):
        ledger.record_interruption("SIGINT", "report")
