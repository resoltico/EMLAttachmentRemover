# Protocol scenarios

These fixed JSON Lines are hand-authored contract examples, independent of the
Rust renderer. `@VERSION@` is substituted by each consumer's test version.
`posix/1:2` is an opaque normalized test identity, not execution evidence. The
fixed fsync/cleanup observations represent specified outcomes; actual native
receipts are tested separately. Only checked native identities and OS receipt
observations are normalized when Rust compares actual attachment events.

The two attachment scenarios use independent MIME fixture01:1028 input bytes,
513 golden bytes and one507-byte entity at section2, application/pdf,
report.pdf, reason attachment. File savings are515, demonstrating why the app
must not add entity sizes to calculate savings. Other success scenarios use the
34-byte plain message from the planned-create test. No message payload is part
of an event.

The cases cover new and existing copies, both plans, no inputs, preparation and
existing-sync failures, observed/unknown publication, gapped root admission,
collection limits, pre/post-edge cancellation, incomplete per-item discovery
failure, raw control/non-Unicode path data and qualified limited full-sync
fallback. Partial namespace, unattempted required edges and confirmed cleanup
with an earlier native error are valid failure observations; they cannot qualify
success. The invalid request has no event stream and must exit2.

The Rust frontend decoder and eml-app splitter/reducer consume all22 JSONL cases,
including every byte split. CLI and private-worker process checks exercise real
Create/UpToDate operations against the independent golden. The invalid request
has no event stream. These examples do not substitute for native interruption,
filesystem failure, extracted archive, app or GUI integration. Required shipping
qualification remains tracked separately.
