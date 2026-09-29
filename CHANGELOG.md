# Changelog

Notable changes to this project are documented in this file. The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.1.0/), and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [Unreleased]

### Changed

- Human output names each copy's destination
  (`created: source.eml -> /path/to/source.mime-pruned.eml`); dry runs show the
  planned destination.

### Added

- `tasks.py ci` runs every CI gate the host can reproduce (both interpreters,
  cross-platform type checks, release build and verification, mutation, randomized
  exploration), and a test fails when a workflow step has no local counterpart.
  On macOS, mutation runs in CI's Linux runner image through Docker, because mutant
  IDs and the reviewed equivalents are platform-specific.
- Mutation campaigns check the equivalence-manifest source binding before any
  work; `--allow-stale-manifest` and `check_mutation_results.py --source-sha256`
  document the rebind flow. `--workers` makes Mutmut parallelism benchmarkable.

- A mutation campaign takes an exclusive lease on its checkout
  (`build/mutation.lock`); a second concurrent run fails at once and names the
  holder. Every campaign, successful or not, leaves `build/mutation-diagnostics/`
  (each non-killed mutant with its status, mapped tests, and a bounded patch), which
  the mutation workflow uploads.
- `tasks.py quality --native` runs only the host-dependent checks (repository audit
  and coverage); CI runs the platform-independent static checks on one lane instead
  of six, with every required check name unchanged.

### Fixed

- A failure while flushing, rewinding, or cleaning up after a staged report can no
  longer erase it: every step that can fail runs before the first byte reaches a
  channel (falling back to the reserved status records), spool cleanup cannot
  consume the report, and cleanup failures are diagnosed after delivery.
- A reader that stops draining the report cannot make termination signals
  ineffective: delivery runs under a progress-bounded grace (10 s after the first
  signal) and a repeated signal ends the process at once, with status 130.
- A signal during delivery leaves the complete document unchanged and exits 130;
  the Finder launcher accepts exactly that disagreement between a complete report
  and status 130 and keeps the run's receipts, instead of rejecting the report.
- Report admission reserves a record's exact terminal form (a worst-case publication
  receipt with the longest reportable final address, status, and error) and every
  later input's minimal record before publication, instead of estimating; the
  destination address bound (4,096 units) is checked when the destination is bound,
  so a long absolute address can no longer turn a created copy into
  `published_with_error`. Error messages are capped at 2,048 characters.
- Dry-run destinations are built in the backend's native path grammar and escaped
  once, so a newline or control character in a name no longer breaks the line and a
  trailing backslash is an ordinary POSIX name character; all human diagnostics
  are escaped the same way.
- Recovery reports are staged and delivered as canonical ASCII bytes like every other
  JSON report, whatever the terminal's text encoding.
- `paths0` no longer omits a created copy whose name has no Unicode text; it emits
  the exact native path bytes, as documented.
- Human output on a non-UTF-8 terminal no longer fails after creating a copy; text
  is escaped for the terminal's actual encoding.
- An interrupted or failed report keeps the requested format and mode (a `paths0`
  or dry-run request no longer turns into human or apply output), is never followed
  by a second document, and still reports published items if report staging fails.
- A zipapp or installed package accepts a temporary directory beside it; only a
  source checkout keeps temporary storage out of its tree.
- Reported payload hashes for parts with an empty header block no longer include the
  separator line: the hash covers exactly the body octets.
- RFC 2231 extended parameters work as structural controls: `boundary*=`,
  `boundary*0*=` continuations and an empty charset (`filename*=''name`) are
  accepted, each encoded segment decoded exactly once, while reported evidence keeps
  the exact wire spelling. A decoded boundary must be printable ASCII.
- A root message consisting only of complete header fields is accepted as a message
  with an empty body; nested parts still require their separator.
- Folded headers are accumulated in linear time (128 KiB: about 0.12 s to about
  0.01 s).
- A derived output name that would exceed the destination directory's filename limit
  is shortened with a readable prefix and a stable digest of the full source name,
  instead of failing after the source was read. Explicit `--output` names are never
  renamed.
- A message whose report evidence cannot fit a report record (about 3,700 retained
  MIME parts) or the remaining report capacity is refused with `PARSE_ERROR` before
  any copy is written, instead of creating a copy and then reporting
  `published_with_error`.
- Task timeouts and interruptions stop the whole process group, so no test or
  mutant descendant can outlive its task step and write after evidence capture.
- Mutation workers remove each pytest session's temporary directory, keeping disk
  use bounded by live workers rather than completed mutants.
- Coverage XML is published when tests fail or the threshold is missed.
- Superseded pull-request exploration runs are cancelled; release qualification
  keeps its quality reports; artifact names survive workflow re-runs.
- Hypothesis observation finalization replaces both the reported and the resolved
  spelling of the home, Python prefix, and project paths, so an interpreter or home
  under a symbolic link (such as macOS `/var`) no longer fails publication.

## [3.0.6] - 2026-09-21

### Changed

- Updated the pinned build and qualification tools: Hatchling 1.32.4, Coverage
  7.16.1, Hypothesis 6.168.0, Mutmut 3.8.0, and Ruff 0.16.8.
- Removed the legacy CLI migration layer; unsupported options now receive the
  standard current-surface parser error with no compatibility guidance.

### Fixed

- Made report path evidence and machine-channel serialization independent of host
  text codecs, preserving native-only POSIX addresses through reporting and Finder.
- Preserved post-publication outcomes through report failures, made fail-fast honor
  unsuccessful publication, and classified destination environmental failures as
  item-local write errors.
- Added narrow root Unix-From envelope compatibility without changing MIME removal
  authorization, retained payload bytes, or nested/body `From ` handling.

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
