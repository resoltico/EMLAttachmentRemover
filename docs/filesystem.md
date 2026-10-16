# Filesystem implementation contract corrections

The native filesystem business workflow is still being implemented. These
reviewed corrections preserve the safety requirements; they are not product
qualification evidence. The original design archive and checksum remain intact.

## Typed ordinary-file observations

`BoundFileError` applies to sources and existing destinations. Initial observed
nonregular type is `NotRegular`; a proved identity/signature/parent mismatch is
`Changed`. Once an ordinary file is bound, a newly observed nonregular type is
also a change from that baseline. Failed queries and unusable metadata retain
their exact native/validation cause as `ObservationFailed`; neither English
wording, `InvalidData` nor a missing-entry error is treated as proof of change.
The Unix inventory adapter passes through the original I/O error unchanged.

Windows preserves its existing access, sharing and final-reparse flags. A denied
directory open can remain an inspection failure; no backup flag or privilege is
added to force type classification. The parent helper returns a transient handle
and its observed identity; the caller retains only that identity before opening
the current entry. This preserves the previous lifetime and does not establish
atomic parent/entry continuity. `GetFileType`'s UNKNOWN result is distinguished
from API failure through its immediate thread-local error as
[Microsoft documents](https://learn.microsoft.com/en-us/windows/win32/api/fileapi/nf-fileapi-getfiletype).
The pure classifier cases are logical tests; native target execution remains
separate evidence. No publication or candidate-ACL guarantee follows from this
read/metadata typing change.

## Native source preparation capability

`eml-core::prepare_source` consumes an ordinary bound file, its inventory signature
and a per-run control. It checks that admitted version, obtains bytes in bounded
positional reads, qualifies EOF/actual length or the one-byte-over-limit prefix,
and rechecks binding before calling the existing pure MIME preparer. Short reads
are not EOF. Interrupted reads retry only after a cooperative checkpoint. A stable
oversize refusal follows actual bounded reading and source checks, rather than a
metadata-only assumption. Data-read errors and failed metadata observations retain
separate causes.

A successful `PreparedSource` retains the original read-only file and immutable
`PreparedMessage`; raw input bytes are released after preparation. Its source
length derives from the initial signature qualified against actual EOF/read count.
The constructor checks source again after MIME preparation. Exposed revalidation
returns native facts without assigning a later publication phase. The future run
workflow must still perform every required source check before/after its edge.
This capability creates no output, syncs no file, applies no run metadata budget,
emits no events and supplies no snapshot or permanent-stability guarantee.

## POSIX namespace admission before creating an entry

A pathname identity check followed by rmdir/unlinkat is not atomic compare-and-
delete. A controlled same-UID interleaving checked owned-stage, moved it aside,
moved another owned empty directory into that name, then removed the replacement.
This demonstrated the mechanism; it was not a cross-UID exploit test. Later native
Linux controls established another UID can rename an owned0700 directory under
a suitable nonsticky shared parent. Darwin's directory-rename authorization
differs, as the native checkpoint notes below explain.
A held private staging descriptor protects its contents, not its outer name.

The correction refuses namespace creation unless the held immediate parent
provides the required other-user name protection. Apply this before stage mkdir
and before every newly created mirrored-directory edge, not indiscriminately to
all ancestors. Existing directories are never chmodded or given replacement ACLs.

- Require parent owner equal captured effective UID or trusted UID 0.
- Require mode group/other write bits clear, or an applicable sticky policy.
  A foreign non-root owner is refused even when currently read-only/sticky:
  that owner can change permissions or use the sticky directory-owner exception.
- On Linux, scope this reasoning to documented/qualified POSIX mode/ACL-mask
  semantics. Group mode bits represent the POSIX ACL mask. Do not assume opaque
  NFSv4/SMB/FUSE permission projections satisfy this proof.
- On macOS, inspect ACL through the same held descriptor. ALLOW entries may have
  only known read/search/list/read-attributes/read-extattrs/read-security/
  synchronize rights. Reject data/namespace-write, append, delete/delete-child,
  write-attributes/extattrs/security or change-owner ALLOW rights, regardless of
  principal, ordering or inherit-only flags. Unknown tags/rights and inspection
  errors fail closed; a validated live descriptor's ENOENT means no ACL.
  Read-inheriting ALLOW and known deny-only entries remain admissible.

macOS ACL DELETE/DELETE_CHILD grants can override sticky restrictions, including
inherited DELETE before a new stage's ACL is cleared. Mode/sticky checks alone
are insufficient. This conservative predicate intentionally refuses some safe
own-only or ineffective write grants rather than adding a membership/ACE evaluator.

After admission, retain the original owned mkdir/open/identity/0700/descriptor-ACL-
clear sequence and candidate0600 checks before message bytes. Before cleanup,
recheck parent protection and the held/entry identities. Missing/mismatched or
unprovable protection means no pathname unlink; clean only provably owned contents
through the held private directory and retain/report residue. Same-authority or
administrator chmod/swap after observation remains outside the original threat
boundary. These checks do not create an atomic compare-delete syscall.

A shared upper ancestor may still move an existing protected held destination;
publication stays descriptor-bound and requested-address changes are reported.
An unsafe first missing mirrored edge refuses before mkdir. Identical-existing
verification needs no staging namespace mutation and is not blocked solely by
this creation predicate. Dry run performs available admission checks without writes.
Refusal uses DESTINATION_UNSUPPORTED/stage and actionable private-folder guidance.

Required cross-UID, ACL/mask, mirrored-edge, substitution, residue, synchronization
and native-platform checks remain obligations. Same-UID interleavings or source
inspection do not establish cross-UID execution or full WP3 acceptance.

### Reviewed permission models

Each held immediate mutation parent is inspected, including after a mount
crossing and before identity-dependent pathname cleanup. Linux admits the ext2/3/4
shared magic, tmpfs, XFS and Btrfs permission models; unknown types, including
OverlayFS, FUSE, NFS, CIFS, 9p and FAT/exFAT, are refused for namespace creation.
Recognition establishes the reviewed permission model, not full filesystem
qualification. Every privacy, exclusive-publication and synchronization operation
must still succeed. tmpfs is volatile: successful fsync reports an OS call, not
persistent-media survival.

Linux also reads the bounded numeric kernel overflowuid setting and refuses that
owner value as trust evidence. An unavailable or unusable setting fails closed.
Numeric ownership is interpreted under the active mount/user mappings. Another
host UID granted equivalent owner authority by administrator-configured mappings
belongs to the same-authority exclusion; absence of alternate owner-authority
aliases is not proved by stat/fstatfs. Idmapped mounts have not been adversarially
qualified. macOS refuses held volumes with MNT_IGNORE_OWNERSHIP. No mount setting
or saved owner preference is changed.

Source reads and existing-only comparisons do not require namespace creation
admission. Local APFS currently rejects invalid UTF-8 names with EILSEQ, so those
tests record that volume refusal separately from representable-name checks;
Linux must execute the invalid-byte round trip. Unit cases for model constants
or ownership flags do not establish a real mount's behavior.

Model references: [Linux VFS deletion](https://github.com/torvalds/linux/blob/v6.8/fs/namei.c#L2931-L3004),
[POSIX ACL permissions](https://github.com/torvalds/linux/blob/v6.8/fs/posix_acl.c#L375-L443),
[tmpfs](https://docs.kernel.org/filesystems/tmpfs.html),
[filesystem ownership mappings](https://www.kernel.org/doc/html/latest/filesystems/idmappings.html).

### Native checkpoint evidence

Commit `709fef041ece8de3e4d2076e7ea8fa4428662230` ran the held-filesystem
crate tests on all four native POSIX runners in [CI38792010023](https://github.com/resoltico/EMLAttachmentRemover/actions/runs/38792010023).
Linux x64/ARM each passed18 tests on actual ext-family f_type0xef53; Mac Intel/ARM
each passed20 with actual f_flags0x4909000. Compiler host triples matched the CPUs.
Local Mac tests also passed under invocation-only umask000. These observations
cover that staging checkpoint, not full WP3 or later source changes.

The native different-UID probe is an explicit developer example, built with
`cargo build --locked -p eml-fs --example namespace-credentials`. On an isolated
runner with noninteractive sudo, invoke its executable with the original nonroot
UID/GID. It verifies the actual dropped credentials and positive controls before
protection checks. It creates only a fresh owned /tmp fixture, uses held-identity
cleanup and retains the exact fixture on failure; it never sweeps a pathname.
Local sudo is unavailable without a password, so local cross-UID execution is
unrun. The workflow runs this probe on all four POSIX hosts; a missing credential
or unrelated child failure fails the gate. Linux named-ACL-mask, expanded Darwin
ACL, full orchestration, Windows product backend and archive/GUI checks remain
separate obligations.

At `fd28788e320fcd5a18d28a7b2636ec12fcd228ba`, the full local workspace passed
108 tests. The ACL-clearing negative control cloned that exact source, removed
only the two owned-object clearing calls and compiled successfully. Its inherited-
ACL test then failed the independent OS listing assertion (one remaining entry,
expected zero), demonstrating detection. The injected diff, source identity and
failure log remain under owned `target/acl-negative.zeiiWS`; the faulty checkout
was removed. This is detection evidence, not a passing product run.

[Native credential run38794094552](https://github.com/resoltico/EMLAttachmentRemover/actions/runs/38794094552)
passed on Linux x64/ARM with actual nonroot IDs, cleared supplementary groups,
working own-entry controls, private/self-sticky/trusted-root-sticky protections,
shared nonsticky attack control and foreign-owner refusals, followed by complete
owned cleanup. Both Macs failed before fixture operations because the dropped
child's effective GID was0. [Diagnostic run38794335686](https://github.com/resoltico/EMLAttachmentRemover/actions/runs/38794335686)
confirmed real GID20 but effective GID0 and sole group0, despite UID501. Neither
failed Mac run established filesystem protection.

Pinned Rust clears groups after setgid when uid is requested; Darwin substitutes
group0 for an empty group list. The reviewed correction first requires precisely
the observed nonroot UID/real-GID/sole-group0 state, then performs one safe self-
exec requesting only the permitted real GID. That hop does not request uid or
clear groups again. The final child must pass unchanged strict IDs/groups,
no-root-regain and positive controls before filesystem operations. Corrected
native Mac execution still requires its complete scenario set. No raw credential
code, account or owner-preference change is used.

[Run38795256996](https://github.com/resoltico/EMLAttachmentRemover/actions/runs/38795256996)
at d592bac passed native library Clippy/tests on all four POSIX hosts. Both Linux
credential probes also passed actual named-ACL write grants, pre-mkdir refusal,
zero-mask admission/actor denial and cleanup. Both Macs verified normalized UID,
real/effective GID and groups, root-regain refusal, working own-entry controls and
private/self-sticky/trusted-root-sticky protection. Their shared-parent negative
control then failed: Darwin denied another UID renaming the victim0700 directory.
Thus the overall run remained failed; later foreign-owner cases were not reached.

Darwin24 source requires DELETE and ADD_SUBDIRECTORY on a moved directory even
for same-parent rename, so assuming Linux's0700 rename result was an invalid test
oracle. Its empty-directory removal requests DELETE only. The corrected owned
negative control keeps stage0700, retains its original descriptor, synchronizes
the observation before the actor, and tests actual rmdir plus an actor-owned
replacement. It records the replacement's identity, retains the original handle,
verifies distinct native identity, reclaims/protects the parent, then removes only the known owned
replacement. No product admission is relaxed to obtain an expected test result;
corrected native execution remains required.

At ac11de0, both Linux probes passed the complete matching empty-removal controls
and ACL-mask cases. Both Macs executed denied removals in protected parents and
successful rmdir/replacement in the shared parent, then failed an invalid observer
expectation that the retained original directory report nlink0. A local owned
APFS reproduction established successful rmdir with nlink2 through the open
descriptor. Positive directory link counts therefore never upgrade Unknown
cleanup to known Residual. The observer now correlates the successful native
rmdir receipt, retained original handle identity and the independently observed
distinct actor-owned replacement, recording link counts without assuming APFS
zero behavior. Unknown cleanup still prevents a Created result; it is never
rewritten as successful removal. Logs preserve both the actual native operations
and the failed assertion. That checkpoint preceded the corrected native run below.

The corrected POSIX checkpoint at `45b296b62924f41945c5715ea095bda4f775872e`
passed [run38797832546](https://github.com/resoltico/EMLAttachmentRemover/actions/runs/38797832546):
native filesystem Clippy and25 tests on each Mac CPU,21 on each Linux CPU, zero
failed/ignored/filtered; actual ext-family0xef53 and Mac flags0x4909000 recorded.
All four credential probes passed matching private/self-sticky/trusted-root-sticky
empty-removal denials, working actor controls, successful shared empty-removal/
replacement, foreign-owner refusals and owned cleanup. The retained original link
counts were2→0 on Linux and2→2 on both Macs. Both Linux ACL-mask probes passed.
This closes that checkpoint's POSIX admission/staging subset, not full WP3,
Windows product integration, extracted archives or GUI qualification.

Authorization reference: [Darwin24 rename/rmdir](https://github.com/apple-oss-distributions/xnu/blob/xnu-11417.140.69/bsd/vfs/vfs_subr.c#L8648).

Credential references: [Rust1.99 process setup](https://github.com/rust-lang/rust/blob/1.99.0/library/std/src/sys/process/unix/unix.rs#L302-L330),
[Darwin credentials](https://github.com/apple-oss-distributions/xnu/blob/main/bsd/kern/kern_prot.c).

References: [unlink/unlinkat](https://man7.org/linux/man-pages/man2/unlink.2.html),
[POSIX ACL mask and mode](https://man7.org/linux/man-pages/man5/acl.5.html),
[Apple vnode authorization](https://github.com/apple-oss-distributions/xnu/blob/main/bsd/vfs/vfs_subr.c).

## Selection spelling, links and overlap

The raw request input, output and captured-cwd expressions each have the core's
separate path-unit/component admission limits. Prefixing a valid short relative
POSIX selection with cwd for a derived absolute operand does not create a second
InvalidRequest. Descriptor-relative operations preserve that selection; native
backend operational ceilings still produce truthful operation failures. Walked
path limits apply to the selected spelling plus its discovered suffix, without
charging the captured cwd prefix to a relative expression's component allowance.

The future collector's8MiB path budget charges each source and known planned
destination once by the maximum of its complete display-expression native bytes
and complete reported-native bytes, including failure items. Windows counts two
bytes per UTF-16 unit. The4MiB raw-input budget remains separate and precedes
deduplication. Logical repeated prefixes count even when their allocation is
shared. Derive transient operands and do not hide independently retained copies.
Expanded cwd prefixes count when actually reported, rather than remaining
transient operational material. Exact/one-over
accounting tests remain required with the collector; this clarification does not
claim that collector implemented.

On Windows, every successfully admitted source reports its admitted extended
SELECTED expression plus literal walked suffix as the native value. Preserve
aliases/provenance; do not rebase reporting onto the canonical target. Retain the
original request-derived expression separately for human display. Re-resolving
an ordinary root plus a discovered literal dotted/spaced suffix can select a
different entry, so both actual I/O and reported native values derive from the
admitted extended root. When admission cannot produce a usable operand, a failed
root reports its failed request as unqualified intended input. Successful items
cannot use that exception. The frontend only encodes/renders core-supplied facts.

POSIX selected expressions can be observed or opened relative to an already held
working directory through `observe_selected` and `open_selected`. These use
`fstatat`/`openat`, follow explicit selected links and leave symlink/dot-dot
resolution to the kernel. An absolute expression keeps its ordinary native
meaning. Relative operations remain anchored to the descriptor if the cwd's
requested address moves; the retained absolute spelling still describes the
request and does not falsely verify that address. Walked child operations retain
their separate one-component/no-follow requirements. These primitives perform
no filesystem writes, process cwd changes or whole-run admission decisions.
Child observation has no final-follow flag on either native backend. Explicit
selected-link following stays in selection admission, separate from walking.

Windows selected-directory observation currently supports capture and names-only
enumeration. `BoundDirectory` retains an ordinary disk directory handle and usable
full128-bit identity. Directory access uses `OPEN_EXISTING`, list/read-attributes,
and traverse for held-parent child lookup,
sharing read/write/delete and `FILE_FLAG_BACKUP_SEMANTICS`; code does not enable
privileges. The backup flag can affect checks when the caller already has backup
privileges, so restricted-token tests remain separate evidence. This handle is
not creation protection. Explicit selected directory links follow their target.

An exclusively borrowing iterator restarts the held file object's cursor, queries
fixed aligned64KiB buffers with `FileIdBothDirectoryRestartInfo` then
`FileIdBothDirectoryInfo`, and preserves native UTF-16 names. It ignores record
IDs and other unused metadata. A record's zero next offset ends a buffer, not the
directory; only native `ERROR_NO_MORE_FILES` is EOF. Other native failures and
invalid record extents/names are reported once, then terminate that iterator.
The API supplies no bytes-written count; bounds are checked against each record's
next offset or the fixed allocation for its last record, not a fabricated returned
byte count. Zero initialization is not evidence of a returned record. A fresh
iterator restarts after partial consumption, completion or failure.

Windows child observation/open now uses the narrowly reviewed existing-only
native contract in [the API record](windows-publication.md#existing-only-native-child-open-contract).
Source review and both-CPU native read-only cases passed; exact pinned evidence
is recorded in the implementation log. A reparse leaf is
reported before ordinary signature lookup; a traversal open refuses reparse
processing. The collector must compare its earlier observed full ID before
descending through the returned directory. Physical ancestry/overlap admission
and the core collector remain unimplemented.

The Windows free `observe_selected` function now supplies selected-root metadata
from an already admitted absolute extended DOS/UNC expression. Its fixed
OPEN_EXISTING/READ_ATTRIBUTES/share-all/BACKUP_SEMANTICS call follows the selected
final link and opens no object data. The temporary handle supplies observed
kind/full ID/file signature through the existing entry helper. Non-disk and
failed/unusable observations remain errors; a remaining reparse tag is Reparse,
not a fabricated ordinary target. This is a metadata observation, not held-cwd traversal or
physical admission. Later directory opens must compare the earlier observed ID;
walked children keep the existing no-follow operations. Benign owned tests cover
native names, file/directory metadata, exact missing errors and static selected
directory-alias versus walked-reparse behavior. Source and bounded both-CPU native
checks closed at cbc9947;
no new arbitrary flags, data access, privileges, namespace or API dependency.

Windows `BoundDirectory::resolved_path` now queries the held ordinary directory's
normalized DOS/UNC name with fixed GetFinalPathNameByHandleW flags. One UTF-16
buffer derives from the existing32766-content authority plus NUL; zero retains
the actual native cause and a capacity result is refused without sizing retries
or another namespace. Existing extended-path validation and held ordinary/full-ID
checks bracket success. The name preserves native units but is only an
observation: later ancestor binding must compare its endpoint with the initial
root ID and separately qualify the original requested address. Static selected-
alias tests inspect the RETURNED operand without final-link following and require
the known ordinary target ID; simply reopening an alias with following is not
proof of resolution. A separate owned long/unpaired-name case exercises the
same query. Source/API/test and bounded both-CPU native checks closed at31dd42a. API reference:
[GetFinalPathNameByHandleW](https://learn.microsoft.com/en-us/windows/win32/api/fileapi/nf-fileapi-getfinalpathnamebyhandlew).

Windows `BoundDirectory::name_limit` exposes the observed volume component limit
through its read-only handle before staging. One private handle helper owns the
unchanged GetVolumeInformationByHandleW query for both this getter and the owned
creation parent; the edge still queries freshly before allocating/calling.
Positive DWORDs count WCHARs; native failure/zero remains an error, with no
Windows cache or guessed255 fallback. The existing boundary test now queries the
read-only getter before write capability, compares a direct independent SDK
DWORD, and uses the public staging/rename path for exact/max+1 controls. Source
review/compile checks are closed; native runtime evidence is pending. Metadata
availability supplies no write-admission or confinement authority.

The separate Windows `WritableDirectory`/`CreatedDirectory` primitive now performs
exclusive empty-directory creation from a root-scoped write capability, with
independent child/parent flush receipts and explicit handle-only empty cleanup.
Its [reviewed contract](windows-publication.md#empty-directory-creation-design-for-native-qualification)
passed its bounded source and both-CPU native checks. It writes no message data,
changes no existing destination ACL and does not qualify creation under parent
mutation or publication.

Windows `stage_bytes` now supplies the separate owned-file storage capability:
bounded same-handle write/readback/EOF checks, actual file synchronization and
explicit cleanup, with native inherited ACLs unchanged. Source review is closed;
the bounded both-CPU native storage cases passed at543f8a7. ReadyCandidate now
consumes into a separate physical rename attempt; that increment's source and
bounded both-CPU native checks closed at d58d503. Neither storage nor the
rename-effect receipt qualifies MIME
authorization, source admission, namespace/address synchronization, changed-parent
confinement or final-copy publication.
The names-only structure's64-bit file ID is never padded into a full identity.

Native contracts: [GetFileInformationByHandleEx](https://learn.microsoft.com/en-us/windows/win32/api/winbase/nf-winbase-getfileinformationbyhandleex),
[FILE_ID_BOTH_DIR_INFO](https://learn.microsoft.com/en-us/windows/win32/api/winbase/ns-winbase-file_id_both_dir_info).

An explicitly selected file link keeps the selected entry's basename and parent
for default output. `/links/alias.eml -> /target/original.eml` creates
`/links/alias.no-attachments.eml`; final-link following supplies read identity,
not a new naming parent. A selected directory link uses its admitted label for
mirrored output: O/InboxLink/relative-parent/copy-name. Default beside-source
walks use held directories behind the alias, qualifying the reported alias
address separately. Retargeting cannot redirect a bound output.

Normal admitted terminal directory components supply root labels. Literal
filesystem/volume roots omit them. A selected dot uses the captured working-
directory label; other nameless non-root expressions, including unresolved
symlink/.., use selected-folder. Never join literal dot/dot-dot or lexically
collapse symlink/.. just for naming. Normal collision handling still applies.

Validate request/output-root syntax and existence before binding explicit roots.
Missing/unbindable roots use INPUT_ADMISSION_FAILED without writes. Complete
physical output-overlap admission before writes; read-only inventory may precede
it. OUTPUT_OVERLAP is a streamed run failure: start+summary only, no item events,
known items not_processed, exit1. Collection completeness reflects actual
discovery (false before walking, true if inventory completed). Unexpanded
directories are not fabricated items, so directory-only overlap can have total0.
It is distinct from NO_INPUTS and from invalid missing/non-directory output-root
requests, which remain pre-stream exit2. These rules also apply to dry run.

Admission-only root metadata does not establish logical collection precedence.
Normal retention follows request order and DFS, so [directory D, explicit D/a]
keeps the walked first encounter and its layout. If root admission fails before
walking, materialize known explicit file/failure roots in request order without
folder expansion. Stop/limit during logical collection retains its committed
bounded prefix without backfilling later admission metadata and emits no items.
Cancellation during metadata-only root observations does not convert those
observations into known work items.
Root-admission failure separately retains collection_complete=false and emits
only explicit-root failures, leaving other known work not_processed. Physical
negative-overlap implementation and qualification remain unfinished; neither a
filtered walk nor a plain upward inode no-match establishes disjointness.

The reviewed physical-overlap predicate is ordinary-directory reachability from
each held selected directory in its admitted native traversal context. Selected
root links follow once; encountered symlinks/reparse entries remain excluded.
Ordinary mounted views participate, including when selecting a filesystem root.
A covered underlying storage entry unreachable in that bound namespace is not
an additional hidden-storage graph to reconstruct. Identical directory objects
can have different mounted views, so no global inode visited set may skip one.
These are native observations, not an atomic snapshot or permanent topology proof.

For explicit-output qualification, the reviewed design traverses every ordinary
branch, including hidden/generated names, while retaining file work only in
eligible branches and reading no message bodies. Only a positive full-ID match
is OUTPUT_OVERLAP. A negative result requires the complete unfiltered traversal;
unknown/inaccessible branches prevent whole-run writes with their actual
BIND_FAILED/admission or READ_FAILED cause. They are not fabricated overlap or
explicit-root admission failures. Share the existing depth/path/100000-entry/
stop bounds across all branches and count entries before filtering; limit/stop
retains its no-item outcome. Beside-source keeps filtered/per-item behavior.
Qualification status, logical discovery completeness and known-work prefix are
separate facts. Recheck original requested addresses before writable integration.
This design does not claim the collector or its native qualification exists yet.
