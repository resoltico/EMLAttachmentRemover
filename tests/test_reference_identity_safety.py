# ruff: file-ignore[private-member-access]
"""Require exact case and reserved-octet identity for body resources."""

from __future__ import annotations

from email.message import EmailMessage

import pytest

from eml_attachment_remover import mime_policy, mime_references


def _attachment(
    filename: str,
    *,
    cid: str | None = None,
    location: str | None = None,
) -> EmailMessage:
    """Return one labeled explicit attachment.

    Returns:
        The configured synthetic resource.

    """
    part = EmailMessage()
    part.set_content(b"public payload", maintype="image", subtype="png")
    part.add_header("Content-Disposition", "attachment", filename=filename)
    if cid is not None:
        part["Content-ID"] = f"<{cid}>"
    if location is not None:
        part["Content-Location"] = location
    return part


def _remove(
    body: str, *attachments: EmailMessage
) -> tuple[list[str | None], list[str | None]]:
    """Return removed and body-reference-retained filenames.

    Returns:
        Exact filenames from the policy result.

    """
    message = EmailMessage()
    message.set_content(body, subtype="html")
    message.make_mixed()
    for attachment in attachments:
        message.attach(attachment)
    state = mime_policy._remove_attachments(
        message,
        mime_references._collect_references(message),
    )
    return (
        [part.filename for part in state.removed],
        [part.filename for part in state.preserved_file_parts],
    )


@pytest.mark.parametrize(
    ("reference", "collision"),
    [
        ("User@Example.test", "user@Example.test"),
        ("User@Example.test", "User@example.test"),
        ("User@Example.test", "user@example.test"),
    ],
)
def test_cid_comparison_is_case_sensitive(reference: str, collision: str) -> None:
    """Retain only the Content-ID with exact identifier case."""
    assert _remove(
        f'<img src="CID:{reference}">',
        _attachment("actual.png", cid=reference),
        _attachment("collision.png", cid=collision),
    ) == (["collision.png"], ["actual.png"])


def test_cid_url_decoding_does_not_decode_content_id_header() -> None:
    """URL-decode the cid value but preserve percent syntax in the MIME header."""
    assert _remove(
        '<img src="cid:a%40b">',
        _attachment("actual.png", cid="a@b"),
        _attachment("collision.png", cid="a%40b"),
    ) == (["collision.png"], ["actual.png"])


def test_cid_quoted_local_part_and_literal_scheme_text_remain_exact() -> None:
    """Preserve addr-spec quotes and a literal cid prefix inside the identifier."""
    assert _remove(
        '<img src="cid:%22public%22@example.test">'
        '<img src="cid:cid%3Apublic@example.test">',
        _attachment("quoted.png", cid='"public"@example.test'),
        _attachment("bare.png", cid="public@example.test"),
        _attachment("scheme.png", cid="cid:public@example.test"),
    ) == (["bare.png"], ["quoted.png", "scheme.png"])


def test_cid_uri_brackets_are_removed_only_when_paired() -> None:
    """Normalize a bracketed CID URI but preserve malformed bracket identity."""
    assert (
        mime_references._canonical_content_id(
            "<public@example.test>",
        )
        == "public@example.test"
    )
    assert (
        mime_references._canonical_content_id(
            "CID:<public@example.test>",
        )
        == "public@example.test"
    )
    assert (
        mime_references._canonical_content_id(
            "cid:<public@example.test",
        )
        == "<public@example.test"
    )


def test_uri_host_only_is_case_insensitive() -> None:
    """Fold scheme and host while preserving path, query, and fragment case."""
    actual = "https://Public.Example/Path/Image.png?Q=One#Hero"
    collision = "https://public.example/path/image.png?q=one#hero"
    reference = "HTTPS://PUBLIC.EXAMPLE/Path/Image.png?Q=One#Hero"

    assert _remove(
        f'<img src="{reference}">',
        _attachment("actual.png", location=actual),
        _attachment("collision.png", location=collision),
    ) == (["collision.png"], ["actual.png"])


