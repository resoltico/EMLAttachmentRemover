"""V3 assurance tooling must not retain the removed text-only product surface."""

from __future__ import annotations

from pathlib import Path

PROJECT_ROOT = Path(__file__).resolve().parents[1]


def test_assurance_tools_have_no_import_or_reference_to_removed_text_pipeline() -> None:
    forbidden = (
        "html_text",
        "html_references",
        "mime_text_only",
        "mime_text_plan",
        "mime_text_selection",
        "selected_plain",
        "text-only",
    )
    tool_sources = sorted(PROJECT_ROOT.glob("tools/*.py"))
    combined = "\n".join(path.read_text(encoding="utf-8") for path in tool_sources)
    assert not any(token in combined for token in forbidden)


def test_zipapp_and_release_tools_use_the_canonical_schema_three_path() -> None:
    schema_file = PROJECT_ROOT / "schema" / "report.schema.json"
    schema = schema_file.relative_to(PROJECT_ROOT)
    assert schema.as_posix() == "schema/report.schema.json"
    assert schema_file.is_file()
    assert (PROJECT_ROOT / "tools" / "build_zipapp.py").is_file()
