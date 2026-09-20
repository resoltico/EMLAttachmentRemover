"""Exercise changelog extraction without generated announcements or network access."""

from __future__ import annotations

import re

import pytest
from tools.changelog import ReleaseError, extract_release

HEADING = "## [1.2.3] - 2026-09-20\n"
BODY = "### Fixed\n\n- Preserve **exact** wording.  \n"
ENTRY = HEADING + "\n" + BODY
FOOTER = (
    "[Unreleased]: https://example.test/next\n[1.2.3]: https://example.test/release\n"
)
DOCUMENT = "# Changelog\n\n## [Unreleased]\n\n" + ENTRY + "\n" + FOOTER


def test_exact_text_omits_heading_and_global_reference_footer() -> None:
    assert extract_release(DOCUMENT, "1.2.3") == BODY


def test_previous_entries_are_not_part_of_the_release() -> None:
    older = "\n## [1.2.2] - 2026-09-19\n\n- An older change.\n\n"
    text = DOCUMENT.replace("\n" + FOOTER, older + FOOTER)
    assert extract_release(text, "1.2.3") == BODY


def test_single_line_release_body_is_preserved() -> None:
    text = "## [Unreleased]\n\n" + HEADING + "\n- One complete change.\n"
    assert extract_release(text, "1.2.3") == "- One complete change.\n"


def test_release_body_immediately_after_its_heading_is_preserved() -> None:
    text = "## [Unreleased]\n\n" + HEADING + "- Immediate change.\n"
    assert extract_release(text, "1.2.3") == "- Immediate change.\n"


def test_empty_release_cannot_borrow_unreleased_change_text() -> None:
    text = (
        "## [Unreleased]\n- Pending change.\n"
        + HEADING
        + "## [1.2.2] - 2026-09-19\n\n- Older change.\n"
    )
    with pytest.raises(
        ReleaseError,
        match="^" + re.escape("Release entry must contain change text") + "$",
    ):
        extract_release(text, "1.2.3")


def test_release_body_preserves_leading_markdown_whitespace() -> None:
    text = "## [Unreleased]\n\n" + HEADING + "  - Indented change.\n"
    assert extract_release(text, "1.2.3") == "  - Indented change.\n"


def test_release_body_preserves_a_leading_x_character() -> None:
    text = "## [Unreleased]\n\n" + HEADING + "X-ray change.\n"
    assert extract_release(text, "1.2.3") == "X-ray change.\n"


def test_reference_footer_is_not_part_of_the_release_body() -> None:
    text = (
        "## [Unreleased]\n\n"
        + HEADING
        + "- One change.\n\n[1.2.3]: https://example.test/ends-in-X"
    )
    assert extract_release(text, "1.2.3") == "- One change.\n"


def test_reference_footer_does_not_change_the_release_body() -> None:
    text = (
        "## [Unreleased]\n\n"
        + HEADING
        + "- One change.\n\n  [1.2.3]: https://example.test/release\n"
    )
    assert extract_release(text, "1.2.3") == "- One change.\n"


def test_release_body_terminal_blank_lines_are_normalized() -> None:
    text = "## [Unreleased]\n\n" + HEADING + "\n- One complete change.\n\n\n"
    assert extract_release(text, "1.2.3") == "- One complete change.\n"


def test_indented_heading_is_not_release_change_text() -> None:
    text = "## [Unreleased]\n\n" + HEADING + "\n   # Not a change\n"
    with pytest.raises(
        ReleaseError,
        match="^" + re.escape("Release entry must contain change text") + "$",
    ):
        extract_release(text, "1.2.3")


def test_empty_release_body_cannot_be_satisfied_by_its_heading() -> None:
    text = "## [Unreleased]\n\n" + HEADING + "\n## [1.2.2] - 2026-09-19\n\n- Older\n"
    with pytest.raises(
        ReleaseError,
        match="^" + re.escape("Release entry must contain change text") + "$",
    ):
        extract_release(text, "1.2.3")


