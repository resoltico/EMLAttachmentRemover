# Changelog

Notable changes to this project are documented in this file. The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.1.0/), and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [Unreleased]

## [4.0.0] - 2026-10-01


### Added

- A native macOS application with original, MIT-licensed project graphics accepts files directly or through Finder and presents processing progress and final reports with the app name and actual processor version, per-file outcomes, expandable diagnostics, and Finder actions. Stop processing interrupts an active batch without undoing completed copies; Done closes a finished report. Forced app termination also stops its owned processing runs. The universal app bundles the processor and requires CPython 3.14 at runtime; build and installation instructions are included.
- GitHub releases include a prebuilt universal macOS application ZIP. It requires macOS 14 or later and an external CPython 3.14 runtime, and is ad-hoc signed without Developer ID or notarization; Gatekeeper may require an app-specific exception.
- The native app installer records the selected CPython interpreter privately for Finder launches and retains the previous app bundle. Quit the app before updating it; a running installation is refused.
- Maintainers can run `uv run python tools/tasks.py ci` to reproduce the host-applicable quality, release-build, archive-verification, mutation, and randomized property-exploration checks with both standard and free-threaded CPython. On macOS, mutation testing uses the Linux runner image through Docker; GitHub publication and attestations remain CI operations.

### Changed

