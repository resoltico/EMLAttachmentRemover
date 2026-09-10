"""Exact final raw-CLI bounds receipt."""

from __future__ import annotations

from eml_attachment_remover import cli_parser


def test_raw_json_selection_treats_a_trailing_value_option_as_unselected() -> None:
    """A missing output-format value must be safe for preparse inspection."""
    assert not cli_parser.raw_json_requested(["--output-format"])
    assert not cli_parser.raw_json_requested(["source.eml", "--output-format"])
