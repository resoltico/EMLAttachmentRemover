# ruff: file-ignore[private-member-access]
"""Exact semantic contracts for retained MIME structure verification."""

from __future__ import annotations

import tempfile
from collections import Counter
from email.message import EmailMessage
from pathlib import Path
from unittest.mock import patch

import pytest

from eml_attachment_remover import mime_serialization
from eml_attachment_remover.models import CliError, ExitCode


def _structured_message() -> EmailMessage:
    """Return a public message with observable structure metadata.

    Returns:
        A two-leaf multipart message with parameters and auxiliary text.

    """
    first = EmailMessage()
    first.set_content("first")
    second = EmailMessage()
    second.set_content("second")
    message = EmailMessage()
    message["Subject"] = "PUBLIC SUBJECT"
    message["Content-Type"] = (
        'multipart/mixed; boundary="PUBLIC-BOUNDARY"; '
        'protocol="public-protocol"; x-public="one"'
    )
    message.set_payload([first, second])
    message.preamble = "PUBLIC PREAMBLE\r"
    message.epilogue = "PUBLIC EPILOGUE\r"
    return message


def test_iter_parts_preserves_depth_first_paths_and_sibling_order() -> None:
    """Yield every node once at its exact immutable path."""
    message = _structured_message()

    assert [path for path, _part in mime_serialization._iter_parts(message)] == [
        (),
        (0,),
        (1,),
    ]


def test_leaf_fingerprint_path_makes_each_counter_key_unique() -> None:
    """Represent identical siblings independently through their MIME paths."""
    leaf = EmailMessage()
    leaf.set_content("PUBLIC")
    message = EmailMessage()
    message.make_mixed()
    message.attach(leaf)
    message.attach(leaf)

    fingerprints = mime_serialization._leaf_fingerprints(message)

    assert set(fingerprints.values()) == {1}
    assert {fingerprint[0] for fingerprint in fingerprints} == {(0,), (1,)}


def test_empty_multipart_has_no_leaf_fingerprint() -> None:
    """Traverse a valid empty multipart without inventing a payload leaf."""
    message = EmailMessage()
    message.make_mixed()

    assert mime_serialization._leaf_fingerprints(message) == Counter()


def test_structure_fingerprint_records_parameters_headers_and_lone_cr() -> None:
    """Exclude only the transport boundary while retaining all semantic metadata."""
    fingerprint = mime_serialization._structure_fingerprint(_structured_message())
    root = fingerprint[0]

    assert root[0] == ()
    assert root[1] == "multipart/mixed"
    assert root[2] == (("protocol", "public-protocol"), ("x-public", "one"))
    assert root[3] == (("subject", "PUBLIC SUBJECT"),)
    assert root[5:] == ("PUBLIC PREAMBLE\n", "PUBLIC EPILOGUE\n")
    assert [part[0] for part in fingerprint] == [(), (0,), (1,)]


@pytest.mark.parametrize(
    ("field", "replacement"),
    [
        ("preamble", "PUBLIC CHANGED PREAMBLE\n"),
        ("epilogue", "PUBLIC CHANGED EPILOGUE\n"),
    ],
)
def test_structure_fingerprint_distinguishes_each_auxiliary_field(
    field: str,
    replacement: str,
) -> None:
    """Keep preamble and epilogue independently bound to their tree node."""
    original = _structured_message()
    changed = _structured_message()
    setattr(changed, field, replacement)

    assert mime_serialization._structure_fingerprint(changed) != (
        mime_serialization._structure_fingerprint(original)
    )


@pytest.mark.parametrize(
    ("mutate", "expected_component"),
    [
        ("parameter", (("protocol", "changed"), ("x-public", "one"))),
        ("header", (("subject", "PUBLIC CHANGED"),)),
    ],
)
def test_structure_fingerprint_distinguishes_parameters_and_headers(
    mutate: str,
    expected_component: tuple[tuple[str, str], ...],
) -> None:
    """Bind semantic Content-Type parameters and ordinary headers separately."""
    message = _structured_message()
    if mutate == "parameter":
        message.set_param("protocol", "changed")
        component_index = 2
    else:
        message.replace_header("Subject", "PUBLIC CHANGED")
        component_index = 3

    assert mime_serialization._structure_fingerprint(message)[0][component_index] == (
        expected_component
    )


def test_structure_verification_uses_the_exact_public_diagnostic() -> None:
    """Report the stable structure-and-header verification failure text."""
    with tempfile.TemporaryDirectory() as directory:
        temporary = Path(directory) / "public.eml"
        temporary.write_bytes(b"PUBLIC")
        with (
            patch.object(
                mime_serialization,
                "_parse_message",
                return_value=(EmailMessage(), ()),
            ),
            patch.object(
                mime_serialization,
                "_leaf_fingerprints",
                return_value=Counter(),
            ),
            patch.object(
                mime_serialization,
                "_structure_fingerprint",
                return_value=(),
            ),
            pytest.raises(CliError) as raised,
        ):
            mime_serialization._verify_serialized_message(
                temporary,
                Counter(),
                (((), "changed", (), (), None, None, None),),
            )

    assert raised.value.code is ExitCode.VERIFICATION_ERROR
    assert raised.value.message == (
        "generated EML did not preserve the retained MIME structure and headers"
    )