- **Compatibility when migrating the Quick Action:** Finder users can migrate their Quick Action to the native app launch command and remove Show Content. Each batch has its own report window, and an open report does not hold the shortcut open. The shortcut now finishes at application launch; integrations needing processing completion or exit status must continue to use the direct launcher or zipapp.
- **Breaking for parsers of human output:** successful and dry-run lines now include the final or planned destination, for example `created: source.eml -> /path/to/source.mime-pruned.eml`. Integrations that parse the former source-only lines should use schema-3 JSON or NUL-delimited `paths0` instead.
- Automatically derived output names that exceed the destination filesystem's filename limit are shortened to a readable prefix, a stable digest of the complete source name, and `.mime-pruned.eml`. Explicit `--output` names are never changed; callers locating derived copies should use the reported destination.
- **Compatibility:** report capacity is checked before publication. Inputs whose evidence exceeds the 1 MiB per-record limit or available 64 MiB report capacity are refused with `PARSE_ERROR` before a copy is written; the batch also reserves space for later inputs' terminal records.
- **Breaking for previously accepted longer paths:** destinations are refused before publication if their resolved final address cannot be obtained or exceeds 4,096 native units (POSIX bytes or Windows UTF-16 code units). Use a shorter resolved destination path if this limit is reached.
- **Compatibility:** error messages are bounded to 2,048 characters and 12,288 canonical ASCII JSON content bytes, including the truncation marker. Consumers must allow truncated messages and use the error code and publication receipt to interpret the outcome.
- Large MIME payloads and folded headers require less scanning and copying; human-output runs defer packaging-version lookup unless the version is requested. The scanning optimizations preserve delimiter matching and display escaping behavior.
- The Finder launcher preserves validated complete reports when CPython exits 120 after buffered-stream finalization fails, explains the failure, retains exit 120, and suppresses reveal actions. Integrators must treat 120 as unsuccessful even if a complete report describes successful copies; the report's processing `exit_code` can differ from the final process status. See [Python's exit contract](https://docs.python.org/3.14/library/sys.html#sys.exit).

### Fixed

- Accept physical optional header names containing RFC 5322 punctuation, root messages consisting only of complete header fields with an empty body, and RFC 2231 extended structural parameters such as `boundary*=` and `boundary*0*=` continuations. Empty charsets in extended filename parameters are accepted; structural parameter values must decode to printable ASCII, and ambiguous or malformed MIME remains refused.
- Parts with an empty header block no longer include the separator line in their retained payload or reported payload hash. Only body octets are retained and fingerprinted.
- JSON usage and report-storage startup errors retain the requested JSON selection and dry-run mode. Repeated output-format options use the final effective known choice, and arguments after `--` remain filenames. Processing-error and interruption reports retain the selected output format and mode.
- Report staging, sealing, and reads preserve known publication outcomes through storage failures, including persistent failures, using bounded memory recovery independently of spool reads and writable storage. Recovery can restart only before stdout accepts its first report byte; partial documents are never followed by a replacement, and unavailable output endpoints can still prevent delivery. Recovery retains terminal/publication receipts but may omit detailed MIME evidence and warnings.
- JSON and recovery JSON remain ASCII on non-UTF-8 stdout, and `paths0` preserves exact native POSIX path bytes through sealing and delivery. Human output and diagnostics escape terminal-unsafe or unrepresentable text, including dry-run destinations, without splitting one item across physical lines. Short writes and nonblocking stalls retain unwritten output instead of dropping it. Closed report pipes on Windows select the ordinary broken-output status 1 even when the Windows runtime reports an invalid-argument error.
- Interruption no longer discards completed publication receipts or exposes unfinished terminal rows. Completed creations remain `created`, verified existing outputs remain `existing_verified`, and genuine post-publication failures remain `published_with_error`, with invocation interruption recorded separately. Human and `paths0` reports explain batch failure and interruption on stderr without changing accepted stdout paths or repeating the same notice.
- Cancellation remains effective while report output or an interruption notice is blocked: after the first signal, delivery allows 10 seconds without accepted-output progress; a repeated signal terminates without waiting for that grace. A signal after a complete report preserves its bytes and selects status 130 with a bounded notice, including JSON error-response finalization. CPython shutdown failures can subsequently change the observed exit to 120.
- `process_file()` returns a detached, bounded result with removal, payload-hash, candidate, verification, warning, and publication evidence, and closes its internal report storage. Shutdown-time cooperative interruption returns the completed ledger with interruption metadata; controller setup and shutdown release acquired state independently of other cleanup failures and preserve the primary error, avoiding stale invocation state in later calls.
- Cancelling the Finder launcher still validates and displays available complete results, suppresses reveal, and preserves the launcher's signal status. Its processor has at most 12 seconds to finish, and a repeated launcher signal kills it immediately; invalid or truncated reports remain rejected. Legacy Shortcuts text presentation retains the `|| :` transport so failed and mixed batches can reach Show Content.
- Packaged installations and zipapps accept a temporary directory beside the installation; only source checkouts exclude their own tree from report temporary storage.

### Security

- Private receipt storage stays in owned temporary handles rather than reopening named files: anonymous or unlinked-open storage on POSIX and delete-on-close storage on Windows. Its lifetime no longer depends on Python cleanup running after forced termination.

### Internal

- Swift source now has pinned strict formatting and lint gates, structural size and complexity checks, and a central exception approval registry shared with Python. Python method-size checks also cover nested classes. Native presentation, process ownership, runtime configuration, and application lifecycle are separated for review and verification.

- macOS CI produces and ad-hoc signs the universal native application together with the portable CLI assets. Release publication requires the complete checksummed set and reverifies the extracted native app, both architectures, source-bound installer/documentation files, and bundled processor. The release pipeline and audit contract are documented.
- Updated the verification toolchain to UV 0.12.21, Actionlint-py 1.7.12.25, Coverage 7.16.2, Hypothesis 6.168.3, Pyflakes 4.0.1, Ruff 0.16.9, and refreshed transitive dependencies. GitHub Setup UV is pinned to v10.2.0 with its safer cache defaults; local Linux base images are pinned by digest. Signal-boundary tests follow mutation dispatch so campaigns exercise the selected implementation. The installable standard/free-threaded CPython qualification pins remain 3.14.7 pending provider availability of 3.14.8 builds.
- Quality tasks can run host-dependent checks separately with `quality --native`; shared static checks explicitly type-check Linux, macOS, and Windows targets. Tests check that supported workflow steps have local equivalents.
- Task timeouts and interruptions stop owned process groups; mutation workers clean each pytest session's temporary storage. Mutation campaigns take an exclusive checkout lease, verify the equivalence manifest's source binding, support configurable worker counts, and retain diagnostics for non-killed mutants.
- Coverage XML is retained when tests or coverage thresholds fail. CI preserves release quality reports and distinct artifacts across reruns, cancels superseded pull-request exploration, and sanitizes Hypothesis observations for both reported and resolved home, interpreter, and project paths, including symlinked paths.

## [3.0.6] - 2026-09-21

### Changed

- Updated the pinned build and qualification tools: Hatchling 1.32.4, Coverage 7.16.1, Hypothesis 6.168.0, Mutmut 3.8.0, and Ruff 0.16.8.
- Removed the legacy CLI migration layer; unsupported options now receive the standard current-surface parser error with no compatibility guidance.

### Fixed

- Made report path evidence and machine-channel serialization independent of host text codecs, preserving native-only POSIX addresses through reporting and Finder.
- Preserved post-publication outcomes through report failures, made fail-fast honor unsuccessful publication, and classified destination environmental failures as item-local write errors.
- Added narrow root Unix-From envelope compatibility without changing MIME removal authorization, retained payload bytes, or nested/body `From ` handling.

## [3.0.5] - 2026-09-20

### Changed

- Replaced maintained per-release GitHub note files with a changelog-bound, immutable release publisher. GitHub release bodies now contain the exact categorized Markdown beneath the tagged version heading.

### Fixed

- Validate tag targets, draft metadata, verified asset identities, publication read-back, and concurrent-writer reconciliation before public publication.

## [3.0.4] - 2026-09-20

### Fixed

- Includes the MIME-comment, RFC 2231, related-resource, removal-authorization, publication-address, existing-output, bounded report-storage/recovery, and visible Finder mixed-result corrections recorded under the v3.0.1 and v3.0.2 candidates, together with their qualification work.
- Re-reviewed and rebound source-location-sensitive equivalent mutants after the v3.0.3 tagged qualification failed. This corrected release qualification rather than EML processing.

## [3.0.3] - 2026-09-19

This tagged candidate's release workflow stopped before asset publication; its changes are included in the normal v3.0.4 release.

### Fixed

- Corrected the source-bound equivalence-manifest digest used by the immutable corrective release qualification, after the final cross-platform report-spool hardening changes.

## [3.0.2] - 2026-09-14

This tagged candidate's release workflow stopped before asset publication; its changes are included in the normal v3.0.4 release.

### Fixed

- Corrected the portable Darwin descriptor-address qualification contract so every supported platform proves the exact native address query before release.
- Re-opened each reported final address literally through the native no-follow path before a copy can be reported usable.
- Made generated Hypothesis evidence—including related-root and RFC 2231 continuation contracts—a six-platform pull-request and tagged-release prerequisite.
- Streamed terminal reports through bounded private spools, with a pre-reserved complete status-recovery path for a post-publication report-spool failure.
- Made the Finder Quick Action show every terminal category and bounded safe details instead of silently hiding failed or mixed processing.

## [3.0.1] - 2026-09-14

This tagged candidate's release workflow stopped before asset publication; its changes are included in the normal v3.0.4 release.

### Fixed

- Hardened MIME comment, RFC 2231, related-resource, and independent verifier parsing against malformed state and ambiguous ownership.
- Strengthened native publication receipts, existing-output verification, and generated-property evidence; macOS Finder guidance now requires a visible Quick Action result.
- Added bounded, process-isolated mutation execution and complete behavioral qualification for every actionable mutant.

## [3.0.0] - 2026-09-13

### Security

- **Breaking:** Replaced the v2 text-only projection with source-bound MIME pruning. v3 preserves retained `text/plain` and `text/html` payloads and their source order; it removes only exact MIME attachments and non-root `multipart/related` components, rejecting ambiguous roles instead of guessing.
- Added raw-span candidate construction, source/candidate payload fingerprints, independent reparse verification, schema-3 batch ledger reports, and atomic no-replace output publication.
- Removed HTML/CSS parsing, semantic-equivalence projection, `--force`, `--skip-existing`, newline `paths`, the `.text-only.eml` suffix, and all v2 report compatibility behavior. Regenerate derived copies from unchanged originals.
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
