# Changelog

Notable changes to this project are documented in this file. The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.1.0/), and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [Unreleased]

## [4.0.0] - 2026-10-06

### Added

- A native macOS app creates EML copies from files or folders selected through its picker, drop zone or Finder Services. Choose a destination folder or save beside each original, follow progress, inspect per-file outcomes and diagnostics, and reveal completed copies in Finder. `Stop processing` preserves completed copies; `Done` closes a finished report. Multiple batches have independent report windows, and File → Close Completed Reports leaves active runs open. The interface follows light/dark appearance and Increase Contrast, includes original app and status artwork, and provides About and license information. See the [app guide](integrations/macos-ui/README.md).
- Separate Apple Silicon and Intel app ZIPs require macOS 14 or later. Primary downloads include a private CPython runtime; smaller `-external-python.zip` editions require separately managed CPython 3.14. Choose the CPU matching your Mac. Apps are ad-hoc signed without Developer ID or notarization; first launch may require [app-specific Gatekeeper approval](integrations/macos-ui/README.md#first-launch-and-macos-approval).
- The optional managed installer refuses CPU mismatches and updates while the app is running, retains previous bundles in private recovery backups, and preserves maintained interpreter symlinks for external-Python apps. Backups accumulate until you remove them; the installation guide explains cleanup. Bundled apps can also be copied into Applications and do not change external interpreter configuration.
- Frontends can use `--request-stdin` for bounded, framed native-path selections with cancellation tied to the stdin pipe's lifetime, and `--progress-fd=N` for advisory schema-1 progress on a separate pipe. Final schema-3 reports remain authoritative; progress counts do not prove successful copies. The native app uses these interfaces and admits up to 4,096 inputs; Finder launch commands can still encounter OS argument limits. See the [input](QA.md#framed-frontend-input) and [progress](QA.md#advisory-processing-progress) contracts.

### Changed

- **Licensing change for redistributors:** project software and documentation use MPL 2.0; original artwork has separate proprietary terms permitting unchanged redistribution with this application and excluding independent reuse or modification. Distributing MPL-covered binaries requires corresponding source availability and a notice telling recipients how to obtain it. The release source archive includes the native app and editable artwork. Previously distributed MIT versions retain their grants, and bundled third-party components retain their own licenses. See [LICENSE](LICENSE).
- **Breaking for Finder automation:** Quick Actions launch the native app and finish at launch, rather than waiting for processing. Leaving a finished report open does not hold the shortcut open. Integrations requiring completion or exit status must use the wheel-installed CLI or standalone zipapp.
- **Breaking for `--output-dir` callers:** automatic copy names include a digest of the kernel-resolved source address, distinguishing ordinary same-basename inputs from different folders and keeping subset reruns stable. Different spellings resolving to the same address reuse the same name; hard links at different addresses and source renames can still produce different names. Use the reported destination. Explicit `--output` names and copies beside originals retain their naming behavior; aliases and residual collisions remain refused.
- Automatic names exceeding the destination filesystem's filename limit use a readable source-name prefix, a stable 16-hex digest and `.mime-pruned.eml`. Explicit `--output` names are never changed. Use the reported destination to locate shortened copies.
- **Breaking for callers supplying home-directory shorthand:** filename arguments are literal; the processor no longer expands `~`. Let the shell expand it before invocation or supply an absolute path. Quoted paths such as `'~/message.eml'` now address an actual `~` directory.
- **Breaking for human-output parsers:** success and dry-run lines include the final or planned destination, for example `created: source.eml -> /path/to/source.mime-pruned.eml`. Use schema-3 JSON or NUL-delimited `paths0` instead of parsing the former source-only lines.
- **Breaking for status consumers:** explicitly unsupported atomic publication on POSIX (`ENOTSUP`/`EOPNOTSUPP`) or Windows (`STATUS_NOT_SUPPORTED`) reports `ATOMIC_PUBLICATION_UNSUPPORTED` and single-input process status 11 instead of `WRITE_ERROR`/7. Mixed batches retain status 9. Use a compatible destination volume; ambiguous failures such as Linux `EPERM` remain generic write errors. Publication remains atomic and never replaces existing files.
- **Breaking for previously accepted longer paths:** publication is refused when the resolved final address cannot be obtained or exceeds 4,096 native units: POSIX bytes or Windows UTF-16 code units. Use a shorter resolved destination path when the length limit is reached.
- Unexpected ordinary exceptions confined to MIME preparation after source binding produce a per-file `INTERNAL_ERROR` while later inputs continue. The invocation remains unsuccessful. Invariant failures, memory exhaustion and failures in shared backend, report-admission or publication state still stop the batch.
- Structured MIME headers accept one trailing parameter separator while rejecting empty internal or repeated trailing segments. Diagnostics identify the affected content field and MIME location. The 128 MiB raw-source limit remains, while the separate 96 MiB decoded-retention cap is removed; neither bounds peak process memory.
- Error messages are capped at 2,048 characters and 12,288 canonical ASCII JSON content bytes, including the truncation marker. Consumers must allow truncation and interpret outcomes using error codes and publication receipts.
- On macOS, file and directory synchronization additionally requests a hardware-cache flush when supported, with portable fsync fallback for an explicitly unsupported stronger operation. Receipts describe operating-system call completion, not survival of power loss or failing hardware.
- Archive files and directories carry the build invocation datetime instead of fixed historical timestamps. Independent build comparisons share that datetime; separate invocations can produce different checksums.
- Large MIME payloads and folded headers require less scanning and copying while retaining the same delimiter and display-escaping rules.
- Native source builds require full Xcode 26 or later for the layered icon and the pinned official Swift.org compiler. Prebuilt app users need neither. See the [build instructions](integrations/macos-ui/README.md#build-from-source).

### Removed

- **Breaking for Python callers inspecting source snapshots:** removed `SourceSnapshot.mode`. POSIX source mode remains represented in `identity.file_type`; Windows records file kind there. New output permissions are not copied from the source.
- **Breaking for existing Quick Actions:** removed the shell-command text-report integration, its presentation switches and dedicated installer/uninstaller. Configure the native app launch action described in the [Finder guide](integrations/macos-ui/README.md#finder-quick-action), or use the CLI/zipapp for synchronous automation. The app installer does not install the CLI.

### Fixed

- Removing a final stale MIME header includes its terminating newline, preserving the blank-line separator instead of inserting another blank line. Header-byte budgets count the complete field ending. Source-open errors identify the requested path without Python byte-string formatting.
- Unterminated MIME comments ending in a complete escape pair are refused as parse errors rather than crashing.
- MIME parsing accepts RFC 5322 punctuation in optional header names, complete root headers with an empty body, and RFC 2231 structural parameters such as `boundary*=` and `boundary*0*=`. Extended filename parameters may have empty charsets; structural values must decode to printable ASCII. Malformed or ambiguous forms remain refused.
- Parts with empty header blocks exclude the separator line from retained payloads and hashes; only body octets are retained and fingerprinted.
- Windows source directories no longer require write access; destination handles retain the access required for publication and durability. Directory-sync errors use platform errno names rather than values from another operating system.
- **Breaking for oversized evidence:** inputs exceeding the existing 1 MiB per-record evidence limit or available 64 MiB report capacity now fail with `PARSE_ERROR` before any copy is published. Batches reserve space for later terminal records; reduce batch size or message complexity when these limits are reached.
- Usage errors, report-storage startup failures, processing failures and interruption reports preserve the selected output format and dry-run mode. Repeated format options use the final effective known choice; arguments after `--` remain filenames.
- Report-storage failures preserve known publication outcomes through bounded memory recovery without requiring spool reads or writable storage. Recovery begins only before stdout accepts any report bytes; partial documents are never replaced. Detailed MIME evidence and warnings can be omitted, and an unavailable output endpoint can still prevent delivery.
- JSON and recovery JSON remain ASCII on non-UTF-8 stdout, `paths0` preserves exact native POSIX bytes, and human output escapes unsafe or unrepresentable text. Short writes and nonblocking stalls retain unwritten output. Closed Windows report pipes select status 1 even when the runtime reports an invalid-argument error.
- Cooperative interruption preserves completed publication receipts and records invocation interruption separately. Human and `paths0` reports explain failures on stderr without changing accepted stdout paths or duplicating notices. Interrupted delivery allows 10 seconds without accepted-output progress; a second signal ends that grace. Signals after a complete report preserve its bytes and select status 130 with a bounded notice. Forced termination can leave stages, copies or partial output without a complete report; CPython stream-finalization failure can change process status to 120. See the [cancellation contract](README.md#destination-and-existing-policy).
- `process_file()` closes internal report storage before returning a detached single-item ledger with bounded processing and publication evidence. Cooperative shutdown interruption retains that ledger; controller setup and shutdown release acquired state independently, preserve primary errors and restore the caller's signal policy.
- Packaged installations and zipapps permit a temporary directory beside the installation; source checkouts continue to exclude their own tree from report temporary storage.

### Security

- Private report receipts use anonymous or unlinked-open storage on POSIX and delete-on-close storage on Windows. They are not reopened by name and do not depend on Python cleanup after forced termination. Publication stages have a different lifetime and can remain after a hard stop; see the [staging-file guidance](README.md#input-and-filesystem-limits).

### Internal

- Native receipt, path and progress handling share coverage-guided libFuzzer qualification with AddressSanitizer, raw/structured inputs and deliberate failure controls. Python MIME qualification includes bounded byte mutations and seed splices alongside structured properties; model checks also exercise production optimization. Mutation qualification isolates test working directories and rejects campaigns with changed source inputs or unexpected workspace artifacts.
- Python and Swift formatting, lint, size and complexity checks use centralized policies and scoped exception approvals. Ambiguous Python directives bind to their owning statement or declaration, and inline file-wide Ruff overrides are rejected.
- `uv run python tools/tasks.py ci` runs host-applicable quality, release-build, mutation and property checks with standard and free-threaded CPython; macOS mutation campaigns require Docker. Release qualification also checks Windows and separate minimum-OS/Intel/macOS consumers in GitHub CI. Publication requires main-branch ancestry, the complete checksummed asset set, independent build comparisons, archive verification and provenance attestations. Pinned runtime archives are cached and reauthenticated; build products remain fresh. See [QA](QA.md) and the [release contract](integrations/macos-ui/RELEASE.md).

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
