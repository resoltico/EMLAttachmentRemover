# v6 implementation evidence

Implementation remains in progress. No complete v6 product/archive qualification
or release acceptance is claimed. Version6 stays under Unreleased.

## Scope, source and prerequisites

CLI and GUI are separate products and thin adapters over the same Rust business
libraries. The macOS frontend uses Slint with a private core-backed worker.
Windows/Linux GUI implementations are outside scope. Required outputs remain
six native CLI archives, two native macOS app archives and SHA256SUMS. There are
no universal binaries, compatibility decoders, migrations or Python tooling.
Signing is Apple ad hoc only, without Developer ID or notarization.

Published baseline and last observed main are
`f7adf489734beff3ca94f829f8db1c110be316f4` (v5.0.0); no later main commits were
present at the last fetch. Work is isolated from the owner's checkout. AGENTS.md
continues to govern. No tag, publication, merge, approval, auto-merge, repository
settings change or pushed-history rewrite is authorized.

The supplied design archive's sidecar verified SHA-256
`dfa9a31a98be458e35c8219586fd1096086270b73f33af4ab26e1a1e129e7569`.
Member inspection found34 regular files under one relative prefix, without links,
absolute/parent paths or devices. Extraction used a new empty directory outside
the repository. PROMPT.md and every README-ordered design/contract/evidence file
were read. The owner's subsequent frontend and product-boundary decisions govern
current work; [frontend design](desktop-frontend-design.md) records the result.

Rust1.99.0 is pinned by rust-toolchain.toml, commit
`b940084d7eb6a299eb4bfeb8e34901bc051e7ac4`, edition2024. Target and native runner
policy comes from xtask/distribution.toml. macOS minimum is15.0; required build
Xcode is26.3/17C529. The local ARM host has macOS27.0/26A428 and Xcode27/SDK27.
Local observations do not qualify the pinned native macOS matrix.

## Business, transport and storage

eml-core owns collection, destination naming/reservations, immutable MIME edits,
independent edit authorization, cancellation, existing-copy comparison,
publication decisions and outcomes. eml-fs supplies held native capabilities.
eml-protocol shares the current wire contract. CLI arguments/human presentation
remain private to the CLI. The desktop worker calls eml-core directly.

Synthetic MIME coverage includes12 supplied inputs,10 independent output goldens,
protected-message refusal, differential/model/limit checks, physical ftext and
root-From behavior, nested boundary-prefix/reuse controls and contextual duplicate
CTE checks. Input bytes are never regenerated as a source of expected output.
Independent escaped-byte review covers all four retained LF-bearing goldens at
both CRLF and CR-only review views; the latter are derived review views rather
than additional executions. Original messages remain read-only.

Native storage checks exercise stable bounded reads, held full identities,
private staging, no-replace publication, requested address, synchronization,
checked close/cleanup and truthful post-edge failures. Unix namespace controls
used synthetic owned resources and actual different UIDs. Windows uses full
128-bit IDs and generated SDK layouts; its final confinement/lifecycle gates
remain open. No metadata-immutability, rollback-after-publication or power-loss
survival guarantee is inferred from these calls.

Actual process checks cover CLI create/rerun/request/EOF, partial output and
signals, closed stdout and independent counterfeits. Cooperative and forced
interruption preserve observed outcomes and uncertainty. An exit code alone is
not archive, file-byte, stream or GUI evidence.

## Desktop model and native owner

The Rust desktop model retains immutable destination snapshots, eight waiting
requests, one active run and two report reservations. Full report space pauses
admission without evicting results. Independent tests cover native non-Unicode
transport, required nulls, malformed/map/duplicate/closed-value boundaries,
strict LF/run limits and irreversible failures.

The owned decoder is checked against all22 hand-authored protocol scenarios and
recursive deletion of each required field, including present nullable fields.
The stateful Rust reducer runs all22 cases at every byte boundary with explicit
item/exit oracles. Additional controls reject changed publication receipts,
counts, identity kind, index order, partial tails and actual-exit disagreement,
while preserving earlier accepted outcomes and structured technical exports.
These are library checks, separate from actual app/GUI qualification.

