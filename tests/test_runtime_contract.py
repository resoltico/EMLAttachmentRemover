"""Require installed entry points and wheel tags to enforce the runtime contract."""

from __future__ import annotations

import io
import re
import tomllib
from pathlib import Path
from unittest.mock import MagicMock, patch

import pytest
from tools import hatch_build

from eml_attachment_remover import runtime

PROJECT_CONFIG = Path(__file__).resolve().parents[1] / "pyproject.toml"


def test_runtime_constants_match_authoritative_project_metadata() -> None:
    """Bind the source guard to the canonical build and runtime declaration."""
    with PROJECT_CONFIG.open("rb") as config_file:
        configuration = tomllib.load(config_file)
    assert configuration["project"]["requires-python"] == ">=3.14,<3.15"
    assert (
        configuration["tool"]["eml-attachment-remover"]["runtime"]["implementation"]
        == runtime.SUPPORTED_IMPLEMENTATION
    )
    assert runtime.SUPPORTED_VERSION == (3, 14)
    assert hatch_build.wheel_tag(PROJECT_CONFIG) == "cp314-none-any"


def test_runtime_predicate_requires_both_exact_contract_dimensions() -> None:
    """Reject another implementation or Python minor independently."""
    assert runtime.runtime_supported("CPython", (3, 14))
    assert not runtime.runtime_supported("PyPy", (3, 14))
    assert not runtime.runtime_supported("CPython", (3, 13))
    assert not runtime.runtime_supported("CPython", (3, 15))


def test_runtime_boundary_rejects_before_importing_application() -> None:
    """Return a stable diagnostic without entering application code."""
    stderr = io.StringIO()
    with (
        patch.object(runtime, "runtime_supported", return_value=False),
        patch(
            "eml_attachment_remover.runtime.platform.python_implementation",
            return_value="PyPy",
        ),
        patch("eml_attachment_remover.runtime.sys.version_info", (3, 14)),
        patch("sys.stderr", stderr),
    ):
        status = runtime.main()
    assert status == runtime.UNSUPPORTED_RUNTIME_STATUS
    assert stderr.getvalue() == (
        "EML Attachment Remover requires CPython 3.14.x; found PyPy 3.14\n"
    )


def test_runtime_boundary_delegates_on_supported_interpreter() -> None:
    """Return the application status unchanged on the declared runtime."""
    with (
        patch.object(runtime, "runtime_supported", return_value=True),
        patch("eml_attachment_remover.app.main", return_value=9) as application_main,
    ):
        assert runtime.main() == 9
    application_main.assert_called_once_with()


@pytest.mark.parametrize(
    ("requirement", "implementation", "message"),
    [
        (
            ">=3.14",
            "CPython",
            "wheel tag requires exact CPython major/minor project bounds",
        ),
        (
            ">=3.14,<3.16",
            "CPython",
            "wheel tag requires one supported CPython minor version",
        ),
        (
            ">=3.14,<3.15",
            "PyPy",
            "wheel tag requires exact CPython major/minor project bounds",
        ),
    ],
)
def test_wheel_tag_rejects_ambiguous_or_non_cpython_metadata(
    tmp_path: Path,
    requirement: str,
    implementation: str,
    message: str,
) -> None:
    """Fail rather than publish a falsely portable wheel tag."""
    project = tmp_path / "pyproject.toml"
    project.write_text(
        "[project]\n"
        f"requires-python = {requirement!r}\n"
        "[tool.eml-attachment-remover.runtime]\n"
        f"implementation = {implementation!r}\n",
        encoding="utf-8",
    )
    with pytest.raises(ValueError, match=re.escape(message)) as raised:
        hatch_build.wheel_tag(project)
    assert str(raised.value) == message


def test_hatch_hook_applies_the_metadata_derived_wheel_tag(tmp_path: Path) -> None:
    """Exercise Hatchling's actual hook boundary and canonical config path."""
    hook = hatch_build.CustomBuildHook(
        root=str(tmp_path),
        config={},
        build_config=MagicMock(),
        metadata=MagicMock(),
        directory=str(tmp_path),
        target_name="wheel",
    )
    build_data: dict[str, object] = {}
    with patch.object(
        hatch_build,
        "wheel_tag",
        return_value="cp314-none-any",
    ) as wheel_tag:
        hook.initialize("standard", build_data)
    assert build_data == {"tag": "cp314-none-any"}
    wheel_tag.assert_called_once_with(tmp_path / "pyproject.toml")
