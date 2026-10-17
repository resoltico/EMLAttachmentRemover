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

Five native eml-macos fixtures passed locally: stdio/EOF/normal reap; Interrupt
while waiting for input; live-child Kill; deliberately blocked spawning-thread
SIGINT mask reset; and failed spawn with no child authority. Post-reap signals
were refused. Immediate capability retention and visible bounded failure cleanup
are source-reviewed. These fixtures do not establish the complete production
supervisor, inherited-descriptor or GUI lifecycle boundaries.

Two actual private-worker process tests exercise GUI request transport directly
to core: byte-exact Created/UpToDate with unchanged source, plus worker version,
malformed input and private-argument refusal. Held-open/exact/oversize stdin,
first/second signals and closed-output paths still need dedicated worker evidence.

The initial Slint1.18.1 Winit/software/system-font/accessibility screen builds
natively. An explicit ARM minimum15 build has thin ARM64 Mach-O, min15.0/SDK27.0.
No window was launched locally and no saved owner preference was changed.
Software rendering is an initial implementation profile, not an owner requirement
or a claim about actual screen-reader/international-text behavior. Screen control
wiring and the required macOS system integrations remain incomplete.

## Tools, notices and scoped native evidence

Private Rust developer tools were built from reviewed unchanged published sources
and compatible repository-owned locks. All four source/lock graphs were audited
with the newly built cargo-deny; no advisory ignores or broad dev exclusions were
added. Published-source inputs, private binaries, actual profiles, versions and
consumer admission are distinct evidence. Selected paths have no global-tool or
PATH fallback. Current workspace changes require dependent graph checks again.

Six CLI legal trees include actual dependency licenses/notices, Rust legal files
and target-specific runtime/source bindings. Windows runtime source pointers are
not final selected-link evidence. Artwork path and source changes require real
generator/freshness runs; current GUI/worker legal closure must be generated and
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

- Complete native Windows property-equivalent confinement evidence; the required
  attribute-only directory writer gate remains failed on both CPUs. Other passing
  cases and same-SID restricted-token observations do not waive it or establish a
  different-user boundary.
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