def test_absolute_uri_dot_segments_are_normalized() -> None:
    """Remove dot segments without folding the remaining path identity."""
    assert _remove(
        '<img src="https://public.example/a/%2E%2E/b.png">',
        _attachment(
            "actual.png",
            location="https://public.example/b.png",
        ),
        _attachment(
            "collision.png",
            location="https://public.example/B.png",
        ),
    ) == (["collision.png"], ["actual.png"])


def test_relative_uri_dot_segments_are_normalized_without_erasing_parents() -> None:
    """Collapse safe relative dot segments while preserving unresolved parents."""
    assert _remove(
        '<img src="a/../b.png"><img src="../a/../parent.png">',
        _attachment("actual.png", location="b.png"),
        _attachment("parent.png", location="../parent.png"),
        _attachment("collision.png", location="B.png"),
    ) == (["collision.png"], ["actual.png", "parent.png"])


@pytest.mark.parametrize(
    ("reference", "actual", "collision"),
    [
        (
            "http://public.example:80/image.png",
            "http://public.example/image.png",
            "http://public.example:81/image.png",
        ),
        (
            "https://public.example:443/image.png",
            "https://public.example/image.png",
            "https://public.example:444/image.png",
        ),
    ],
)
def test_http_default_ports_are_normalized(
    reference: str,
    actual: str,
    collision: str,
) -> None:
    """Treat only the HTTP scheme's defined default port as equivalent."""
    assert _remove(
        f'<img src="{reference}">',
        _attachment("actual.png", location=actual),
        _attachment("collision.png", location=collision),
    ) == (["collision.png"], ["actual.png"])


@pytest.mark.parametrize("reserved", ["23", "2F", "3F", "26"])
def test_reserved_percent_octet_cannot_retain_decoded_collision(
    reserved: str,
) -> None:
    """Keep reserved escapes distinct from URI syntax and literal characters."""
    encoded = f"img%{reserved}one.png"
    decoded = f"img{chr(int(reserved, 16))}one.png"

    assert _remove(
        f'<img src="{encoded}">',
        _attachment("encoded.png", location=encoded),
        _attachment("decoded.png", location=decoded),
    ) == (["decoded.png"], ["encoded.png"])


@pytest.mark.parametrize(
    ("reference", "collision"),
    [
        ("image.png?", "image.png"),
        ("image.png?#", "image.png"),
        ("image.png?#Hero", "image.png#Hero"),
    ],
)
def test_empty_query_delimiters_remain_distinct(
    reference: str,
    collision: str,
) -> None:
    """Do not merge an explicitly empty query component away."""
    assert _remove(
        f'<img src="{reference}">',
        _attachment("actual.png", location=reference),
        _attachment("collision.png", location=collision),
    ) == (["collision.png"], ["actual.png"])


def test_embedded_data_and_same_document_fragments_do_not_retain_parts() -> None:
    """Ignore self-contained data and fragment-only body references."""
    assert _remove(
        '<img src="data:image/png;base64,AAAA"><svg><use href="#symbol"></svg>',
        _attachment(
            "data.png",
            location="data:image/png;base64,AAAA",
        ),
        _attachment("fragment.png", location="#symbol"),
    ) == (["data.png", "fragment.png"], [])


def test_valid_apostrophes_in_uri_paths_remain_identity_significant() -> None:
    """Preserve apostrophes from URI data instead of treating them as wrappers."""
    assert _remove(
        '<img src="&apos;logo.png&apos;">',
        _attachment("actual.png", location="'logo.png'"),
        _attachment("collision.png", location="logo.png"),
    ) == (["collision.png"], ["actual.png"])


def test_fragment_removal_starts_at_the_first_delimiter() -> None:
    """Map a tolerant multi-delimiter fragment spelling to its fetched resource."""
    assert _remove(
        '<img src="image.png#public#view">',
        _attachment("actual.png", location="image.png"),
        _attachment("collision.png", location="image.png#public"),
    ) == (["collision.png"], ["actual.png"])
