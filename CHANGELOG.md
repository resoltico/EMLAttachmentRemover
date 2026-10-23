# Changelog

Notable changes to this project are documented in this file. The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.1.0/), and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [Unreleased]

### Changed

- **Breaking copy naming/layout/order:** output filenames now include a 26-character token of the complete logical destination route, and generated mirrored folders end in `.eml-dir`. There is no old-name lookup, renaming or migration. Overlapping source entries prefer explicit files, then the deepest selected root; complete planning assigns deterministic order and contiguous result indices. Known destination conflicts refuse every contender before writes. Integrators should use reported native destinations and indices; see [filesystem behavior](docs/filesystem.md).
- **Breaking MIME behavior:** valid non-inline extension dispositions on ordinary parts now remove the whole attachment. Retained-body `multipart/report` refuses conversion with `UNSUPPORTED_MIME` rather than dropping its required machine-readable part. Attached and nonselected related reports remain opaque removable entities; originals remain unchanged. See [MIME behavior](docs/mime.md).
- Matching existing copies acquire only their actual synchronization capabilities. Creation policy applies to each immediate mutation parent, preserving real synchronization/access failures and no-replace publication. Unsupported new-name profiles can refuse read-only planning with `DESTINATION_UNSUPPORTED` at admission.
- The macOS app offers **Check only / Create copies**, defaulting to Create. A Create from a completed Check receives fresh identities and repeats every authoritative check. CLI and GUI share core outcome/removal meanings, including simultaneous publication, address, durability and cleanup consequences; public protocol 1 retains its field shape.
- Report export runs in an owned child lane alongside processing. A folder picker plus filename form replaces the occupancy-checking Save panel. Existing reports are never replaced. Cancellation, Force Stop and Quit retain actual settlement and possible-publication uncertainty; two full-report reservations still bound processing capacity.
- **Breaking private transport/report format:** bundled desktop operations use strict protocol 2; old private worker modes are removed. Metadata exports use app-owned `eml-technical-report` schema 1 with required explicit nullable lifecycle observations and derived terminal kind. Historical unversioned exports are not imported.
- macOS lifecycle and open-URL delivery use source-defined callbacks in the retained Winit fork. Standard native About replaces the custom panel; Help exposes the actual AboutSlint widget and complete licenses, plus Project Website. Finder navigation is advisory without a parent existence probe.
- **Native qualification prerequisites:** finite owned-volume naming and existing-copy fixtures now require Python 3.9+, native filesystem utilities and disposable storage privileges; Windows additionally uses PowerShell 7.4 or newer for native byte-preserving command capture. Qualification records actual case/ownership/8.3 behavior and cleanup, and refuses missing profile coverage.
- Contributor/release tools use one typed distribution plan and verified current-run inputs. `verify-release-inputs` precedes `stage-release-assets --inputs`; exact product/checksum sets and same-byte consumers govern admission. Healthy picker qualification is independent of incident policy; only the exact recognized failure may admit the expiring reviewed intervention. Final modified-host admission rechecks original policy identity/expiry after server readback.

### Internal

- Contributor checks use `cargo xtask check-config` for Cargo ownership and lint inheritance. The source-coordinate exception registry, custom Rust/Swift scanners and structural metric gates are removed; compiler/lint/formatter/native checks remain, with local exception rationales and Clippy unsafe-block documentation checks.
- MIME preparation uses a forward physical parser with active ancestor-boundary matching and an independent retained-span proof. Candidate and fixed-point verification no longer reconstruct payloads at each container ancestor; private deterministic tests cover work bounds and cancellation across preparation phases.

## [6.0.0] - 2026-10-21

### Changed