The production supervisor now owns private identity and processing launches,
concurrent bounded stdout/stderr/control drains, request feeding, checked pipe
release, and actual reap observations on independent threads. It retains child
and endpoint ownership through parser/backpressure failures and panics; unresolved
actors close admission and stay visibly pending. Control uses launch-bound
positive-PID authority, private readiness/first-interrupt observations, immutable
run identities and fixed confirmed-Quit escalation. Stop intent never rewrites
accepted Core outcomes or establishes actual signal delivery by itself.

The local integrated workspace command passed417 tests in39 positive plus eight
expected empty result summaries,
with zero failures, ignores or filters, using Rust1.99.0, deployment15.0 and the
local SDK27. The command enabled the native Mac child/worker and GUI fixture
features. Separate strict Clippy checks include the actual GUI profile and Slint
compiler warnings as errors. Logs remain under the owned worktree's target
folder; these are local source-checkpoint observations, not six-platform or
archive qualification. Later source changes require their applicable checks again.

The repository entry-point check passed every section except macos-desktop,
which explicitly failed because required Xcode26.3 is absent locally. Its separate
default-feature test discovery/execution agreed on395 passing cases,35 positive
plus nine empty summaries. Compiler/format/check/Clippy/docs, all selected
workspace/fuzz/tool dependency audits, actionlint1.7.12, shellcheck0.11.0 and
all-six-target notice freshness passed. Shellcheck found zero tracked shell
scripts; that is a scoped empty inventory, not script behavior evidence.
The enabled desktop/native-fixture profile separately passed full workspace
Clippy with Rust and Slint warnings treated as errors. CLI notice regeneration
left its tracked legal materials unchanged; enabled GUI/worker legal review
still remains separate and incomplete.

Seven actual native descriptor/owner cases cover spawn/stdin/stdout/stderr,
closed standard descriptors, signal-mask reset, terminal signal revocation,
full nonblocking control pipes and broken-pipe behavior without SIGPIPE exit.
An executed negative control removing only the control pipe's Darwin
F_SETNOSIGPIPE protection caused actual SIGPIPE failure; restored code passed.
Real private-worker checks cover Core/golden execution, readiness, held-input
Stop, first-interrupt observation, the same handler's second interrupt and
preserved earlier outcomes. A separate unexpected-pre-readiness-stdout negative
control demonstrated rejection. Accelerated policy-clock Quit tests establish
state transitions, not elapsed ten/twelve-second physical deadlines.

Explicit report export retains the exact run's accepted facts and terminal
worker exit/signal, first lifecycle failure, diagnostics and uncertainty. It uses
held eml-fs staging/no-replace/sync/cleanup primitives, keeps exact final-file
identity alive across candidate cleanup, and checks final bytes/address. Actual
worker-backed tests cover successful export, original/report collision refusal,
pre-staging cancellation and failed-worker lifecycle export. A busy report lock
fails promptly rather than blocking export cancellation behind an inspector.
Export cancellation is separate from Core Stop and does not roll back an
attempted edge. Native I/O may remain pending; frontend ownership must last
through actual task/buffer disposal and join.

The connected Slint1.18.1 frontend supplies selection/destination sheets, bounded
run/item/part views, retained-report controls, confirmation, technical disclosure
and protected export over the shared session. Supplied SVGs are embedded unchanged;
necessary native integration reads appearance/contrast without changing it.
Actual local development-window observations exercised message selection, a real
Core-backed run, unchanged original and hand-reviewed golden bytes, explicit
report save, part metadata, success artwork and idle Quit cancel/confirmation.
The directly owned detail-view process returned exit0 after confirmed Quit.
These windows preceded subsequent layout refinements; current physical-frame,
compact layout, other appearance and complete accessibility checks remain open.
Temporary observation bundle identifiers isolated native panel/window saved
preferences; their two saved domains were explicitly removed after zero running
instances were observed. No product destination preference was changed by a
user choice. This is not a before/after proof for all incidental system UI state.

The actual Foundation preference checks pass both parallel owned-suite cases:
argument String overrides, session-only later choices, persisted native-byte
Strings, non-String refusal, no-write reads and fresh-process readback. Exclusive
temporary directories allocate suite namespaces. Logical zero saved keys is the
cleanup oracle; an OS-retained empty dictionary/cache is not a remaining value.
They do not establish the remaining complete app/native2 UI preference gate.

