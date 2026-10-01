"""Every macOS quality lane checks Swift with its selected Python runtime."""

from __future__ import annotations

import sys
from unittest.mock import Mock, call, patch

import pytest
from tools import tasks


@pytest.mark.parametrize("platform", ["darwin", "linux", "win32"])
@pytest.mark.parametrize("native", [False, True])
def test_quality_orders_swift_checks_before_host_gates(
    platform: str, *, native: bool
) -> None:
    calls = Mock()
    with (
        patch.object(sys, "platform", platform),
        patch.object(tasks, "_run", calls.swift),
        patch.object(tasks, "_hygiene", calls.hygiene),
        patch.object(tasks, "_check", calls.check),
        patch.object(tasks, "_coverage", calls.coverage),
    ):
        assert tasks.main(["quality", *(["--native"] if native else [])]) == 0
    expected = []
    if platform == "darwin":
        expected.append(
            call.swift(
                (
                    "/bin/sh",
                    str(tasks.PROJECT_ROOT / "integrations/macos-ui/quality.sh"),
                ),
                environment_updates={"EML_REMOVER_PYTHON": sys.executable},
            )
        )
    expected.extend([call.hygiene() if native else call.check(), call.coverage()])
    assert calls.mock_calls == expected


def test_static_gate_executes_the_central_exception_registry() -> None:
    audit = Mock(issues=(), public_files=())
    with (
        patch.object(tasks, "_run") as run,
        patch.object(
            tasks.check_repository_hygiene, "audit_repository", return_value=audit
        ),
    ):
        assert tasks.main(["check"]) == 0
    assert run.call_args_list[7] == call((sys.executable, "tools/lint_exceptions.py"))
