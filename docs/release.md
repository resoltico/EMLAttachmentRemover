# Owner release procedure

Publication requires an explicit owner release decision. Local tests and
workflow lint do not establish live attestation or publication integration.

## Prepare and dispatch

Authenticate `gh` with repository push access so it can see unpublished drafts.
Select a full commit on main containing the
workspace version and release notes and the same workflow tree as current main,
fetch main, and verify read-only admission
from a clean checkout of that commit:

```sh
git fetch origin main
cargo xtask release-check --source FULL_SHA --version 7.0.0
```

Before distributing changed product binaries, independently authenticate the
previous published app archive and extract its `Contents/Info.plist`. With Python
3 on PATH, compare that retained file against the current source:

```sh
cargo xtask check-product-identity --previous-info "/path/to/previous/EML Attachment Remover.app/Contents/Info.plist"
```

Retain the command's prior-file identity and ordering result with preparation
evidence. This command checks app identity, canonical version/build values and
monotonic ordering; it does not authenticate the caller's file, prove equal
binaries when values match, or authorize publication. The owner must establish
the prior archive's authenticity separately. The pending 7.0.0 source uses Build
36, above published 6.0.0 Build 35; these undistributed packaging changes reuse 36.

Resolve every admission failure before dispatch. After the owner authorizes
publication, start the workflow from main with the same source and version:

```sh
gh workflow run release.yml --ref main -f source=FULL_SHA -f version=7.0.0
```

Follow the resulting run through qualification and publication; a successful
dispatch only acknowledges scheduling. Inspect failed-run server observations
before attempting recovery.

## Admission and qualification

After owner review and a separate release decision, the `Owner release` workflow
accepts a full source commit S on main and the matching Cargo version. S must
contain one nonempty dated changelog section while retaining Unreleased and
published history. Dispatch the workflow from main. Trusted workflow steps verify
S against current remote main before executing any repository code, including
ancestry and equality of the workflow trees. A source with different workflows
would require a workflow-write credential that `GITHUB_TOKEN` cannot provide.
Read-only admission operations check the exact clean checkout, version, notes,
tag and draft-aware API release state. Only a successful GraphQL lookup with an
explicit null release establishes absence; a resolved release ID must return a
matching REST response. API errors and inaccessible IDs never establish absence.
Local callers must supply push-capable credentials; a read-only credential can
hide drafts and cannot establish their absence.

The reusable native qualification workflow runs on S. Its native, app, complete
desktop-floor and picker matrices are rendered from the one immutable typed
`BuildPlan` loaded from Cargo metadata, `xtask/distribution.toml` and tool pins.
The approved six CLI products, two macOS app products, both macOS15 desktop
consumers and macOS26 Intel picker role have one independent ordinary regression
oracle. Missing or skipped required jobs fail the explicit CI-result admission.

`cargo xtask verify-release-inputs --products DIR --desktop DIR --picker DIR --out DIR`
replaces the former shell inventory loop. It admits the exact declared product set
and current source, run, attempt, workflow ref/SHA, native target and archive bytes.
App references and full desktop consumer records must match the actual frozen
archive/reference identities. Picker receipts use the app's declared picker role;
ordinary-host and admitted incident-host evidence remain distinct, and the raw
product receipt bytes must match their typed projection. Failed, missing, skipped,
duplicate or mismatched evidence refuses admission before assembly. No receipt or
digest alone establishes native execution.

Desktop admission binds ordinary workflow, pagination and appearance to the
shipping archive, and the 18 controlled cases to a separate producer-frozen
companion. Its closed reference binds the source, CPU, epoch, workflow and
shipping input, with actual GUI-code, signature, feature/material and native
verification facts. Producer and floor observations must agree. The verified
input manifest retains this nonshipping binding; asset assembly excludes the
companion archive and reference.

The versioned `verified-inputs.json` retains the exact frozen native documents and
identities, typed picker receipt and source/run binding. Archive bytes are copied
with exclusive creation, synchronized and read back. This manifest is admitted
data, not a filesystem mutation capability or continuing release authorization.
`cargo xtask stage-release-assets --inputs verified-inputs.json --out DIR` checks
those identities again and assembles the exact planned archive set plus the typed
`SHA256SUMS` auxiliary asset. Counts derive from the expected set. No production
consumer maintains another six/eight/nine inventory.

The pinned `actions/attest` action uses an explicit qualification predicate at
`https://github.com/resoltico/EMLAttachmentRemover/attestations/native-qualification/v1`.
It records S, the distinct dispatch commit and workflow ref/SHA, native run and
attempt, policy and assembly identities, and compact actual-host/desktop/picker
case observations from the validated consumer records. Every admitted archive and
SHA256SUMS is a subject. This is a qualification/assembly statement, not default SLSA build
provenance pretending that a checkout override changes the dispatch identity.
Full native job observations and build/test records remain in run artifacts.

