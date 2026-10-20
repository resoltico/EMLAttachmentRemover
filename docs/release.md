# Owner release procedure

Publication requires an explicit owner release decision. Local tests and
workflow lint do not establish live attestation or publication integration.

## Prepare and dispatch

Authenticate `gh` for this repository. Select a full commit on main containing the
workspace version and release notes, fetch main, and verify read-only admission
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
published history. Dispatch the workflow from main. Read-only admission checks
the exact clean checkout, current remote main ancestry, version, notes, tag and
actual API release state. An API failure is never treated as absence.

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
normal success or the explicitly owner-authorized controlled-host product pass
after a retained known normal failure. Modified-host mode retains the same-run
normal failure, complete preparation and actual product; it makes no healthy
unmodified-host claim. The shared picker predicates independently bind S, workflow
W, current run/attempt, archive/reference bytes, native host, method, preferences
and normal settlement. Unknown failures and incomplete controlled attempts block
both the reusable aggregate and release. Both full macOS15 consumers remain required.
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

Only the publication job has release, attestation and OIDC write permissions.
It repeats admission, requires an absent release, creates an unpublished draft,
and uploads without clobbering. An actual API response feeds
`cargo xtask check-release-server --assets DIR --release RESPONSE.json`:
exact source/version/draft, nine distinct positive asset IDs, exact names,
uploaded state, sizes and SHA256 digests must match the local immutable set.
The manifest itself is regenerated from the actual local archive bytes before
comparison. This read-only command trusts the caller to obtain the response
from the correct repository; the workflow supplies that boundary directly.
Only after these checks does the workflow publish the draft.

After success or failure, the workflow observes server state and retains its
responses, assembly/admission records, source/policy predicate and diagnostics.
No failure handler deletes a release, replaces assets or resets a tag. Existing
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
