# Changelog

Notable changes to this project are documented in this file. The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.1.0/), and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [Unreleased]

## [4.0.0] - 2026-10-01

### Added

- A native macOS app accepts files directly or through Finder and shows progress, per-file outcomes, expandable diagnostics and Finder actions. `Stop processing` interrupts a batch without undoing completed copies; `Done` closes its report. Forced termination stops the app's owned processing runs. The app bundles the processor, includes original MIT-licensed graphics, shows version/copyright information in About, and exposes the full MIT license in the app menu. Corrupt UTF-8 processing receipts are rejected before presentation. The app accepts up to 4,095 file paths per launch; long paths can require a smaller selection. Conflict cards use their actual result label, including cases without an existing copy. Oversized selections receive launch-size guidance, and startup failures use structured error kinds rather than diagnostic message matching. Build and installation instructions are included.
- GitHub releases include separate Apple Silicon and Intel macOS app ZIPs requiring macOS 14 or later and external CPython 3.14. Choose the ZIP matching your Mac; installation refuses the other CPU. Apps are ad-hoc signed, without Developer ID or notarization; Gatekeeper may require an app-specific exception.
- The app installer privately records the selected CPython interpreter for Finder launches and retains the previous app bundle. Quit before updating: installation is refused while the app is running. A missing or non-executable selected interpreter is refused instead of silently using another interpreter from automatic discovery.
- `uv run python tools/tasks.py ci` reproduces host-applicable quality, release-build, archive-verification, mutation and randomized property checks with standard and free-threaded CPython. On macOS, mutation testing uses the Linux runner image through Docker. GitHub publication and attestations remain CI-only operations.
- Native reports retain validated completed results when CPython exits 120 after buffered-stream finalization fails, explain the unsuccessful invocation and disable copy-reveal actions. A complete report's processing `exit_code` can differ from process status; automation must treat 120 as unsuccessful. See [Python's exit contract](https://docs.python.org/3.14/library/sys.html#sys.exit).

### Changed

- **Breaking for status consumers:** an explicitly unsupported atomic publication operation on POSIX (`ENOTSUP`/`EOPNOTSUPP`) or Windows (`STATUS_NOT_SUPPORTED`) now reports `ATOMIC_PUBLICATION_UNSUPPORTED` and single-input process status 11 instead of `WRITE_ERROR`/7. Mixed batches retain status 9. The app explains the destination limitation and how to process copies on a compatible local volume; publication remains atomic and never replaces existing files.
- Unexpected ordinary exceptions confined to MIME preparation after source binding produce a per-file `INTERNAL_ERROR` while later inputs continue. The invocation remains unsuccessful; explicit invariant failures, memory exhaustion and exceptions in shared backend, report-admission or publication state still stop the batch.
- A single trailing parameter separator is accepted in structured MIME headers; empty internal or repeated trailing segments remain refused. Parse diagnostics identify the affected content field and MIME location. Supported files retain the 128 MiB raw-source bound without a separate 96 MiB decoded-retention cap; this does not bound peak process memory.
- Distribution files and archived files/directories carry the build invocation datetime instead of fixed historical timestamps. Independent build comparisons share that datetime; separate invocations can produce different checksums.
- **Breaking for Finder automation:** Quick Actions only launch the native app and finish at launch, not processing completion. Each batch has its own report window; leaving it open does not hold the shortcut open. Integrations requiring completion or exit status must use the CLI installed from the wheel or the standalone zipapp.
- **Breaking for human-output parsers:** success and dry-run lines include the final or planned destination, for example `created: source.eml -> /path/to/source.mime-pruned.eml`. Parsers of the former source-only lines should use schema-3 JSON or NUL-delimited `paths0`.
- Automatically derived names exceeding the destination filesystem's filename limit use a readable prefix, stable digest of the complete source name and `.mime-pruned.eml`. Explicit `--output` names are never changed. Use the reported destination to locate derived copies.
- **Compatibility:** inputs exceeding the 1 MiB per-record evidence limit or available 64 MiB report capacity are refused with `PARSE_ERROR` before publication. Batches reserve capacity for later inputs' terminal records.
- **Breaking for previously accepted longer paths:** publication is refused when the resolved final address cannot be obtained or exceeds 4,096 native units: POSIX bytes or Windows UTF-16 code units. If the length limit is reached, use a shorter resolved destination path.
- **Compatibility:** error messages are bounded to 2,048 characters and 12,288 canonical ASCII JSON content bytes, including the truncation marker. Allow truncated messages and interpret outcomes using the error code and publication receipt.
- Large MIME payloads and folded headers need less scanning and copying, without changing delimiter matching or display escaping. Human-output runs defer packaging-version lookup unless the version is requested.
- Source builds of the native app require full Xcode 26 or later to compile the layered icon, plus the checksum-pinned official Swift.org compiler. Production, model-test and fuzz builds use that compiler; prebuilt app users need no build toolchain.

### Removed

- **Breaking:** removed the shell-command text-report Quick Action, its presentation switches and dedicated installer/uninstaller. Use the wheel-installed CLI or standalone zipapp for synchronous automation; the app installer installs only the app and runtime selection.

### Fixed

- Unterminated MIME comments ending in a complete escape pair are refused as parse errors rather than crashing.
- Windows source directories no longer require write access; destination handles retain the permissions needed for publication and durability. Directory-sync error classification uses platform errno names rather than values borrowed from different operating systems.
- MIME parsing accepts physical optional header names with RFC 5322 punctuation, root messages containing complete headers and an empty body, and RFC 2231 structural parameters such as `boundary*=` and `boundary*0*=`. Extended filename parameters may have empty charsets; structural values must decode to printable ASCII. Ambiguous or malformed MIME remains refused.
- Parts with an empty header block exclude the separator line from retained payloads and payload hashes; only body octets are retained and fingerprinted.
- Usage, report-storage startup, processing-error and interruption reports preserve the selected output format and dry-run mode. Repeated output-format options use the final effective known choice; arguments after `--` remain filenames.
- Report-storage failures preserve known publication outcomes through bounded memory recovery, independently of spool reads or writable storage. Recovery starts only before stdout accepts its first report byte; partial documents never receive a replacement. Terminal/publication receipts are retained, but detailed MIME evidence and warnings may be omitted. Unavailable output endpoints can still prevent delivery.
- JSON and recovery JSON remain ASCII on non-UTF-8 stdout; `paths0` preserves exact native POSIX path bytes through sealing and delivery. Human output and diagnostics escape unsafe or unrepresentable text, including dry-run destinations, without splitting an item across physical lines.
- Short writes and nonblocking stalls retain unwritten output. Closed Windows report pipes select ordinary broken-output status 1, even when the runtime reports an invalid-argument error.
- Interruption preserves completed publication receipts without exposing unfinished terminal rows: `created`, `existing_verified` and genuine `published_with_error` outcomes retain their meanings, with invocation interruption recorded separately. Human and `paths0` reports explain batch failure/interruption on stderr without changing accepted stdout paths or duplicating notices.
- Cancellation remains effective during blocked report or interruption-notice output. The first signal permits 10 seconds without accepted-output progress; a repeated signal terminates without that grace. A signal after a complete report preserves its bytes and selects status 130 with a bounded notice, including JSON error finalization. CPython shutdown failures can subsequently change the observed exit to 120.
- `process_file()` returns a detached, bounded result containing removal, payload-hash, candidate, verification, warning and publication evidence, and closes internal report storage.
- Shutdown-time cooperative interruption returns the completed ledger with interruption metadata. Controller setup and shutdown release acquired state independently of other cleanup failures and preserve the primary error, avoiding stale invocation state in later calls.
- Packaged installations and zipapps accept a temporary directory beside the installation; only source checkouts exclude their own tree from report temporary storage.

### Security

- Private receipts remain in owned temporary handles: anonymous or unlinked-open storage on POSIX, delete-on-close storage on Windows. Storage is not reopened by name, and its lifetime does not depend on Python cleanup after forced termination.

### Internal

- Bounded byte mutations and seed splices exercise the full Python MIME pipeline alongside structured properties; signal tests use a controlled baseline while retaining explicit ignored-signal cases. Test modules are named by the contracts they verify.
- Delivery uses a versioned macOS runner and verifies an explicitly selected Xcode version/build; those labels still receive operating-system updates. Report serialization has one shared internal interface, and duplicate source fingerprint work was removed while preserving source/candidate and independent policy checks.
- Native receipt and path handling share a local/CI libFuzzer target with AddressSanitizer, raw and structured inputs, presentation invariants and deliberate failure controls. Fresh private campaign directories preserve source snapshots, stage outcomes and failure artifacts; bounded stages clean owned processes on timeout or interruption. Model contracts also run with production optimization, and a heap-overflow control verifies sanitizer detection.
- Swift formatting, lint, size and complexity gates use pinned tools and an exception registry shared with Python; Python method-size checks include nested classes. Native presentation, process ownership, runtime configuration and application lifecycle are separated for review and verification.
- macOS CI builds and ad-hoc signs separate single-CPU apps alongside portable CLI assets. Publication requires the complete checksummed set, reverifies both extracted apps, icon resources, source-bound installer/documentation files and bundled processor, and exercises the tagged archives on macOS 14 Apple Silicon, macOS 15 Intel and macOS 27 Apple Silicon. The release pipeline and audit contract are documented.
- Setup UV v10.2.0 uses safer cache defaults; Linux base images are digest-pinned. Signal-boundary tests exercise mutation-selected implementations. Standard/free-threaded CPython qualification remains pinned to 3.14.7 until the pinned UV installer catalog supports the required 3.14.8 builds.
- `quality --native` runs host-dependent checks separately. Shared static checks type-check Linux, macOS and Windows targets; tests check that supported workflow steps have local equivalents.
- On POSIX, task timeouts and interruptions stop owned process groups; mutation workers clean per-session temporary storage. Campaigns hold an exclusive checkout lease, verify source-bound equivalence manifests, support configurable worker counts and retain diagnostics for non-killed mutants.
- Release qualification disables child-process bytecode writes, including direct invocations. Coverage XML survives test or threshold failures. CI preserves release-quality reports and distinct rerun artifacts, cancels superseded pull-request exploration, and sanitizes Hypothesis observations for reported and resolved home, interpreter and project paths, including symlinks.

## [3.0.6] - 2026-09-21

### Changed

- Unsupported legacy CLI options receive the standard current parser error instead of migration guidance.

### Fixed

- Made report path evidence and machine-channel serialization independent of host text codecs, preserving native-only POSIX addresses through reporting and Finder.
- Preserved post-publication outcomes through report failures, made fail-fast honor unsuccessful publication, and classified destination environmental failures as item-local write errors.
- Added narrow root Unix-From envelope compatibility without changing MIME removal authorization, retained payload bytes, or nested/body `From ` handling.

## [3.0.5] - 2026-09-20

### Changed

- GitHub release notes come exclusively from the categorized Markdown beneath the tagged changelog version heading, using an immutable publisher instead of maintained per-release note files.

### Fixed

- Release publication validates tag targets, draft metadata, verified asset identities, publication read-back and concurrent-writer reconciliation before making a release public.

## [3.0.4] - 2026-09-20

This normal release includes the corrections recorded under v3.0.1–v3.0.3, whose release workflows stopped before asset publication.

### Fixed

- Corrected MIME-comment and RFC 2231 parsing, related-resource ownership and removal authorization, with independent candidate verification.
- Strengthened publication-address and existing-output verification. Reported final addresses are reopened literally through the native no-follow path before a copy is reported usable.
- Terminal reports use bounded private spools with a reserved recovery path for post-publication spool failures. Finder Quick Actions show all terminal categories and bounded safe details, including failed or mixed results.

### Internal

- Included the generated-property and mutation qualification accompanying these corrections, including cross-platform native-address qualification.
- Corrected source-bound equivalent-mutant evidence used for release qualification. This verification correction does not change EML processing.

## [3.0.3] - 2026-09-19

This tagged candidate's release workflow stopped before asset publication; its changes are included in the normal v3.0.4 release.

### Internal

- Corrected the source-bound equivalence-manifest digest used for corrective release qualification after cross-platform report-spool hardening.

## [3.0.2] - 2026-09-14

This tagged candidate's release workflow stopped before asset publication; its changes are included in the normal v3.0.4 release.

### Fixed

- Re-opened each reported final address literally through the native no-follow path before a copy can be reported usable.
- Streamed terminal reports through bounded private spools, with a pre-reserved complete status-recovery path for a post-publication report-spool failure.
- Finder Quick Actions show every terminal category and bounded safe details instead of hiding failed or mixed processing.

### Internal

- Corrected the portable Darwin descriptor-address qualification contract so every supported platform proves the exact native address query before release.
- Made generated Hypothesis evidence—including related-root and RFC 2231 continuation contracts—a six-platform pull-request and tagged-release prerequisite.

## [3.0.1] - 2026-09-14

This tagged candidate's release workflow stopped before asset publication; its changes are included in the normal v3.0.4 release.

### Fixed

- Hardened MIME-comment, RFC 2231, related-resource and independent-verifier parsing against malformed state and ambiguous ownership.
- Strengthened native publication receipts and existing-output verification. macOS Finder guidance requires a visible Quick Action result.

### Internal

- Expanded generated-property evidence for the publication and verification corrections.
- Added bounded, process-isolated mutation execution and complete behavioral qualification for every actionable mutant.

## [3.0.0] - 2026-09-13

### Added

- Added raw-span candidate construction, source/candidate payload fingerprints, independent reparse verification, schema-3 batch ledger reports, and atomic no-replace output publication.

### Changed

- **Breaking:** Replaced the v2 text-only projection with source-bound MIME pruning. v3 preserves retained `text/plain` and `text/html` payloads and their source order; it removes only exact MIME attachments and non-root `multipart/related` components, rejecting ambiguous roles instead of guessing.

### Removed

- **Breaking:** removed HTML/CSS parsing, semantic-equivalence projection, `--force`, `--skip-existing`, newline `paths`, the `.text-only.eml` suffix and all v2 report compatibility behaviour. Regenerate derived copies from unchanged originals.

### Security

- Retained HTML is explicitly not sanitized. Related-resource references can become unresolved and are reported rather than silently rewritten.

## [2.0.1] - 2026-08-20

### Fixed

- Equivalent plain/HTML alternatives now retain readable paragraph, list, and quotation boundaries in the canonical text-only output. The selected plain body remains the source-content precondition; HTML is used only as a local formatting projection after exact non-whitespace text equality is proven.

## [2.0.0] - 2026-08-20

### Changed

- **Breaking:** The sole output contract now creates a verified text-only EML working copy by selecting a safe plain-text body, discarding unselected body representations with their dependent resources, and removing ordinary attachments elsewhere.
- **Breaking:** Default output names now end in `.text-only.eml`.
- **Breaking:** Machine-readable reports now use schema version 2 and distinguish selected plain-text bodies, discarded body representations, discarded body resources, and removed ordinary attachments.
- **Breaking:** Processing now fails without publishing an output when a verified text-only representation cannot be produced safely. Explicit attachment or unselected protected subtrees may be discarded atomically; protected content needed by the selected body is never guessed at. There is no v1 compatibility mode.
- The Finder Quick Action is now named **Create Text-Only EML Copy** and reports created outputs separately from unverified existing outputs that were skipped.

## [1.0.0] - 2026-08-15

- First public release.

[Unreleased]: https://github.com/resoltico/EMLAttachmentRemover/compare/v4.0.0...HEAD
[4.0.0]: https://github.com/resoltico/EMLAttachmentRemover/compare/v3.0.6...v4.0.0
[3.0.6]: https://github.com/resoltico/EMLAttachmentRemover/compare/v3.0.5...v3.0.6
[3.0.5]: https://github.com/resoltico/EMLAttachmentRemover/compare/v3.0.4...v3.0.5
[3.0.4]: https://github.com/resoltico/EMLAttachmentRemover/compare/v3.0.0...v3.0.4
[3.0.3]: https://github.com/resoltico/EMLAttachmentRemover/compare/v3.0.2...v3.0.3
[3.0.2]: https://github.com/resoltico/EMLAttachmentRemover/compare/v3.0.1...v3.0.2
[3.0.1]: https://github.com/resoltico/EMLAttachmentRemover/compare/v3.0.0...v3.0.1
[3.0.0]: https://github.com/resoltico/EMLAttachmentRemover/compare/v2.0.1...v3.0.0
[2.0.1]: https://github.com/resoltico/EMLAttachmentRemover/compare/v2.0.0...v2.0.1
[2.0.0]: https://github.com/resoltico/EMLAttachmentRemover/compare/v1.0.0...v2.0.0
[1.0.0]: https://github.com/resoltico/EMLAttachmentRemover/releases/tag/v1.0.0
