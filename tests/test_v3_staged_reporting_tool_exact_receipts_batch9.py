"""Exact last-mile receipts for staged error composition and public tooling."""

from __future__ import annotations

from contextlib import contextmanager
from typing import TYPE_CHECKING

import pytest
from tools import hypothesis_observation_safety, junit_report, smoke_distribution

from eml_attachment_remover import cli_parser, staged_output
from eml_attachment_remover.native_paths import bind_destination

if TYPE_CHECKING:
    from collections.abc import Iterator
    from pathlib import Path


def test_staged_publish_preserves_inner_and_deferred_exit_failures_together(
    tmp_path: Path, monkeypatch: pytest.MonkeyPatch
) -> None:
    """Pre-edge and deferred-exit failures remain independently visible."""
    primary = RuntimeError("stage failure")
    deferred = KeyboardInterrupt()

    @contextmanager
    def defer() -> Iterator[None]:
        yield  # ruff: ignore[fallible-context-manager] - inject post-yield failure.
        raise deferred

    monkeypatch.setattr(staged_output, "_defer_signals", defer)
    monkeypatch.setattr(
        staged_output,
        "_bind_parent",
        lambda _state: None,
    )
    monkeypatch.setattr(
        staged_output,
        "_create_stage",
        lambda _state: (_ for _ in ()).throw(primary),
    )
    monkeypatch.setattr(staged_output, "_cleanup", lambda _state: ("succeeded", None))
    destination = bind_destination(str(tmp_path / "output.eml"))

    with pytest.raises(BaseExceptionGroup) as raised:
        staged_output.publish(destination, b"candidate")
    assert raised.value.exceptions == (primary, deferred)


def test_staged_publish_retains_default_cleanup_receipt_when_entry_fails(
    tmp_path: Path, monkeypatch: pytest.MonkeyPatch
) -> None:
    """Shield entry failure leaves a valid default cleanup tuple for finishing."""
    failure = RuntimeError("shield setup failed")

    def fail() -> None:
        raise failure

    @contextmanager
    def defer() -> Iterator[None]:
        fail()
        yield

    monkeypatch.setattr(staged_output, "_defer_signals", defer)
    destination = bind_destination(str(tmp_path / "output.eml"))

    with pytest.raises(RuntimeError) as raised:
        staged_output.publish(destination, b"candidate")
    assert raised.value is failure


def test_parser_removed_short_and_inline_formats_have_exact_preparse_results() -> None:
    """The raw parser recognizes removed compact spellings before argparse owns them."""
    assert cli_parser._removed_existing("-f")  # ruff: ignore[private-member-access] - removed short spelling.
    assert cli_parser._removed_existing("-fverify")  # ruff: ignore[private-member-access] - removed compact assignment.
    assert cli_parser._removed_paths(["--output-format=paths"])  # ruff: ignore[private-member-access] - removed inline channel.
    assert cli_parser.raw_json_requested(["--output-format=json"])


def test_junit_replacements_include_raw_and_resolved_absolute_isolated_roots(
    tmp_path: Path, monkeypatch: pytest.MonkeyPatch
) -> None:
    """Sanitization recognizes a symlinked isolated parent and its resolved form."""
    actual = tmp_path / "actual-isolated"
    actual.mkdir()
    isolated = tmp_path / "isolated-link"
    isolated.symlink_to(actual, target_is_directory=True)
    source = isolated / "test-results.xml"
    monkeypatch.setattr(
        hypothesis_observation_safety,
        "path_prefix_variants",
        lambda value: (value,),
    )
    replacements = dict(junit_report._replacements(source, tmp_path))  # ruff: ignore[private-member-access] - raw and resolved replacement origins.
    assert replacements[str(isolated)] == junit_report.ISOLATED_PLACEHOLDER
    assert replacements[str(actual)] == junit_report.ISOLATED_PLACEHOLDER


class _Leaf:
    """Minimal parser leaf with one controlled semantic defect list."""

    def __init__(self, content_type: str, defects: list[object]) -> None:
        self.content_type = content_type
        self.defects = defects

    @staticmethod
    def is_multipart() -> bool:
        return False

    def get_content_type(self) -> str:
        return self.content_type

    @staticmethod
    def get_content_disposition() -> None:
        return None

    @staticmethod
    def get(_name: str) -> None:
        return None


class _Body:
    """Minimal body value retaining the expected public text."""

    @staticmethod
    def get_content() -> str:
        return smoke_distribution.EXPECTED_BODY


class _Parsed:
    """Minimal output parse tree that is semantically clean except for one defect."""

    def __init__(self) -> None:
        self.leaves = [_Leaf("text/plain", []), _Leaf("text/html", [object()])]

    @staticmethod
    def get_body(*, preferencelist: tuple[str, ...]) -> _Body:
        assert preferencelist == ("plain",)
        return _Body()

    def walk(self) -> list[_Leaf]:
        return self.leaves

    def __getitem__(self, name: str) -> str:
        assert name == smoke_distribution.SUBJECT_HEADER
        return smoke_distribution.EXPECTED_SUBJECT


class _Parser:
    """Minimal standard-library parser facade yielding the controlled parse tree."""

    @staticmethod
    def parsebytes(_raw: bytes) -> _Parsed:
        return _Parsed()


def test_smoke_verification_rejects_an_otherwise_clean_output_with_defects(
    tmp_path: Path, monkeypatch: pytest.MonkeyPatch
) -> None:
    """Parser defects independently fail installed-artifact semantic verification."""
    source = tmp_path / "source.eml"
    destination = tmp_path / "output.eml"
    original = b"public source"
    source.write_bytes(original)
    destination.write_bytes(b"public output")
    monkeypatch.setattr(smoke_distribution, "BytesParser", lambda **_kwargs: _Parser())

    with pytest.raises(RuntimeError, match="expected MIME-pruned EML"):
        smoke_distribution._verify_output(  # ruff: ignore[private-member-access] - defect semantics gate.
            source, destination, original
        )
