# Owner release procedure

v6 remains Unreleased. Implementing this workflow does not authorize dispatch,
tagging or publication. The current implementation has not run publication or
attestation against GitHub; parser tests and workflow lint are source evidence.

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
matching `desktop_qualification` and, for Intel, `picker_compatibility` receipts
from the same run/attempt, source, target and archive bytes. Failed, skipped,
missing or mismatched case receipts cannot admit an app. A known compatibility
failure continues to block the reusable aggregate and release preparation.
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
