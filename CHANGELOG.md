# Changelog

Notable changes to this project are documented in this file. The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.1.0/), and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [Unreleased]

## [2.0.1] - 2026-08-20

### Fixed

- Equivalent plain/HTML alternatives now retain readable paragraph, list, and
  quotation boundaries in the canonical text-only output. The selected plain
  body remains the source-content precondition; HTML is used only as a local
  formatting projection after exact non-whitespace text equality is proven.

## [2.0.0] - 2026-08-17

### Changed

- **Breaking:** The sole output contract now creates a verified text-only EML
  working copy by selecting a safe plain-text body, discarding unselected body
  representations with their dependent resources, and removing ordinary
  attachments elsewhere.
- **Breaking:** Default output names now end in `.text-only.eml`.
- **Breaking:** Machine-readable reports now use schema version 2 and distinguish
  selected plain-text bodies, discarded body representations, discarded body
  resources, and removed ordinary attachments.
- **Breaking:** Processing now fails without publishing an output when a verified
  text-only representation cannot be produced safely. Explicit attachment or
  unselected protected subtrees may be discarded atomically; protected content
  needed by the selected body is never guessed at. There is no v1 compatibility
  mode.
- The Finder Quick Action is now named **Create Text-Only EML Copy** and reports
  created outputs separately from unverified existing outputs that were skipped.

## [1.0.0] - 2026-08-15

- First public release.

[Unreleased]: https://github.com/resoltico/EMLAttachmentRemover/compare/v2.0.1...HEAD
[2.0.1]: https://github.com/resoltico/EMLAttachmentRemover/compare/v2.0.0...v2.0.1
[2.0.0]: https://github.com/resoltico/EMLAttachmentRemover/compare/v1.0.0...v2.0.0
[1.0.0]: https://github.com/resoltico/EMLAttachmentRemover/releases/tag/v1.0.0