Final Core publication verification now keeps object identity alive. Unix
verification runs while the candidate remains held. Windows transfers this
lifetime to a separate metadata-only file object before releasing its strict
zero-data-sharing candidate, then performs full readback and address checks.
The existing native Windows sharing test now also retains that metadata pin;
both native CPUs still must execute these changed assertions. No Windows pass
is inferred from the local Mac checks.

## Tools, notices and scoped native evidence

Earlier private Rust developer tools were built from reviewed unchanged published sources
and compatible repository-owned locks. All four source/lock graphs were audited
with the newly built cargo-deny; no advisory ignores or broad dev exclusions were
added. Published-source inputs, private binaries, actual profiles, versions and
consumer admission are distinct evidence. Those build inputs are historical evidence. Current tools are exact-version
prerequisites resolved on PATH, with checksum-verified prebuilt installation in
CI and no source-build fallback. The source-build path and its private locks have
been removed; workspace and fuzz dependency policy remain required.

Build-generated six-target CLI legal materials include actual dependency licenses/notices, Rust legal files
and target-specific runtime/source bindings. Windows runtime source pointers are
not final selected-link evidence. Artwork path and source changes require real
generator runs; current GUI/worker legal closure must be generated and
reviewed separately. Do not infer that CLI notices cover the enabled GUI graph.

The bounded core fuzz campaign recorded1,540,438 executions without a finding;
the specified guard mutation set had15 selected mutants,14 caught and one narrowly
adjudicated compiler-invalid mutant, with none missed or timed out. These are
scoped earlier source observations; final changed-input/exact-head runs remain
required.

Prior scoped native Unix CLI archive checks passed actual extraction, CPU/runtime,
independent golden behavior and both counterfeit controls on both Mac/Linux CPUs.
Linux execution used Ubuntu22.04-class native hosts, system dependencies and max
required GLIBC2.34, below the2.35 cap. Current source changes require rerunning
those deliverables. Windows archives and complete app archives remain unqualified.