- **Breaking for Python users and integrators:** reusable Rust libraries and a native CLI replace the Python processor, and a Slint macOS app with a private core-backed worker replaces the Swift/Python frontend. The public Python API, including `eml_attachment_remover.process_file`, Python packages, wheels, zipapps, Python source distributions, bundled/external interpreter editions and managed app installer are removed. Use the native CLI or macOS app; library integrations must use the Rust contracts. GUI and CLI remain separate products sharing the same business libraries. Corresponding MPL-covered source is available from the matching repository revision; redistributors must preserve notices and source availability. See [setup and redistribution](README.md#develop-and-redistribute).
- **Breaking for CLI/report consumers:** use `--output-dir`, `--dry-run`, `--format human|json` and `--request FILE|-`, plus help/version. `--version` and human run headers include the shared product build number; JSON version fields remain release versions. `-o`/`--output`, `--existing`, `--fail-fast`, `--output-format`, `paths0`, `--request-stdin` and `--progress-fd` are removed. Protocol 1 replaces the schema-3 document and advisory progress channel with LF-delimited start/item/summary events, lossless native paths and explicit publication, address, synchronization and cleanup observations. Requests are bounded EOF-terminated JSON rather than the former framed, keep-open pipe contract. Replace the numeric error taxonomy (codes 3–11 and internal error 70): v6 uses 1 for failure, 2 for pre-stream usage/admission failure and 130 for interruption. A missing summary or truncated stream is incomplete; a failure does not establish that no copy exists. Update integrations to the [protocol contract](docs/protocol.md).
- **Breaking MIME policy:** copies retain supported plain-text and HTML body representations rather than only pruning explicitly marked attachments and non-root related resources. Other non-body parts, including inline images outside related containers, calendar payloads and opaque encoded forwarded messages, are removed without requiring attachment disposition. Empty containers collapse, and a message with no supported retained body is refused. Protected content reached in body context remains refused. See the [MIME policy](docs/mime.md); retained HTML is not sanitized or certified safe to open.
- **Breaking copy naming and layout:** `.no-attachments.eml` replaces `.mime-pruned.eml`. Explicitly selected files go directly into the existing output folder; selected directory trees mirror beneath root labels instead of using flat source-address-digest names. Long stems are shortened to native component limits without a digest or numeric collision search. Conflicting reservations, differing existing files, source aliases and overlapping output trees are refused. Use reported destinations rather than deriving names, and choose distinct destinations for collisions.
- Identical existing copies are automatically verified and resynchronized as up to date instead of requiring v5's `--existing=verify`; different existing bytes remain untouched. This applies to the current derived destination: old `.mime-pruned.eml` copies are not renamed or reused as `.no-attachments.eml` outputs and can be selected again during directory walks. Move old copies out of selected folders if they should not be processed.
- **Breaking for CLI cancellation:** the first interrupt requests cooperative stop without v5's automatic 10-second processing grace escalation; a second forces exit 130. Completed copies remain, and forced termination can leave staging residue or incomplete output. Native OS/storage calls can outlast cooperative cancellation; no universal stop deadline is promised. Inspect destinations after all processing has settled before retrying or removing recognized residue.
- **Breaking for macOS app users:** one main window retains serial queued runs instead of independent report windows. Each selection captures its destination; up to eight requests may wait, and two retained reports pause further processing until a completed report is dismissed. The app adds confirmed Force Stop and Quit actions plus explicit technical-report export; Quit waits for owned worker and export settlement. The interface uses the discovered macOS system UI font family and AppKit base text size with optical sizing and display-density-independent tracking, and a single About window is accessible from the macOS application menu. Saved destinations are not migrated; choose the destination again. CLI and GUI display the shared release version and distinct product build number; the app bundle retains a monotonically increasing build number. Worker protocol identity uses the workspace release version. The macOS 15.0 minimum and Apple ad hoc signing remain; there is no Developer ID or notarization. See the [app guide](docs/macos-app.md).
- The raw-source limit increases from 128 MiB to 256 MiB; this is an input limit, not a peak-memory guarantee. Collection admits at most 4,096 file/failure work items, 100,000 visited entries, 64 directory levels within a selected root and 8 MiB of charged reported source/destination paths. **Directory selections exceeding these bounds are refused;** split the selection or shorten paths. Raw requests retain separate 4,096-path/4 MiB native-path limits. See the [collection contract](docs/filesystem.md#selection-spelling-links-and-overlap).
- **Contributor build requirements:** Rust and its components are selected by `rust-toolchain.toml`; macOS app qualification requires Xcode 26.3 build 17C529 from `xtask/tools.toml`. Unix evidence-admission checks also require jq on PATH, and repository checks require the pinned tools in that file. Prebuilt-product users need neither Rust nor Xcode. Pinned-Xcode release checks run on hosted native Macs; an unavailable local Xcode leaves those local checks unverified rather than blocking hosted qualification. See [contributor setup](CONTRIBUTING.md).

### Added

- Separate native CLI archives for macOS Intel/Apple Silicon, Linux x64/ARM64 and Windows x64/ARM64, plus two same-CPU macOS app archives and `SHA256SUMS`. Download names use `remove-eml-attachments-VERSION-PLATFORM` for the CLI and `EML-Attachment-Remover-VERSION-macos-CPU.zip` for the app; Apple Silicon is labeled `aarch64` instead of `arm64`. There are no universal binaries or Windows/Linux GUIs. Linux targets Ubuntu 22.04+ GNU/glibc with required GLIBC symbols capped at 2.35; Windows uses system UCRT. Target declarations do not qualify every OS, filesystem or client patch. See the [platform policy](README.md#native-platform-policy) and [release procedure](docs/release.md).

### Security

- **Breaking for POSIX destinations:** staging and mirrored-directory creation now require a parent namespace protected against other-user entry substitution, with ownership, mode/sticky, macOS ACL and supported filesystem-model checks before creation and identity-dependent cleanup. Linux namespace creation refuses unreviewed models, including OverlayFS, FUSE, NFS, CIFS and FAT/exFAT; macOS refuses volumes that ignore ownership. Use a compatible private destination owned by the current user or trusted root; existing folders are not chmodded or assigned replacement ACLs. Source reads and identical-existing-copy comparisons have separate admission. Same-authority/administrator interference and later changes to completed copies remain outside this protection. See the [filesystem boundary](docs/filesystem.md#posix-namespace-admission-before-creating-an-entry).

### Internal

- Rust verification combines synthetic MIME/protocol contracts, native test discovery/execution accounting, real filesystem/process controls and extracted-archive behavior with counterfeit rejection. Preparation fuzzing and mutation campaigns are bounded; mutation selection covers two authorization/edit-range guards rather than the whole workspace. Authored-source compiler and linter checks reject warnings, with necessary exceptions centrally registered by exact file and named scope. Structural limits cover application code, tests and checking tools, with rejection controls; these checks support design review rather than proving architectural quality. Builds generate target-specific crate, standard-library and native-runtime notices with accompanying license files from reviewed source inputs. CI installs checksum-verified prebuilt tools with source-build fallback disabled; see [verification](docs/verification.md).
- Owner-dispatched publication qualifies the exact source across all six native CLI targets, both app candidates, both complete macOS 15 desktop consumers and Intel macOS 26 picker acceptance before assembling the eight archives and `SHA256SUMS`. Intel picker acceptance requires guarded preparation on the configured hosted image with recognized current-boot IconServices crash evidence, then tests the unchanged app and verifies host/service state afterward. These records establish modified-host coverage, not general unmodified-host compatibility or causal proof of recovery; earlier failed runs remain failed. Admission requires push-capable credentials to recognize unpublished drafts and source workflow configuration matching current main. Publication uses source/qualification attestations and verifies draft assets before publishing; failed runs retain server observations for recovery. Qualification and publication records identify the exact admitted source and delivered bytes; see the [release procedure](docs/release.md).

## [5.0.0] - 2026-10-15

### Changed

- **Breaking for native app users on macOS 14:** Apple Silicon and Intel app builds now require macOS 15.0 or later. Upgrade macOS before installing these builds; published v4.0.0 apps retain their macOS 14 requirement and remain unchanged. Pull-request and tagged-release compatibility checks cover macOS 15 Apple Silicon, macOS 15 Intel and Xcode 27, with additional Intel checks through Rosetta on Apple Silicon. See the [native release requirements](https://github.com/resoltico/EMLAttachmentRemover/blob/f7adf489734beff3ca94f829f8db1c110be316f4/integrations/macos-ui/RELEASE.md#minimum-native-macos-release-150-sequoia) and [Issue #29](https://github.com/resoltico/EMLAttachmentRemover/issues/29).
- **Native fuzz runner callers:** `-runs` must be between 1 and the configured 100,000-execution ceiling; unlimited `-runs=-1` and larger requests are refused. For extended exploration, omit `-runs` and use `-max_total_time=1800`; the execution ceiling still applies.

### Fixed

- The native app displays advisory processing progress as messages arrive, including small batches, instead of buffering updates until the processing pipe closes. Final reports remain authoritative.
- The native app rejects malformed JSON receipts and ignores malformed JSON progress updates before passing them to Foundation's JSON decoder, avoiding decoder traps on malformed input.
- Bundled-runtime builds and archive verification compile Python bytecode with an interpreter from the SHA-256-pinned upstream runtime archives instead of the invoking Python. Cross-CPU builds authenticate a separate host-native compiler; bytecode mismatch diagnostics identify the differing header or payload and both digests.

### Security

- POSIX publication uses an owner-private, descriptor-held staging directory to prevent replacement of the outer staging name from redirecting the candidate used by the atomic hard-link or exclusive-rename operation. On macOS, inherited extended ACL entries are cleared before retained message bytes are written; unexpected ACL failures refuse staging. A forced stop or cleanup failure can leave a directory containing `candidate`, rather than the previous staging file. After all processing using the destination has stopped, inspect and manually remove only recognized leftovers; later runs do not remove old stages. Actors allowed to modify the destination directory can still alter final entries after publication. See the [staging cleanup guidance](https://github.com/resoltico/EMLAttachmentRemover/blob/f7adf489734beff3ca94f829f8db1c110be316f4/README.md#input-and-filesystem-limits).
- MIME verification derives affected ancestor headers independently of candidate construction, so a shared traversal defect cannot silently retain stale headers.

### Internal

- Release and compatibility consumers select the exact producer's artifact ID, including partial workflow reruns, instead of selecting delivery artifacts by name.
- Corrected source-bound mutation-equivalence evidence and added pull-request validation of its full source digest. Relevant main-branch changes trigger mutation campaigns; bounded diagnostics prioritize unreviewed survivors. Expanded failure controls cover runtime authentication and publication, POSIX staging and ACLs, generated-root provenance, CLI reporting and mutation diagnostics.
- Superseded push and pull-request assurance campaigns cancel while manual and scheduled runs retain separate concurrency groups. All workflow evidence and delivery-transport artifacts explicitly expire after 30 days; public release assets remain available separately.
- Hosted UV commands replace their shell wrapper so cancellation reaches the task; POSIX task termination sweeps its owned command group before exit. Mutation workers keep standard temporary files inside campaign-owned cleanup storage. Native fuzzing removes compiler scratch, disables core dumps, caps each stage's diagnostics at 8 MiB and avoids redundant corpus reloads. Hypothesis observation sanitization streams records and uses a same-filesystem move to avoid a second sanitized-tree copy; cross-filesystem publication can still copy. Failed sanitization closes input and output streams before cleanup so open handles do not strand temporary files on Windows. These controls do not impose an aggregate filesystem quota; see the [resource limits](https://github.com/resoltico/EMLAttachmentRemover/blob/f7adf489734beff3ca94f829f8db1c110be316f4/QA.md#native-receipt-fuzzing).
- Native tool downloads enforce pinned archive sizes while verifying SHA-256, and swift-format fetches only its pinned source revision with disposable build caches. CI reuses the small installed quality-tool binaries through exact platform/source cache keys and still checks their versions; the compiler installation is excluded. The local Linux mutation cache prunes dangling UV entries while retaining reusable wheels.

## [4.0.0] - 2026-10-06

### Added

- A native macOS app creates EML copies from files or folders selected through its picker, drop zone or Finder Services. Choose a destination folder or save beside each original, follow progress, inspect per-file outcomes and diagnostics, and reveal completed copies in Finder. `Stop processing` preserves completed copies; `Done` closes a finished report. Multiple batches have independent report windows, and File → Close Completed Reports leaves active runs open. The interface follows light/dark appearance and Increase Contrast, includes original app and status artwork, and provides About and license information. See the [app guide](https://github.com/resoltico/EMLAttachmentRemover/blob/f7adf489734beff3ca94f829f8db1c110be316f4/integrations/macos-ui/README.md).
- Separate Apple Silicon and Intel app ZIPs require macOS 14 or later. Primary downloads include a private CPython runtime; smaller `-external-python.zip` editions require separately managed CPython 3.14. Choose the CPU matching your Mac. Apps are ad-hoc signed without Developer ID or notarization; first launch may require [app-specific Gatekeeper approval](https://github.com/resoltico/EMLAttachmentRemover/blob/f7adf489734beff3ca94f829f8db1c110be316f4/integrations/macos-ui/README.md#first-launch-and-macos-approval).
- The optional managed installer refuses CPU mismatches and updates while the app is running, retains previous bundles in private recovery backups, and preserves maintained interpreter symlinks for external-Python apps. Backups accumulate until you remove them; the installation guide explains cleanup. Bundled apps can also be copied into Applications and do not change external interpreter configuration.
- Frontends can use `--request-stdin` for bounded, framed native-path selections with cancellation tied to the stdin pipe's lifetime, and `--progress-fd=N` for advisory schema-1 progress on a separate pipe. Final schema-3 reports remain authoritative; progress counts do not prove successful copies. The native app uses these interfaces and admits up to 4,096 inputs; Finder launch commands can still encounter OS argument limits. See the [input](https://github.com/resoltico/EMLAttachmentRemover/blob/f7adf489734beff3ca94f829f8db1c110be316f4/QA.md#framed-frontend-input) and [progress](https://github.com/resoltico/EMLAttachmentRemover/blob/f7adf489734beff3ca94f829f8db1c110be316f4/QA.md#advisory-processing-progress) contracts.

### Changed

- **Licensing change for redistributors:** project software and documentation use MPL 2.0; original artwork has separate proprietary terms permitting unchanged redistribution with this application and excluding independent reuse or modification. Distributing MPL-covered binaries requires corresponding source availability and a notice telling recipients how to obtain it. The release source archive includes the native app and editable artwork. Previously distributed MIT versions retain their grants, and bundled third-party components retain their own licenses. See [LICENSE](https://github.com/resoltico/EMLAttachmentRemover/blob/f7adf489734beff3ca94f829f8db1c110be316f4/LICENSE).
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
- Native source builds require full Xcode 26 or later for the layered icon and the pinned official Swift.org compiler. Prebuilt app users need neither. See the [build instructions](https://github.com/resoltico/EMLAttachmentRemover/blob/f7adf489734beff3ca94f829f8db1c110be316f4/integrations/macos-ui/README.md#build-from-source).

### Removed

- **Breaking for Python callers inspecting source snapshots:** removed `SourceSnapshot.mode`. POSIX source mode remains represented in `identity.file_type`; Windows records file kind there. New output permissions are not copied from the source.
- **Breaking for existing Quick Actions:** removed the shell-command text-report integration, its presentation switches and dedicated installer/uninstaller. Configure the native app launch action described in the [Finder guide](https://github.com/resoltico/EMLAttachmentRemover/blob/f7adf489734beff3ca94f829f8db1c110be316f4/integrations/macos-ui/README.md#finder-quick-action), or use the CLI/zipapp for synchronous automation. The app installer does not install the CLI.

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
- Cooperative interruption preserves completed publication receipts and records invocation interruption separately. Human and `paths0` reports explain failures on stderr without changing accepted stdout paths or duplicating notices. Interrupted delivery allows 10 seconds without accepted-output progress; a second signal ends that grace. Signals after a complete report preserve its bytes and select status 130 with a bounded notice. Forced termination can leave stages, copies or partial output without a complete report; CPython stream-finalization failure can change process status to 120. See the [cancellation contract](https://github.com/resoltico/EMLAttachmentRemover/blob/f7adf489734beff3ca94f829f8db1c110be316f4/README.md#destination-and-existing-policy).
- `process_file()` closes internal report storage before returning a detached single-item ledger with bounded processing and publication evidence. Cooperative shutdown interruption retains that ledger; controller setup and shutdown release acquired state independently, preserve primary errors and restore the caller's signal policy.
- Packaged installations and zipapps permit a temporary directory beside the installation; source checkouts continue to exclude their own tree from report temporary storage.

### Security

- Private report receipts use anonymous or unlinked-open storage on POSIX and delete-on-close storage on Windows. They are not reopened by name and do not depend on Python cleanup after forced termination. Publication stages have a different lifetime and can remain after a hard stop; see the [staging-file guidance](https://github.com/resoltico/EMLAttachmentRemover/blob/f7adf489734beff3ca94f829f8db1c110be316f4/README.md#input-and-filesystem-limits).

### Internal

- Native receipt, path and progress handling share coverage-guided libFuzzer qualification with AddressSanitizer, raw/structured inputs and deliberate failure controls. Python MIME qualification includes bounded byte mutations and seed splices alongside structured properties; model checks also exercise production optimization. Mutation qualification isolates test working directories and rejects campaigns with changed source inputs or unexpected workspace artifacts.
- Python and Swift formatting, lint, size and complexity checks use centralized policies and scoped exception approvals. Ambiguous Python directives bind to their owning statement or declaration, and inline file-wide Ruff overrides are rejected.
- `uv run python tools/tasks.py ci` runs host-applicable quality, release-build, mutation and property checks with standard and free-threaded CPython; macOS mutation campaigns require Docker. Release qualification also checks Windows and separate minimum-OS/Intel/macOS consumers in GitHub CI. Publication requires main-branch ancestry, the complete checksummed asset set, independent build comparisons, archive verification and provenance attestations. Pinned runtime archives are cached and reauthenticated; build products remain fresh. See [QA](https://github.com/resoltico/EMLAttachmentRemover/blob/f7adf489734beff3ca94f829f8db1c110be316f4/QA.md) and the [release contract](https://github.com/resoltico/EMLAttachmentRemover/blob/f7adf489734beff3ca94f829f8db1c110be316f4/integrations/macos-ui/RELEASE.md).

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

[Unreleased]: https://github.com/resoltico/EMLAttachmentRemover/compare/v6.0.0...HEAD
[6.0.0]: https://github.com/resoltico/EMLAttachmentRemover/compare/v5.0.0...v6.0.0
[5.0.0]: https://github.com/resoltico/EMLAttachmentRemover/compare/v4.0.0...v5.0.0
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