def test_fenced_headings_and_definitions_remain_literal() -> None:
    example = (
        "\n````markdown\n## [9.9.9] - 2026-09-20\n"
        "[literal]: not-a-footer\n```\n~~~~\n````\n"
    )
    text = DOCUMENT.replace(ENTRY, ENTRY + example)
    assert extract_release(text, "1.2.3") == BODY + example


def test_tilde_fences_and_missing_final_newline() -> None:
    example = "\n~~~\n## this is example text\n~~~~\n"
    text = ("## [Unreleased]\n\n" + ENTRY + example).rstrip("\n")
    assert extract_release(text, "1.2.3") == BODY + example


@pytest.mark.parametrize("opening", ["```", "~~~"])
def test_mismatched_fence_characters_do_not_close(opening: str) -> None:
    closing = "~~~" if opening[0] == "`" else "```"
    text = DOCUMENT.replace(ENTRY, ENTRY + "\n" + opening + "\n" + closing + "\n")
    with pytest.raises(
        ReleaseError, match="^" + re.escape("Unclosed fence in CHANGELOG.md") + "$"
    ):
        extract_release(text, "1.2.3")


def test_crlf_is_the_only_internal_newline_normalization() -> None:
    assert extract_release(DOCUMENT.replace("\n", "\r\n"), "1.2.3") == BODY


@pytest.mark.parametrize(
    "version", ["v1.2.3", "01.2.3", "1.2", "1.2.3rc1", "1.2.3+build"]
)
def test_only_supported_stable_versions(version: str) -> None:
    with pytest.raises(
        ReleaseError, match="^" + re.escape("Expected a stable version") + "$"
    ):
        extract_release(DOCUMENT, version)


@pytest.mark.parametrize(
    ("text", "message"),
    [
        ("", "Current release must follow Unreleased"),
        ("# Changelog\n", "Current release must follow Unreleased"),
        (ENTRY, "Current release must follow Unreleased"),
        (
            DOCUMENT.replace("## [Unreleased]", "## [Unreleased] - 2026-09-20"),
            "Unreleased must not have a date",
        ),
        (
            DOCUMENT.replace(HEADING.rstrip(), "## [1.2.3]"),
            "Release heading must have an ISO date",
        ),
        (
            DOCUMENT.replace("2026-09-20", "2026-02-30"),
            "Invalid release date in CHANGELOG.md",
        ),
        (DOCUMENT.replace("## [1.2.3]", "## [01.2.3]"), "Invalid stable version"),
        (
            DOCUMENT.replace("## [1.2.3]", "## 1.2.3"),
            "Invalid second-level changelog heading",
        ),
        (
            DOCUMENT.replace("## [1.2.3]", "## [1.2.4]"),
            "Current release must follow Unreleased",
        ),
        (DOCUMENT.replace(ENTRY, ENTRY + "\n" + ENTRY), "Duplicate changelog section"),
        (
            DOCUMENT.replace("- Preserve **exact** wording.  ", ""),
            "Release entry must contain change text",
        ),
        (
            DOCUMENT + "\n[1.2.3]: https://example.test/duplicate\n",
            "Duplicate changelog reference definition",
        ),
        (
            DOCUMENT + "\nUnexpected prose after the reference footer.\n",
            "Reference definitions must form one footer",
        ),
        (
            DOCUMENT + "\n```\nnot a definition\n```\n",
            "Reference footer contains non-definition content",
        ),
        (
            DOCUMENT.replace(ENTRY, ENTRY + "\n```unclosed\n"),
            "Unclosed fence in CHANGELOG.md",
        ),
        (
            DOCUMENT.replace(ENTRY, ENTRY + "\n```info`invalid\n"),
            "Invalid backtick fence in CHANGELOG.md",
        ),
        (DOCUMENT.replace("Changelog", "Change\rlog"), "Invalid changelog encoding"),
        (DOCUMENT.replace("Changelog", "Change\x00log"), "Invalid changelog encoding"),
    ],
)
def test_ambiguous_or_invalid_changelogs_fail(text: str, message: str) -> None:
    with pytest.raises(ReleaseError, match="^" + message + "$"):
        extract_release(text, "1.2.3")
