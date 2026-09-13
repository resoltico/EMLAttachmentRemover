"""Public reporting receipts for the last residual serializer mutations."""

from __future__ import annotations

import io
from typing import TYPE_CHECKING

from eml_attachment_remover import reporting_v3
from eml_attachment_remover.domain import (
    LedgerItem,
    PathValue,
    RetainedFingerprint,
    TransformationPlan,
)

if TYPE_CHECKING:
    import pytest


def test_basename_serialization_keeps_one_exact_native_encoding_channel() -> None:
    """Byte and Windows-string basenames produce mutually exclusive schema evidence."""
    assert reporting_v3._basename(b"\xff") == {  # ruff: ignore[private-member-access] - POSIX basename evidence.
        "basename_base64": "/w==",
        "basename_utf16le_base64": None,
    }
    assert reporting_v3._basename("π") == {  # ruff: ignore[private-member-access] - Windows basename evidence.
        "basename_base64": None,
        "basename_utf16le_base64": "wAM=",
    }


def test_transformation_serialization_preserves_opaque_parameter_bytes_exactly() -> (
    None
):
    """Retained parameter names and values remain independently encoded evidence."""
    item = LedgerItem(0, PathValue("source", "source", "c291cmNl"))
    item.transformation = TransformationPlan(
        removals=(),
        retained=(
            RetainedFingerprint(
                (),
                "text/plain",
                "8bit",
                ((b"name", b"\xff"),),
                "encoded",
                "decoded",
            ),
        ),
        stripped_headers=(),
        candidate_sha256="candidate",
        candidate_size=1,
        candidate=b"x",
    )
    serialized = reporting_v3._transformation(item)  # ruff: ignore[private-member-access] - opaque retained evidence.
    assert serialized is not None
    retained = serialized["retained"]
    assert retained == [
        {
            "source_mime_path": (),
            "content_type": "text/plain",
            "cte": "8bit",
            "content_type_parameters": [
                {"name_base64": "bmFtZQ==", "value_base64": "/w=="}
            ],
            "encoded_sha256": "encoded",
            "decoded_sha256": "decoded",
        }
    ]


def test_safe_human_output_preserves_unpaired_surrogates_as_display_escapes() -> None:
    """Human reporting cannot fail on an unpaired surrogate display string."""
    stream = io.StringIO()
    reporting_v3._safe(stream, "prefix-\ud800")  # ruff: ignore[private-member-access] - terminal display safety.
    assert stream.getvalue() == "prefix-\\ud800\n"


def test_json_reporting_keeps_unicode_visible_and_writes_one_newline(
    capsys: pytest.CaptureFixture[str],
) -> None:
    """The JSON channel emits a canonical visible-Unicode one-document line."""
    reporting_v3.write_json({"z": "π", "a": 1})
    assert capsys.readouterr().out == '{"a": 1, "z": "π"}\n'