[CI run38986847036](https://github.com/resoltico/EMLAttachmentRemover/actions/runs/38986847036)
verified H=`2f281cba8b840858810ec4b1c2208b5fdf8b860b`,
base=`f7adf489734beff3ca94f829f8db1c110be316f4` and actual GitHub
M=`086ed7792072832f274ec818dfec0ca3cf428988`, with ordered parents[base,H].
Actual M Linux workspace tests discovered and passed338 cases,24 positive plus
four expected empty summaries, without failures/ignores/filters. Final live-ref
reobservation passed. The actual skipped-need control accepted its positive
baseline then rejected only the changed required job as
required_job_skipped/repository. This is that source pair's Linux integration,
not current Slint or complete native/artifact qualification.

## Required open gates

- Windows owned process/console/Job supervision, private extraction and final
  PE import/delay-import/selected-link provenance, including rejection controls.
- Actual owned exFAT image creation/mount/sync/publication/cleanup evidence.
- Both native macOS complete Slint/worker builds with the pinned Xcode, ad-hoc
  signatures, owner icon, enabled-graph legal/native input review, bundle/archive
  readback and extracted behavior. ARM actool icon compilation currently fails;
  it remains a separate required icon gate.
- Full GUI ingress/picker/Services/drop, destination/session preference, queue,
  report/export/reveal, Stop/Force Stop/active Quit, hide/reopen, appearance,
  international-text, accessibility/VoiceOver and screenshot evidence.
- Complete reusable native6 qualification, exact six receipts/eight archives,
  source/event/run/attempt aggregation and always-running ci-ok rejection controls.
- Release-check, release workflow/source/tag/asset validation and final clean-clone
  exact delivered-head verification. No release execution is authorized.

Failed/unrun gates remain failed/unrun. Checksums identify bytes; they do not
replace source review, real native integration or owner acceptance.

## Windows detect-before-bytes correction

At `39f7ee34217742de377303deaab47f7ac8d91aac`, the [native Windows run](https://github.com/resoltico/EMLAttachmentRemover/actions/runs/39091719923)
passed the unfiltered workspace selection, required-name discovery, standalone
filesystem probe and one-off rejection experiments on both GNU LLVM native CPUs.
The required test was actually compiled out and then marked ignored in separate
owned checkouts; both cases were refused by exact name. Disabling only the
containment predicate made the product gate fail with native exit 101 and 513
message bytes observed through the exact object independently placed at the
redirect target. Both originals remained unchanged. The protected product run
observed zero bytes there and no write callback. The mutation was restored and
its tracked checkout verified clean. The one-off workflow/script is now removed;
the positive gates and named-sentinel check remain required.

The first experiment at `b8d020f` failed its evidence check: directory enumeration
reported the old zero length despite a completed write. The strengthened observer
retains independent target placement/full-ID evidence and queries size through
the exact live file handle before cleanup can erase the effect. The initial
failed experiment remains [recorded](https://github.com/resoltico/EMLAttachmentRemover/actions/runs/39091321146).
This was an inadequate size oracle, not a passing product containment claim.
The earlier category-only failure at `9b403ba` remains distinct from the fully
passing product/standalone run at [47a3fc3](https://github.com/resoltico/EMLAttachmentRemover/actions/runs/39090882290).

Generated CLI legal output is no longer tracked. The complete reviewed package,
Rust-library and native-runtime source materials remain under `legal/`; build
commands generate all accompanying files from those inputs. Local generation
passed all six selected CLI targets after review of the opt-in fixture-only
manifest additions. The first generation refusal is retained; it correctly
reported an outdated Core manifest review input. GUI/worker legal review is still
separate. The local native CLI archive attempt remains unrun beyond its required
Xcode preflight because Xcode26.3/17C529 is unavailable on this host. The renamed
`build-macos-sources` command describes its actual executable-only output and
returns success when those outputs validate; it does not qualify a complete app.

CI consolidation, complete app/archive production, matching macOS15 smoke and the
final six-platform aggregate remain open. No current source-only job is evidence
for those gates.

## Enabled desktop legal inputs and app archive prerequisites

At `f9da54984a2a76f7b7431c0acb645a4db4f4285a`, the [native boundary run](https://github.com/resoltico/EMLAttachmentRemover/actions/runs/39092468611)
passed on both Linux and both macOS CPUs. Linux passed actual native CLI archive
creation, extraction, runtime/golden checks and counterfeit controls using the
new build-time legal generation path. Both Macs passed pinned Xcode desktop
fixtures and native GUI/worker release executable/ad-hoc-signature checks. These
are executable/library observations, not complete app archives or GUI acceptance.
The [Windows run](https://github.com/resoltico/EMLAttachmentRemover/actions/runs/39092468604)
also passed both native CPUs at that head.

The enabled desktop source/material pass now covers both GUI target graphs and
both private-worker graphs in addition to six CLI graphs. Complete generation
passed all ten profiles locally, with 267/268 GUI bindings and 39/40 worker bindings
on ARM/Intel respectively, including ten explicit Slint license supplements per
GUI graph. The source catalog separately checked 718 raw source members against
published archive bytes identified by Cargo.lock. Grant/condition/disclaimer
bodies and source-specific notices were reviewed separately from those identity
checks; the review records its method and limits, including metadata-only MIT
attribution information in dispatch and objc2's unchanged Apple SDK discussion.
It is implementation-agent material review, not external independent legal review.
Target-selected enabled GUI cargo-deny advisory/source/license checks passed for
both macOS triples. The unsupported all-platform/all-features GUI selection
reported additional Linux/Windows-only findings; it is not a shipping GUI scope
and is not reported as a passing all-feature audit.

The distribution policy separates native macOS26 build hosts from matching
macOS15 app smoke hosts while retaining deployment minimum15.0 and exact
Xcode26.3/17C529. A one-attempt icon producer validates actual outputs and records
whether a settled actool failure permits the approved alternate host. Timeout,
cancellation, uncertain cleanup and output-validation failure do not qualify for
that fallback. Native execution of this producer on the new hosts remains open.

A local owned synthetic fixture exercised actual ditto ZIP output. The app ZIP
profile validates original records before a filename map, signed data descriptors,
the finite Unix time/uid/gid metadata shape, exact expected files/modes and CRC
readback. It rejected duplicate original names, changed descriptors, changed
metadata and wrong expected bytes. All 22 archive tests passed, including existing
CLI rejection/readback controls; CLI ZIPs still forbid extensions/descriptors.
This is format-boundary evidence, not an app build, extracted app execution or
GUI acceptance. Complete bundle assembly and native floor smoke remain open.

## macOS26 different-UID probe correction

At `26cdd708549297d2239c5c36a5cd16f642057dcb`, native macOS26 ARM passed
pinned-Xcode desktop/library checks and the strict different-UID rename/removal
checks for admitted private, self-sticky and trusted-root-sticky parents. The
[namespace probe then failed](https://github.com/resoltico/EMLAttachmentRemover/actions/runs/39096472740/job/117583965544)
because another UID could rename a manually seeded 0700 directory in the shared
non-sticky 0777 parent. Older Darwin runs had denied that extra operation.

The product had already refused that parent before mkdir. Its admission predicate
requires the current/trusted-root owner and either no group/other-write bits or
the supported sticky policy, plus the Darwin ACL predicate; cleanup rechecks the
same protection. No product safety argument uses the extra denial on a refused
parent. The obsolete macOS-only assertion is removed. All three admitted-parent
protection checks remain strict, and the real other-UID rmdir/replacement control
on the refused parent remains required. This corrects a probe assumption without
relaxing product admission or cleanup. Corrected native execution remains pending;
the original failure is retained rather than reported as passing.

The corrected namespace probe and native desktop executable checks passed on
both macOS26 CPUs at [a33b106](https://github.com/resoltico/EMLAttachmentRemover/actions/runs/39097429129),
along with both native Linux archive checks and complete legal generation.
The obsolete refused-parent assertion was not a product safety premise; all
admitted-parent protections and real other-UID replacement controls remain.

## Consolidated CI and complete bundle path

`ci.yml` replaces native-boundaries.yml and filesystem-feasibility.yml while
retaining their product/native controls. ci-identity.yml, ci-refs and the ci-*
subcommands are removed; ordinary PR merge checkout and actual checkout recording
replace that subsystem under WD5. macos-icon-diagnostic.yml is manual only. The
new CI includes six native test rows, the enabled Slint/owned Darwin fixtures,
POSIX different-UID controls and standalone filesystem probes, dependency policy,
complete legal production, a shared icon with the approved alternate-host rule,
two native macOS26 app/CLI producers and matching macOS15 extracted-byte smoke.
`ci-ok` always runs and requires every direct required job/matrix to succeed.
Local aggregate controls accepted the positive set and refused failure,
cancellation, skip, missing job and missing result. These controls are decision-
logic evidence only; actual GitHub ci-ok remains required.

The complete app path now assembles an actual bundle, preserves the independently
signed worker bytes when signing the outer app ad hoc, checks all native images
with Mach-O parsing/vtool/system-dependency/signature inspection, and uses ditto
creation/extraction. A frozen producer file/mode reference is checked before
native extraction; hashes bind bytes rather than review quality. The extracted
worker executes through the production supervisor/Core for Created and UpToDate
with independent goldens and unchanged originals. Hosted GUI startup uses the
real exact owned process and OS window metadata, then explicitly settles that
quiescent process; it does not claim content, accessibility or UI Quit acceptance.
Normal owner preferences are protected by refusing GUI startup outside owned
hosted-runner context. The same archive/reference pair goes to matching floor
hosts; missing current producer artifacts fail rather than selecting another run.

Local checks passed native test discovery/execution, 44 xtask command tests,
22 archive library tests, strict workspace Clippy, both Windows target
Clippy checks, workflow lint and workspace/fuzz dependency policies. The new
readonly window observer compiled with the local SDK27 as supplementary evidence.
Pinned local complete app execution is unrun because Xcode26.3/17C529 is absent.
Complete app production and actual macOS15 smoke in the new CI are pending; no
build, checksum or synthetic archive-format fixture substitutes for those gates.
The earlier 641aa1a enum/path-name collision failed compilation and is retained;
26cdd70 fixed it and passed the affected command tests and three Clippy selections.

## Windows/CI correction qualified at branch head

At H=`6ac5a729ea60a7248a9c5580f6fdd971a3c97d40`, [CI39099729375](https://github.com/resoltico/EMLAttachmentRemover/actions/runs/39099729375)
and its always-running ci-ok passed. All six native test matrices passed,
including both Windows product P-W gates/standalone primitives, both macOS
qualified Slint/Darwin fixture sets and all POSIX different-UID controls. The
macOS26 icon producer succeeded once; the approved alternate host was unnecessary.
Both separate native app/CLI archive producers passed; their same archive bytes
then passed actual macOS15 ARM and Intel extraction/signature/runtime/Core-worker
and GUI-startup smoke. This establishes the scoped archive/startup gates, not
full GUI paint/interaction/VoiceOver acceptance. Main remains published baseline
`f7adf489734beff3ca94f829f8db1c110be316f4`; the corresponding PR merge candidate was
M=`9dde7c86eface828d1421aa05270d55ed814e467`. The [full M run](https://github.com/resoltico/EMLAttachmentRemover/actions/runs/39099733317)
remains separately tracked; it is not relabeled as H.

The first floor matrix at 9ebcdee was unexpectedly skipped through the optional
icon-fallback dependency despite successful app producers. ci-ok refused that
state. An explicit always()+successful-required-needs condition corrected the
scheduler gate; the subsequent real floor jobs passed. Missing/skipped outcomes
were never reported as success.

Fresh preparation fuzz at 9ebcdee passed 1,710,574 executions with 12 independent
fixtures plus 10 structural seeds. At 7ee027d the complete named guard mutation set
ran 15 cases: 14 caught, one reviewed compiler-invalid, none missed or timed out.
The earlier escaped 16-case discovery and unmutated-baseline timeout are retained
as failures, not successful campaigns. The selected guard functions did not change.

Owned exFAT integration is now implemented for fresh hosted Macs with native
hdiutil create/attach/info/detach, exact backing-file/device/mount association,
actual RENAME_EXCL/conflict checks, real mode/ownership/synchronization facts and
Core golden/original/physical-publication observations. Cleanup has no force or
name-pattern sweep. Pure controls reject ambiguous/wrong-mount/device metadata;
local source/Clippy checks pass. Actual disk-image execution remains required and
unrun. This does not complete original WP3 or the remaining Windows packaging,
GUI interaction/accessibility and release-identity/clean-delivery requirements.

## Desktop ingress and presentation checkpoint

Native file-open and Services callbacks now reserve a bounded batch before
reading external payloads. Collection releases the broker borrow before native
calls and refuses nested collectors. The actual delegate remains Winit's;
installation adds only five absent, signature-checked methods. A uniquely owned
native pasteboard test round-trips a file URL containing reserved characters,
a newline and non-BMP text, and rejects a mixed file/text selection without
silently filtering it. No global pasteboard is modified. Drop integration and
actual Finder/Services batch delivery remain separate required checks.

The view uses the application library's eight-waiting-request limit, retains
native dialog callbacks through settlement, presents run-level failures as
failure/attention artwork, and retains the first system notice until dismissed.
Finder reveal uses only an earlier qualified native address, checks availability
off the UI thread, and waits for its owned task during Quit. It makes no fresh
identity/content claim. License text is available from Help. Item/part paging
controls reflect the actual bounded report pages.

Local strict GUI Clippy and the GUI test selection passed, including ten unit
tests and the existing two real preference-domain integration tests. A private,
ad-hoc-signed synthetic bundle started with the new hooks and real Ready window.
Its marker-owned preference domain was removed only after both observed app
instances exited; a fresh observer found zero keys. This supplementary SDK27
observation did not select files through the native picker or prove processing,
drop, Services, accessibility, close/reopen or final native archive acceptance.
No owner destination preference was changed. Complete GUI acceptance remains open.

The desktop supervisor now checks a captured output before even probing the
worker executable, on its existing owned background thread. The Core binding
routine is shared with collection, so the GUI introduces no filesystem policy.
This read-only diagnostic does not authorize subsequent processing: collection
rebinds the output and publication retains its existing checks. A missing output
produces a settled, reusable Request refusal with actionable destination guidance,
no fabricated process exit and no request sent. A real missing-helper negative
control demonstrates that output refusal precedes launch. Nine actual supervised
worker tests and the Core read-only/file-versus-directory/disappearance test
passed locally, along with strict GUI Clippy. Blocking native I/O remains owned
until actual settlement; this does not claim a hard deadline for filesystem calls.

## Native drop source bridge

The published winit0.30.13 archive was inspected before extraction:217 regular
members, no link/path escape. A member-by-member byte comparison confirms all
upstream files are retained; only src/platform/macos.rs and
src/platform_impl/macos/window_delegate.rs are modified, plus a new explanatory
EML-PATCH.md. Both modified files carry explicit change notices. LICENSE is
unchanged. Cargo.lock changes only winit's registry source/checksum to a local
path source. The patch exposes one synchronous native pasteboard callback on the
existing delegate, corrects draggingEntered's NSDragOperation ABI, permits only
Copy and removes the lossy per-file drop path. The application uses its existing
bounded/reentrancy-aware ingress and native file-URL reader, with distinct
non-enqueuing inspection and committing delivery. The callback is bound through
Slint's Winit attributes hook when each native application window is created,
including recreated windows; it does not depend on a post-show raw-window lookup.

Both actual generator graphs retain259/260 GUI package identities and the same
license expressions, with winit explicitly recorded as reviewed retained source.
Full legal generation passed all ten profiles and produced602 files, including
the unchanged upstream license and shipped patch notice. Ten legal controls,
strict GUI/tooling Clippy, GUI tests and both Windows Core/application target
Clippy checks passed locally. The first patch compile exposed a missing second
WindowExtMacOS implementation; it was corrected before this checkpoint. Native
drag delivery, GUI frame/interaction and final archive qualification remain open.
Edit and Window commands use existing Slint APIs; technical detail expands in the
same read-only report area, avoiding a competing text pane in compact layouts.

## Hosted interaction and read-only release admission preparation

The real XCTest harness and Swift6 fresh preference observer compile locally
with SDK27; strict xtask Clippy passes. This is source/tooling evidence only.
The first observer compile used the Command Line Tools without its required SDK
and failed standard-library loading; the explicit Xcode SDK attempt then exposed
an invalid CocoaError constant. Both diagnostics remain retained. The corrected
observer compiles with the selected SDK and an explicit native ARM/minimum15
target. The current xtask tests passed22 archive,47 command and19 process cases;
Windows-target tooling Clippy and workflow lint also passed.
The proposed hosted run uses the exact already-inspected extracted app while its
private enclosure remains held, receives NSWorkspace's exact new instance,
delivers two local URLs as one Open With batch and checks both original/golden
bytes after rendered worker settlement. It retains Ready/Finished screenshots,
checks Quit cancel/confirm and observes complete persistent-domain equality from
fresh before/after processes. Native pinned-Xcode execution on both CPUs remains
required. File/folder pickers, drag, Services, queue/retention, active Stop/Force
Stop/Quit, other frames and accessibility/appearance checks remain open.

ExFAT image creation failed at30faf41, at PR merge4185d7e/run39103853108
with the attempted administrator route, and locally at4aca758. All returned
Operation not permitted before the explicit probe attach/Core steps. Local native
hdiutil debug output established the cause: newfs_exfat rejected the volume
label's length, then hdiutil translated that formatter status into EPERM. The
native newfs_exfat manual specifies at most11 UTF16 characters. The label is now
EMLPROBE; the unnecessary sudo path is completely removed. These failed attempts
remain failures, not exFAT integration evidence or proof of a host privilege limit.

The same ordinary-user owned-image probe works locally or on CI. It requires a
clean committed checkout, creates only its admitted private enclosure/image,
retains exact mount/device/backing-file ownership checks and does not run a GUI.
The corrected actual volume result remains pending.

That merge's app startup also caught an integration defect in the first drop
hook: Slint had not yet materialized a native window immediately after show, so
the runtime setter returned ParentUnavailable and the app exited1. The patch now
binds an immutable receiver through native window-creation attributes and removes
the runtime setter/lookup entirely. Local strict Clippy and GUI tests pass after
the correction. The original real startup failure remains explicit; native
startup/interaction must be rerun at the corrected source.

Read-only release admission has two passing negative-control tests covering
Unreleased, empty/duplicate/invalid-date sections and API failures/published or
wrong-source releases. An actual authenticated GitHub404 response established
the parser's observed response format. This does not establish release readiness:
v6 has no dated released section, and no tag/release/assets/settings were changed.
