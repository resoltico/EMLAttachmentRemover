"""Exercise the package's direct module entry point in process."""

from __future__ import annotations

import runpy
from unittest.mock import patch

import pytest

from eml_attachment_remover import app


def test_module_entrypoint_exits_with_the_cli_result() -> None:
    """Forward the application result through ``python -m`` semantics."""
    with (
        patch.object(app, "main", return_value=9) as main,
        pytest.raises(SystemExit) as raised,
    ):
        runpy.run_module("eml_attachment_remover.__main__", run_name="__main__")

    assert raised.value.code == 9
    main.assert_called_once_with()
