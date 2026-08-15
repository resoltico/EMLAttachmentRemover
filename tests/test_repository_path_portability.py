"""Require the public source surface to be checkout-safe on every supported OS."""

from __future__ import annotations

from typing import TYPE_CHECKING, cast

import pytest
from tools import repository_path_policy as policy

if TYPE_CHECKING:
    from collections.abc import Callable

component_messages = cast(
    "Callable[[str], tuple[str, ...]]",
    vars(policy)["_component_messages"],
)
component_key = cast("Callable[[str], str]", vars(policy)["_component_key"])


@pytest.mark.parametrize(
    ("relative", "expected"),
    [
        ("tests/CON.txt", "path component uses a Windows reserved device name"),
        ("tests/CON .txt", "path component uses a Windows reserved device name"),
        ("tests/CONIN$.log", "path component uses a Windows reserved device name"),
        ("tests/CONOUT$", "path component uses a Windows reserved device name"),
        (
            "tests/public?.txt",
            "path component contains a Windows-forbidden character",
        ),
        (
            "tests/public. ",
            "path component ends with a Windows-ignored space or dot",
        ),
        (
            "tests/public.",
            "path component ends with a Windows-ignored space or dot",
        ),
        (
            "tests/public\nname.txt",
            "path component contains a control character",
        ),
        ("tests/\udcff.txt", "path component is not valid Unicode text"),
    ],
)
def test_portable_path_messages_reject_every_uncheckoutable_category(
    relative: str,
    expected: str,
) -> None:
    """Detect hazards even when the current host filesystem permits them."""
    assert policy.audit_paths((relative,)) == (policy.PathIssue(relative, expected),)


def test_space_is_not_a_control_character() -> None:
    """Keep printable ASCII space outside the control-character range."""
    assert policy.audit_paths(("tests/public file.txt",)) == ()


def test_reserved_stem_uses_the_first_extension_and_only_windows_trimming() -> None:
    """Model Windows device stems without treating all whitespace as ignored."""
    reserved = "tests/CON.public.txt"
    assert policy.audit_paths((reserved,)) == (
        policy.PathIssue(
            reserved,
            "path component uses a Windows reserved device name",
        ),
    )

    tabbed = "tests/CON\t.txt"
    assert policy.audit_paths((tabbed,)) == (
        policy.PathIssue(tabbed, "path component contains a control character"),
    )
    assert component_messages("CONX.txt") == ()
    assert policy.audit_paths(("tests/CONX.txt",)) == ()


def test_portable_path_key_models_case_unicode_and_windows_suffix_collisions() -> None:
    """Normalize all equivalences relevant to supported checkout targets."""
    pairs = (
        ("tests/Public.txt", "tests/public.txt"),
        ("tests/café.txt", "tests/cafe\u0301.txt"),
        ("tests/public", "tests/public. "),
    )
    for first, second in pairs:
        assert any(
            "collides" in issue.message for issue in policy.audit_paths((first, second))
        )


def test_portable_key_does_not_ignore_an_ordinary_trailing_x() -> None:
    """Trim exactly Windows-ignored suffixes when constructing collision keys."""
    assert component_key("publicX") == "publicx"
    assert policy.audit_paths(("tests/public", "tests/publicX")) == ()


def test_audit_reports_directory_prefix_collisions_with_both_names() -> None:
    """Detect directories that collide before distinct filenames are compared."""
    files = ("Public/alpha.txt", "public/beta.txt")
    issues = policy.audit_paths(files)
    assert issues == (
        policy.PathIssue(
            files[1],
            "public path collides cross-platform with 'Public'",
        ),
    )


def test_nested_prefix_collision_diagnostic_preserves_exact_public_spelling() -> None:
    """Use the original slash-separated prefix in collision diagnostics."""
    files = ("dir/Public/alpha.txt", "dir/public/beta.txt")
    assert policy.audit_paths(files) == (
        policy.PathIssue(
            files[1],
            "public path collides cross-platform with 'dir/Public'",
        ),
    )
