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
cargo xtask release-check --source FULL_SHA --version 6.0.0
```

Resolve every admission failure before dispatch. After the owner authorizes
publication, start the workflow from main with the same source and version:

```sh
gh workflow run release.yml --ref main -f source=FULL_SHA -f version=6.0.0
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

The same reusable native qualification workflow runs on S. All six native test
jobs, both native app candidate producers, both complete minimum-OS desktop
consumers, the macOS26 Intel picker compatibility check and the strict aggregate
must pass. Products are selected only from this run/attempt's six
platform artifacts, downloaded into separate directories with digest mismatch
treated as an error. Their source records must bind to S. Consumer evidence is downloaded separately from
products. Preparation requires each candidate's `native_candidate` stage plus
matching `desktop_qualification` and, for Intel, `picker_qualification` envelopes
from the same run/attempt, source, target and archive bytes. The selected product
must have case `picker` and method `testOwnedNativePickerCompatibility`;
it cannot substitute for either full macOS15 desktop consumer. Failed, skipped,
missing or mismatched case receipts cannot admit an app. Intel envelopes admit
only `mode: modified_host`: guarded preparation on the image declared by
`xtask/distribution.toml`, followed by the complete unchanged product case and
read-only host/service verification. Preparation requires recognized current-boot
IconServices crash evidence, then records the exact GUI-user agent bootout and
disable operations. The shared picker predicates independently bind S, workflow
W, current run/attempt, configured policy, archive/reference bytes, native host,
method, preferences and normal settlement. Preparation and final verification
must agree on boot and caller context; the intervening product's observed OS and
CPU must match. The product does not directly report its boot UUID. This is
modified-host coverage, not general unmodified-host compatibility or causal proof
of recovery. Unconfigured hosts, malformed crash reports, absence of the required
current-boot fault and incomplete attempts block both the reusable aggregate and
release. Both full macOS15 consumers
remain required.
The policy derives exactly eight archive names. `stage-release-assets` freezes their actual bytes,
checks copy readback and writes the sorted SHA256SUMS as the ninth asset.
Existing native checks establish behavior; artifact digests preserve the tested
bytes through transport. Assembly alone establishes neither property.

The pinned `actions/attest` action uses an explicit qualification predicate at
`https://github.com/resoltico/EMLAttachmentRemover/attestations/native-qualification/v1`.
It records S, the distinct dispatch commit and workflow ref/SHA, native run and
attempt, policy and assembly identities, and compact actual-host/desktop/picker
case observations from the validated consumer records. All eight archives and SHA256SUMS are
subjects. This is a qualification/assembly statement, not default SLSA build
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
`cargo xtask check-release-server --assets DIR --release RESPONSE.json`:
exact source/version/draft, nine distinct positive asset IDs, exact names,
uploaded state, sizes and SHA256 digests must match the local immutable set.
The manifest itself is regenerated from the actual local archive bytes before
comparison. This read-only command trusts the caller to obtain the response
from the correct repository; the workflow supplies that boundary directly.
Only after these checks does the workflow publish the draft.

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
source. `BUILD_NUMBER` in `crates/eml-protocol/src/product.rs` is the sole product
build-number source, shared by CLI display, GUI About and `CFBundleVersion`.
Increase that positive integer (1–9999) before distributing changed binaries;
never reset it on a release-version change. Private dirty builds, both CPU
variants, CI reruns and source rebuilds reuse the committed value; compilation
never increments it. The build number identifies a distributed product revision,
not exact bytes or qualification. Checksums identify delivered bytes. Machine
protocol version fields remain the release version, independently of the display
build number.
