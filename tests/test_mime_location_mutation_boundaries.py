# ruff: file-ignore[private-member-access]
"""Boundary contracts for MIME URI normalization and base inheritance."""

from __future__ import annotations

from email.message import EmailMessage
from urllib.parse import SplitResult

from eml_attachment_remover import mime_locations, mime_references


def test_percent_normalization_validates_both_hex_digits_and_progresses() -> None:
    """Decode valid unreserved octets without consuming malformed triplets."""
    assert (
        mime_locations._normalize_percent_encoding(
            "%41%2f%G0%0G%25tail%42",
        )
        == "A%2F%G0%0G%25tailB"
    )


def test_absolute_uri_requires_one_valid_scheme_prefix() -> None:
    """Reject repeated delimiters and punctuation outside RFC scheme syntax."""
    assert mime_locations._is_absolute_uri("x+-.9:value")
    assert not mime_locations._is_absolute_uri("x_value:tail")
    assert not mime_locations._is_absolute_uri("relative/path:tail")


def test_ipv6_authority_normalizes_default_ports_and_nested_brackets() -> None:
    """Fold an IPv6 host and remove only the scheme's exact default port."""
    assert (
        mime_locations._canonical_location(
            "HTTPS://User@[ABCD::EF]:443/path",
        )
        == "https://User@[abcd::ef]/path"
    )
    assert (
        mime_locations._canonical_location(
            "https://[ABCD::EF]:444/path",
        )
        == "https://[abcd::ef]:444/path"
    )
    malformed_parts = SplitResult("http", "[AB]]:80", "/path", "", "")
    assert mime_locations._normalize_authority(malformed_parts) == "[ab]]:80"


def test_relative_path_normalization_preserves_empty_and_terminal_segments() -> None:
    """Keep unresolved parents and directory identity while collapsing safe dots."""
    normalize = mime_locations._normalize_path

    assert normalize("a//../b") == "a//../b"
    assert normalize("a/b/..") == "a/"
    assert normalize("a/b/.") == "a/b/"
    assert normalize("/a/../b/./c") == "/b/c"


def test_dot_segment_cursor_uses_first_path_separator_per_transition() -> None:
    """Move one segment at a time and remove only the last output segment."""
    assert mime_locations._remove_dot_segments("a/b/../../c") == "/c"
    assert mime_locations._without_last_path_segment("/a/b/c") == "/a/b"


def test_empty_query_and_fragment_delimiters_preserve_exact_order() -> None:
    """Restore an empty query before the first fragment delimiter."""
    canonical = mime_locations._canonical_location

    assert canonical("image.png?#one#two") == "image.png?#one#two"
    assert canonical("?#view") == "?#view"
    assert canonical("image.png?") == "image.png?"
    assert canonical("image.png#") == "image.png#"


def test_location_variants_distinguish_fetches_from_exact_mime_labels() -> None:
    """Distinguish fetched resource variants from exact MIME location labels."""
    value = "image.png#view#nested"

    assert mime_locations._location_variants(
        value,
        "https://public.example/root/",
    ) == frozenset({
        "https://public.example/root/image.png#view#nested",
        "https://public.example/root/image.png",
    })
    resource = EmailMessage()
    resource["Content-Location"] = value
    assert mime_locations._resource_location_variants(
        resource,
        "https://public.example/root/",
    ) == frozenset({"https://public.example/root/image.png#view#nested"})


def test_content_base_precedes_location_and_inherits_only_absolute_bases() -> None:
    """Resolve legacy base metadata without replacing a valid inherited base."""
    part = EmailMessage()
    part["Content-Base"] = "assets/"
    part["Content-Location"] = "body.html"

    assert (
        mime_locations._content_location_base(
            part,
            "https://public.example/root/",
        )
        == "https://public.example/root/assets/"
    )
    assert mime_locations._content_location_base(part, None) is None


def test_html_mime_base_handles_unbased_relative_location_directly() -> None:
    """Use a standalone relative Content-Location without a dummy base URI."""
    part = EmailMessage()
    part["Content-Location"] = "mail/body.html"

    assert mime_locations._html_mime_base(part, None) == "mail/body.html"
    query_only = EmailMessage()
    query_only["Content-Location"] = "?view"
    assert mime_locations._html_mime_base(query_only, None) == "?view"
    assert mime_references._effective_html_base("?view", None) == "?view"


def test_resource_content_base_resolution_requires_both_base_levels() -> None:
    """Do not join an inherited label to itself when Content-Base is absent."""
    resource = EmailMessage()
    resource["Content-Location"] = "image.png"

    assert mime_locations._resource_location_variants(
        resource,
        "assets/",
    ) == frozenset({"assets/image.png"})


def test_invalid_base_joins_fall_back_without_masking_other_errors() -> None:
    """Tolerate malformed public URI bases at each supported resolution point."""
    part = EmailMessage()
    part["Content-Base"] = "assets/"
    part["Content-Location"] = "image.png"
    invalid = "http://[invalid"

    assert mime_locations._location_variants("image.png", invalid) == frozenset({
        "image.png",
    })
    assert mime_locations._content_location_base(part, invalid) == invalid
    assert mime_locations._resource_location_variants(part, invalid) == frozenset({
        "assets/image.png",
    })
