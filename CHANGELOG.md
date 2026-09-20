# Changelog

Notable changes to this project are documented in this file. The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.1.0/), and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [Unreleased]

## [3.0.5] - 2026-09-20

### Changed

- Replaced maintained per-release GitHub note files with a changelog-bound,
  immutable release publisher. GitHub release bodies now contain the exact
  categorized Markdown beneath the tagged version heading.

### Fixed

- Validate tag targets, draft metadata, verified asset identities, publication
  read-back, and concurrent-writer reconciliation before public publication.

## [3.0.4] - 2026-09-20

### Fixed

- Re-reviewed and rebound every source-location-sensitive equivalent mutant after
  the v3.0.3 tagged mutation campaign updated report-spool iterator ownership.

## [3.0.3] - 2026-09-19

### Fixed

- Corrected the source-bound equivalence-manifest digest used by the immutable
  corrective release qualification, after the final cross-platform report-spool
  hardening changes.

## [3.0.2] - 2026-09-14

### Fixed

- Corrected the portable Darwin descriptor-address qualification contract so every
  supported platform proves the exact native address query before release.
- Re-opened each reported final address literally through the native no-follow path
  before a copy can be reported usable.
- Made generated Hypothesis evidence—including related-root and RFC 2231 continuation
  contracts—a six-platform pull-request and tagged-release prerequisite.
- Streamed terminal reports through bounded private spools, with a pre-reserved
  complete status-recovery path for a post-publication report-spool failure.
- Made the Finder Quick Action show every terminal category and bounded safe details
  instead of silently hiding failed or mixed processing.

## [3.0.1] - 2026-09-14

### Fixed

- Hardened MIME comment, RFC 2231, related-resource, and independent verifier
  parsing against malformed state and ambiguous ownership.
- Strengthened native publication receipts, existing-output verification, and
  generated-property evidence; macOS Finder guidance now requires a visible
  Quick Action result.
- Added bounded, process-isolated mutation execution and complete behavioral
  qualification for every actionable mutant.

## [3.0.0] - 2026-09-09

### Security

- **Breaking:** Replaced the v2 text-only projection with source-bound MIME pruning.
  v3 preserves retained `text/plain` and `text/html` payloads and their source order;
  it removes only exact MIME attachments and non-root `multipart/related`
  components, rejecting ambiguous roles instead of guessing.
- Added raw-span candidate construction, source/candidate payload fingerprints,
  independent reparse verification, schema-3 batch ledger reports, and atomic
  no-replace output publication.
- Removed HTML/CSS parsing, semantic-equivalence projection, `--force`,
  `--skip-existing`, newline `paths`, the `.text-only.eml` suffix, and all v2 report
  compatibility behavior. Regenerate derived copies from unchanged originals.
- Retained HTML is explicitly not sanitized. Related-resource references can become
  unresolved and are reported rather than silently rewritten.

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

[Unreleased]: https://github.com/resoltico/EMLAttachmentRemover/compare/v3.0.4...HEAD
[3.0.4]: https://github.com/resoltico/EMLAttachmentRemover/compare/v3.0.3...v3.0.4
[3.0.3]: https://github.com/resoltico/EMLAttachmentRemover/compare/v3.0.2...v3.0.3
[3.0.2]: https://github.com/resoltico/EMLAttachmentRemover/compare/v3.0.1...v3.0.2
[3.0.1]: https://github.com/resoltico/EMLAttachmentRemover/compare/v3.0.0...v3.0.1
[3.0.0]: https://github.com/resoltico/EMLAttachmentRemover/compare/v2.0.1...v3.0.0
[2.0.1]: https://github.com/resoltico/EMLAttachmentRemover/compare/v2.0.0...v2.0.1
[2.0.0]: https://github.com/resoltico/EMLAttachmentRemover/compare/v1.0.0...v2.0.0
[1.0.0]: https://github.com/resoltico/EMLAttachmentRemover/releases/tag/v1.0.0