Admission and publication receive `contents: write` credentials because draft
visibility requires push access. Admission performs only read operations on Git refs and releases.
Attestation and OIDC write scopes belong only to publication. Publication rechecks
S against current main before executing repository code, repeats admission,
requires an absent release, creates an unpublished draft and uploads without
clobbering. Draft-aware lookup resolves its numeric ID before reading the raw
REST response; the release-by-tag REST endpoint only finds published releases.
That actual API response feeds
`cargo xtask check-release-server --inputs verified-inputs.json --assets DIR --release RESPONSE.json`:
exact source/version/draft, distinct positive IDs for the complete planned asset set, exact names,
uploaded state, sizes and SHA256 digests must match the local immutable set.
The manifest itself is regenerated from the actual local archive bytes before
comparison. This read-only command trusts the caller to obtain the response
from the correct repository; the workflow supplies that boundary directly.
The final read-only predicate then revalidates each exact modified-host incident
identity and its current validity interval. Ordinary-host receipts never load the
incident registry. Expired, replaced or malformed incident policy leaves the
uploaded release as a draft; no policy renewal is substituted for the original
receipt. There is no staging, upload or attestation between this predicate and the
external `gh release edit --draft=false` call. That call still has a small external
time-of-check interval; admission and GitHub publication are not one atomic edge.

After success or failure, the workflow observes server state and retains its
draft-aware ID lookup and REST responses, assembly/admission records,
source/policy predicate and diagnostics. Failed final lookups retain their errors
and leave server state uncertain; they do not assert absence. No failure handler deletes a release, replaces assets or resets a tag. Existing
tags are always refused. A matching unpublished draft is recognized by admission
but automatic publication still refuses it: inspect its actual assets and tag
state and obtain an owner recovery decision. A failed action does not prove the
server did nothing. Retaining the draft makes recovery possible without silently
retargeting or replacing anything already published.

## Version and build identity

`workspace.package.version` in the root `Cargo.toml` is the sole release-version
source. `BUILD_NUMBER` in `crates/eml-core/src/product.rs` is the sole product
build-number source, shared by CLI display, GUI About and `CFBundleVersion`.
Increase that positive integer (1–9999) before distributing changed binaries;
never reset it on a release-version change. Private dirty builds, both CPU
variants, CI reruns and source rebuilds reuse the committed value; compilation
never increments it. The build number identifies a distributed product revision,
not exact bytes or qualification. Checksums identify delivered bytes. Machine
protocol version fields remain the release version, independently of the display
build number.

## Packaging source and timestamps

The typed `BuildPlan` admits one source commit, UTC epoch and provenance for all
packagers and readers. In a Git checkout, the epoch comes from that exact commit's
committer timestamp. An explicit `EML_SOURCE_COMMIT` or `SOURCE_DATE_EPOCH` must
match the admitted Git input. Packaging rechecks the held commit/epoch and clean
worktree; it does not silently acquire a later commit as authority.

Without `.git`, standalone packaging requires both a full `EML_SOURCE_COMMIT`
and decimal `SOURCE_DATE_EPOCH` from the source archive's metadata. The epoch
must be in 315532800..4294967295 UTC Unix seconds. These inputs are recorded as
`declared_archive`; they identify the caller's declaration without proving the
archive matches a verified Git tree. Qualification and release admission require
`git_commit` provenance and reject declared archives. There is no wallclock,
zero or default-1980 fallback.

CLI archives are `emlattachmentremover-VERSION-OS-CPU.tar.gz` on macOS/Linux and
`.zip` on Windows. Their root and executable use `emlattachmentremover`, with
`.exe` on Windows. The two Mac desktop archives are
`emlattachmentremover-gui-VERSION-macos-CPU.zip`; the human bundle name,
`Contents/MacOS/EMLAttachmentRemover`, bundle identifier and private `eml-worker`
keep their existing identities. The exact eight archive names come from
`cargo xtask distribution`; no other GUI product is introduced. Published
archives and their historical names remain immutable.

TAR members and gzip headers retain the exact source epoch. ZIP timestamps use
UTC with two-second precision, rounded down; references retain the exact source
epoch. Native Mac apps keep the separate ditto metadata profile. After signing
and final byte/mode/signature reads, the builder normalizes all staged files and
directories to the derived ZIP epoch, then runs ditto with `TZ=UTC`. This ordering
also fixes native access-time metadata without modifying signed bytes or modes.
Readback and native extraction check timestamps as well as existing inventories,
resources, permissions, strict signatures and actual private-worker execution.
The same fixed staged inputs and epoch must produce identical archive bytes;
that property does not establish reproducible compiled binaries.
