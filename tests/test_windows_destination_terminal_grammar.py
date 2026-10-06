"""Terminal-form and grammar receipts for remaining native-value mutations."""

from __future__ import annotations

import pytest

from eml_attachment_remover import native_values
from eml_attachment_remover.domain import AppError, ExitCode


def test_windows_destination_grammar_and_both_terminal_separators_are_exact(
    monkeypatch: pytest.MonkeyPatch,
) -> None:
    with monkeypatch.context() as context:
        context.setattr(native_values.__dict__["os"], "name", "nt")
        assert native_values.default_destination("C:\\Mail\\Note.EML") == (
            "C:\\Mail\\Note.mime-pruned.eml"
        )
        for terminal in ("C:\\Mail\\", "C:/Mail/"):
            with pytest.raises(AppError) as captured:
                native_values.validate_windows_argument(terminal)
            assert captured.value == AppError(
                ExitCode.INPUT_ERROR, "path has an empty basename"
            )
