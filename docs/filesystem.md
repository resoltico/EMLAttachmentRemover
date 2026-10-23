# Filesystem contracts

The native filesystem workflow implements the contracts below. Source descriptions
are not execution evidence; the delivered source requires the native checks in
[verification](verification.md).

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
the current entry. This does not establish
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
returns native facts without assigning a later publication phase. The run
workflow performs the required source checks before/after its edge.
This capability creates no output, syncs no file, applies no run metadata budget,
emits no events and supplies no snapshot or permanent-stability guarantee.

## Deterministic generated destinations

A file is `<human-stem>.<route-token>.no-attachments.eml`; generated mirrored
folders are `<human-label>.eml-dir`. The route token is the first 16 bytes of
SHA-256 rendered as 26 lowercase RFC 4648 base32 characters without padding.
It distinguishes complete raw logical routes, including folders whose native
case/normalization/truncation spellings converge on a destination. It is not a
content checksum or proof of identity. Native no-replace publication and exact
byte/binding qualification remain authoritative. There is no old-name lookup,
numeric collision search, dual writing, migration or automatic rewrite.

The seed begins with `eml-attachment-remover:destination-route:v1` and NUL, then a
little-endian u32 component count. Each component has a mirror/file kind byte
(1/2), encoding byte, little-endian u32 payload byte length and exact payload.
Valid Unicode is UTF-8 (encoding 0); invalid Unix bytes use encoding 1; unpaired
Windows UTF-16 units use encoding 2 with little-endian code units. Raw mirror
labels omit `.eml-dir`; the unshortened final component is the human source stem
plus `.no-attachments.eml`. Output-root addresses, file identity/content,
argument positions, component limits and existing-prefix length are excluded.

Only human prefixes shorten, at Unicode-scalar/native-byte or UTF-16-pair
boundaries. Files retain all 46 fixed ASCII units and at least one complete human
character; mirrors retain eight fixed units and a human character. Every actual
parent rechecks its frozen component budget before the edge. A changed budget
refuses the item rather than changing a checked name. Rendered paths count against
the native and 8 MiB report bounds.

New names require a supported local name-domain profile: Linux ext2/3/4, tmpfs,
XFS or Btrfs; local macOS APFS or HFS+; local Windows NTFS. Filesystem naming modes
must preserve unequal lowercase ASCII base32 terminal tokens and the distinct
`.eml-dir`/`.no-attachments.eml` domains. Production does not emulate Unicode
collation, derive a child case mode, or generate short aliases. Existing entries
are always resolved natively. Windows queries generated SDK device information
and requires exact completed success, exactly sized information, disk device type and
no remote-device characteristic. Unexpected pending retains its exclusive query
handle and buffers until actual settlement, then refuses creation; failed waits
and elapsed time do not release outstanding kernel storage or qualify a profile.
The native Windows pending-retention fixture uses an owned kernel event, an
actual failed wait and a completing thread through the same retention loop. It
checks the live handle and stable heap owners until event observation and thread
join; its controlled locality data does not claim that a synchronous volume
query actually returned pending or that a remote filesystem was mounted.
Actual APFS/NTFS case-mode/8.3/native-CPU qualification is separate evidence.

Full seed equality is checked whenever tokens match. Different seeds sharing a
token fail the entire group with a route-token collision reason. Rows execute in
raw logical-route order, then absolute scope/source and selected-origin order;
rows without a logical route follow, with stable semantic/native diagnostic tie
breaks. Report indices are contiguous in that frozen order. A Check is read-only
and confers no authority on a later Create, which binds everything afresh.

## POSIX namespace admission before creating an entry

A pathname identity check followed by rmdir/unlinkat is not an atomic
compare-and-delete operation. A held private staging descriptor protects its
contents, not its outer name.

Namespace creation is refused unless the held immediate parent
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
Read-only planning refusal uses DESTINATION_UNSUPPORTED/admission; an actual
creation-edge refusal uses DESTINATION_UNSUPPORTED/stage and actionable private-folder guidance.

Required cross-UID, ACL/mask, mirrored-edge, substitution, residue, synchronization
and native-platform checks remain obligations. Same-UID interleavings or source
inspection do not establish cross-UID execution or complete product acceptance.

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

Source reads and existing-only comparison/synchronization do not require namespace
creation admission. Existing synchronization still requires actual file and endpoint
directory flush access and successful native calls; it never repairs missing layout.
Known current-run namespace debt on that exact retained route also requires flushing
the affected child and its actual mutation parent before success. Local APFS currently rejects invalid UTF-8 names with EILSEQ, so those
tests record that volume refusal separately from representable-name checks;
Linux must execute the invalid-byte round trip. Unit cases for model constants
or ownership flags do not establish a real mount's behavior.

Nonshipping `native-fixtures` builds expose one finite `StoragePhase` callback
through `OperationObservation` and the same staging engine. Processing preflight
is the per-item destination execution check after source preparation/planning;
readback is the actual held-existing-file comparison. Stage-write/file-sync hooks
pause before the existing write/flush checkpoints, publication pauses before its
existing final guards, and namespace/after-publication hooks retain the actual
post-edge path. They add no cancellation guard, replacement event, errno or sync
receipt. Check invokes no callback. A fixture pause before a syscall is not
claimed as a blocked kernel syscall; its nonshipping artifact and actual process
lifetime remain separate from full shipping-archive qualification.

The held inode's observed name budget cannot ordinarily be changed by a native
fixture. The budget predicate has an independent mismatch control. A separate
native pre-edge control replaces the requested owned parent with a different,
creation-incompatible parent after candidate staging; the actual address guard
must refuse before publication, preserve both namespaces and clean only the
retained owned candidate. This proves actual changed-parent refusal, without
claiming a fabricated mutable `fpathconf` result or renaming a frozen token.

The Linux native profile campaign also runs independently preseeded existing-copy
fixtures on an owned ordinary ext4 image and an OverlayFS mount whose lower,
upper and work directories belong to another owned ext4 image. Root and mirror
mode0777 deliberately refuse creation policy. A separate owner0700 empty
OverlayFS destination must refuse both Check and Create before any witness,
mirror, candidate, permission change or flush; it cannot borrow the0777 refusal
as proof that the profile is unsupported. Matching, unequal, missing-route and
non-directory replacement cases retain exact before/after namespace, native
identity, permissions, hashes and CLI receipts. Ordinary ext4 requires actual
file/endpoint synchronization success. OverlayFS can report an unsupported flush
only when its native errno agrees with an independent flush of the same retained
file/endpoint; other failures leave the profile unqualified. No stage/layout
repair or permission changes are accepted. Missing/replaced cases are controlled
admission-time states, not evidence of an in-flight race. The driver detaches the
owned overlay before its backing image, retaining the backing mount if overlay
detach fails; it never changes an existing mount or forces detach.

Model references: [Linux VFS deletion](https://github.com/torvalds/linux/blob/v6.8/fs/namei.c#L2931-L3004),
[POSIX ACL permissions](https://github.com/torvalds/linux/blob/v6.8/fs/posix_acl.c#L375-L443),
[tmpfs](https://docs.kernel.org/filesystems/tmpfs.html),
[filesystem ownership mappings](https://www.kernel.org/doc/html/latest/filesystems/idmappings.html).

## Selection spelling, links and overlap

The raw request input, output and captured-cwd expressions each have the core's
separate path-unit/component admission limits. Prefixing a valid short relative
POSIX selection with cwd for a derived absolute operand does not create a second
InvalidRequest. Descriptor-relative operations preserve that selection; native
backend operational ceilings still produce truthful operation failures. Walked
path limits apply to the selected spelling plus its discovered suffix, without
charging the captured cwd prefix to a relative expression's component allowance.

The collector's8MiB path budget charges each source and known planned
destination once by the maximum of its complete display-expression native bytes
and complete reported-native bytes, including failure items. Windows counts two
bytes per UTF-16 unit. The4MiB raw-input budget remains separate and precedes
deduplication. Logical repeated prefixes count even when their allocation is
shared. Derive transient operands and do not hide independently retained copies.
Expanded cwd prefixes count when actually reported, rather than remaining
transient operational material. Exact/one-over
source-accounting tests exercise the read-only collector, including max of
independent display/native lengths. Planned-destination and maximal event/run
admission have separate checks; these source tests alone do not establish them.

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

Windows child observation/open uses the narrowly reviewed existing-only
native contract in [the API record](windows-publication.md#existing-only-native-child-open-contract).
A reparse leaf is
reported before ordinary signature lookup; a traversal open refuses reparse
processing. The collector must compare its earlier observed full ID before
descending through the returned directory. Physical ancestry/overlap admission
remain separate from full writable integration. The read-only core collector
below consumes the inventory primitives; native qualification has separate gates.

The Windows free `observe_selected` function supplies selected-root metadata
from an already admitted absolute extended DOS/UNC expression. Its fixed
OPEN_EXISTING/READ_ATTRIBUTES/share-all/BACKUP_SEMANTICS call follows the selected
final link and opens no object data. The temporary handle supplies observed
kind/full ID/file signature through the existing entry helper. Non-disk and
failed/unusable observations remain errors; a remaining reparse tag is Reparse,
not a fabricated ordinary target. This is a metadata observation, not held-cwd traversal or
physical admission. Later directory opens must compare the earlier observed ID;
walked children keep the existing no-follow operations. Benign owned tests cover
native names, file/directory metadata, exact missing errors and static selected
directory-alias versus walked-reparse behavior. The observation uses no data
access or additional privileges and cannot create namespace entries.

Windows `BoundDirectory::resolved_path` queries the held ordinary directory's
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
same query. API reference:
[GetFinalPathNameByHandleW](https://learn.microsoft.com/en-us/windows/win32/api/fileapi/nf-fileapi-getfinalpathnamebyhandlew).

Windows `BoundDirectory::name_limit` exposes the observed volume component limit
through its read-only handle before staging. One private handle helper owns the
unchanged GetVolumeInformationByHandleW query for both this getter and the owned
creation parent; the edge still queries freshly before allocating/calling.
Positive DWORDs count WCHARs; native failure/zero remains an error, with no
Windows cache or guessed255 fallback. The existing boundary test queries the
read-only getter before write capability, compares a direct independent SDK
DWORD, and uses the public staging/rename path for exact/max+1 controls. Metadata
availability supplies no write-admission or confinement authority.

The Windows `CreationDirectory`/`DirectoryCreation` boundary uses the
[detect-before-bytes contract](windows-publication.md). A zero-byte owned file
first proves containment and keeps its parent nonempty while CreateDirectoryW
and the later no-follow mirror open/binding occur. The guard is checked-cleaned
only after the mirror is ordinary, contained and pinned without delete sharing.
The mirror binding grants no ownership or deletion authority. Required child and
parent namespace synchronization remains independent of that ownership limit.

Windows `stage_bytes` supplies the separate owned-file storage capability:
bounded same-handle write/readback/EOF checks, actual file synchronization and
explicit cleanup, with native inherited ACLs unchanged. ReadyCandidate consumes
into a separate physical rename attempt. Neither storage nor the rename-effect
receipt qualifies MIME
authorization, source admission, namespace/address synchronization, changed-parent
confinement or final-copy publication.
The names-only structure's64-bit file ID is never padded into a full identity.

Native contracts: [GetFileInformationByHandleEx](https://learn.microsoft.com/en-us/windows/win32/api/winbase/nf-winbase-getfileinformationbyhandleex),
[FILE_ID_BOTH_DIR_INFO](https://learn.microsoft.com/en-us/windows/win32/api/winbase/ns-winbase-file_id_both_dir_info).

An explicitly selected file link keeps the selected entry's basename and parent
for default output. `/links/alias.eml -> /target/original.eml` creates
`/links/alias.<route-token>.no-attachments.eml`; final-link following supplies read identity,
not a new naming parent. A selected directory link uses its admitted label for
mirrored output: O/InboxLink.eml-dir/relative-parent.eml-dir/copy-name. Default beside-source
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

Entry ownership uses observed parent identity, exact native basename and the same
full file signature. Equal file identity alone does not merge hard-link names.
A signature disagreement for one parent/basename is SOURCE_CHANGED. Explicit file
selection owns a proven same entry; otherwise fewer traversal edges from the
selected root to the source parent wins, followed by raw native source/root/suffix
ordering. Every selected directory view is walked; root inode equality does not
suppress a mounted view. All observed source identities stay reserved.
If root admission fails before
walking, materialize known explicit file/failure roots in request order without
folder expansion. Stop/limit during logical collection retains its committed
bounded prefix without backfilling later admission metadata and emits no items.
Cancellation during metadata-only root observations does not convert those
observations into known work items.
Root-admission failure separately retains collection_complete=false and emits
only explicit-root failures, leaving other known work not_processed. Physical
negative-overlap native qualification and writable integration have separate checks; neither a
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
The read-only collector below implements this design; its native qualification
and all writable integration remain separate requirements.

## Implemented read-only core inventory

`collect_sources(&RunRequest, &RunControl)` borrows the immutable request and
returns an outcome with private captured filesystem handles and authoritative
state. `Ok` is not complete discovery or permission to publish. It validates raw
paths, captures the supplied working context and binds an existing output root,
then observes explicit roots before logical request-order/native-name DFS.
Source-directory links follow at selection; children use no-follow observations
and compare the earlier full identity before descent. Windows source-directory
name resolution checks the reopened normalized endpoint against the held target;
writable ancestry/address qualification is a separate native gate.

The pure error is `RequestPathError`, used by `validate_path_limits` and the
transport decoder. Outer `InvalidRequest` preserves genuine working/output-root
I/O causes through Error::source. Source-root failures after raw validation stay
returned INPUT_ADMISSION_FAILED, including native InvalidInput failures. No
classification depends on native error prose or ErrorKind alone.

Metadata-only failure materialization leaves folders unexpanded. Retained rows
preserve explicit-root versus descendant provenance, native path, flat suffix,
file signature and physical parent. Fingerprint buckets only select candidates;
exact native absolute spelling comparisons constrain deduplication, with stop
checkpoints during collisions. Held-relative and live-absolute routes are
different anchors: bind/walk cross-anchor directory roots separately even when
directory IDs agree, since mounted views can differ. Cross-anchor file work
coalesces only after complete leaf signature and physical parent also agree;
unknown/failure observations cannot prove equivalence. Distinct native spellings
and hardlinks are not merged. Mandatory explicit-root failure diagnostics bypass
work dedupe lookup/insertion, preserving earlier work and the later failed binding.
Intended failed inputs without operational keys never gain keys by prefixing cwd.
Optional equality unavailability uses independent traversal, not a fabricated
root admission failure. Native reported paths are retained/charged; raw root/cwd context is
shared, without an expanded cwd copy for every metadata-only directory root.

Source charge is max of complete display-expression and reported-native bytes,
including failures. The separate4096-work,100000-examined-entry,64-depth and raw
path bounds stop without processing a truncated subset. Structural dot entries
count but never recurse. Explicit output traverses hidden/generated ordinary
branches for separation while file work remains filtered; unknown branches block
the whole run with their actual admission cause. Beside-source keeps descendant
per-item failures. No message bodies, output creation, flushes, global cwd changes,
signals or frontend policy occur here.

Local real-file tests cover deterministic ownership, root gaps and exact causes,
hidden/generated overlap, native-unit ordering, distinct aliases, real4096/4097
work and64/65-depth boundaries, and actual100000/100001 entry totals. Private
hash injection exercises true collisions and deterministic RunControl/owned-empty-
directory-removal boundaries while the filesystem remains real. It does not
qualify another-user interference, mount setup or a signal adapter. Windows
literal-parent/source-native and static junction cases require native execution.
Writable execution is implemented separately.

Complete Windows literal operands use the existing extended validation branch
after append; ordinary normalization is not repeated on literal names. The one
backend ceiling/subset authority stays in eml-fs. A qualification failure after
finished discovery preserves that completion fact. A later stop governs
interruption precedence while retaining already observed aggregate errors and
completed discovery; mid-collection stop still has an incomplete known prefix.
Scripted state tests prove this mapping, not native late signals or OS failures.

Read-only destination planning consumes the inventory and retains one structural
record per known source position. It freezes copy names and mirrored layout,
checks observed parent identities, and reserves full logical routes and tokens
within the actual output-root/beside-source scope. Known physical destination
aliases also form complete conflict groups. Every contender fails before any
mirror/stage/flush/publication, including a contender with another leaf error.
Source identities and selected entries remain reserved. An incompatible or
unverified mirror parent produces no address capability; its raw logical route
still participates in known collision grouping. Only positively missing tails
may borrow the closest observed parent's provisional component budget.
Completed planning is not MIME validation,
existing-byte comparison, write admission or a terminal PlannedCopy outcome.

Optional child observation treats only native ENOENT (Unix) or
STATUS_OBJECT_NAME_NOT_FOUND (Windows) as absence. Other failures retain their
native cause, and dangling links remain present. Absence is an observation at a
held parent, not a promise about a later publication address. These helpers add
no write rights, flags, fallback lookup or directory creation.

Existing-copy comparison accepts a privately prepared source and an already
opened no-follow ordinary destination. It checks both bindings, refuses the held
source's identity, compares length first and reads at most the verified candidate
length plus one byte in64KiB buffers. Both bindings are revalidated after a byte
match or mismatch. A successful value retains the same existing handle; equality
and current stability do not establish UpToDate, synchronization or a verified
requested address. Failure retains the actual opened identity, any completed byte
observation and existing stability separately from the source/binding/data error.
The final workflow must enforce no-follow destination opening and reject every
selected-source and prior-created identity; this narrow operation sees only the
currently prepared source. It performs no flush or write.

The explicit core dry_run capability performs serial preparation and existing
comparison through typed start/item/summary events. It requires RunMode::DryRun
before I/O or events; Create is not silently converted. It rebinds destination
parents in the captured context, compares every frozen observed prefix identity,
and refuses an invalidated naming assumption without calling that proof of an
address change. POSIX relative operations remain anchored to the captured
working-directory descriptor if its address moves. A source parent's cooperative
stop is distinct from native failure and maps to the execution phase without
parsing text or replacing genuine I/O errors with cancellation.

The workflow admits complete removal metadata against20,000 entries before
reporting it. Metadata on a reported failure remains charged; an over-budget item
fails without retaining its removal list. Typed item metadata borrows only the
current prepared source, and RunOutcome keeps counts and run/delivery facts rather
than a message list. Each terminal result is counted before checked delivery.
Failed delivery stops later work/emission and does not request user cancellation.
A successful terminal summary fixes the outcome; a later Stop cannot rewrite its
exit class. A failed summary can retain delivery failure and observed user-stop
precedence because no complete successful stream is claimed.

This operation never creates missing parents, stages, publishes or synchronizes.
Writable consumers and private success constructors require separate native
qualification; a read-only operation alone establishes no product acceptance.

The common run operation implements both Create and DryRun in source. It
shares the same admission, preparation, initial read-only destination inspection,
metadata budget, serial accounting and checked events. Different existing bytes
fail before directory or file synchronization access is requested. Matching
inspection retains its actual parent prefix and held file; normal Windows
execution checked-closes that comparison owner, opens the fixed sync profile,
requires the same parent/full file binding, repeats reservations and the same
bounded comparison, then performs actual file/namespace/address checks. Private
success constructors reject incomplete receipts; publication evidence can still
be retained in a failed result.

Current-item PublicationBinding retains the held root and existing/new mirror
owners. Separate ExistingSyncBinding retains only a complete HeldRoute and an
endpoint SyncDirectory, plus indices of known current-run debt edges on that route.
It has no mirror/stage creation or layout-cleanup API.
BoundDirectory is observation/traversal; CreationDirectory acquires only the
actual immediate mutation parent. Windows route ancestors and existing mirrors
use metadata/traverse pins. The endpoint obtains GENERIC_WRITE flush access; a
known debt edge additionally acquires only synchronization access to its child and
actual mutation parent. Actual creation/deletion rechecks its native parent requirements. It never rebaselines a component bound at planning or first inspection.
Still-unbound tails allow one exclusive creation attempt, or one exact native
collision reobserve/open of an ordinary other-creator directory. Ownership and
synchronization debt are separate. Windows conservatively flushes child and
parent for possibly-new unowned edges; POSIX preserves its run-created-only rule.
A per-run bounded pending set retains known parent/child pairs and unresolved
parent/native-component attempts after failed creation/sync. Native lookup can
resolve an attempted address without granting ownership; missing/error lookup
cannot erase it. Only both successful edge operations or confirmed owned removal
retire the matching debt. Existing-only success also pays known debt on its route:
a failed earlier creation/sync can leave a nonempty mirror after another actor adds
a matching output, so endpoint-only synchronization would otherwise forget the
unsynchronized parent edge. Debt is retired only after both successful edge calls;
failure retains partial receipts and debt. This requires no namespace creation
admission and grants no deletion ownership. Upper pins not modified by this
workflow are not flushed.

Cleanup is candidate-first. POSIX then removes provably owned empty layout in
reverse order, only before publication. Windows releases unowned mirror bindings
with Unknown cleanup, no guessed residual name and no deletion. Such a mirror
may have moved, been populated or replaced during the Boolean create/open gap. A known stage residual component and first native
cleanup error survive aggregation. Multiple verified layout residues may be
Residual with no guessed name; genuinely unverified cleanup is Unknown. No
ancestor basename is placed into the fixed candidate-residual slot. Published or
Unknown retains intended layout and never rolls back a final copy. Actual held-
parent candidate identity can reconcile an ambiguous edge independently of later
source/content/address qualification. Cleanup/checked-close failure is retained
before later address failures. Staging interruption is classified from the exact
checkpoint that stopped, not a contemporaneous flag or native ErrorKind.

## Windows containment

Windows publication checks candidate containment before any message byte is
written. Pins deny delete sharing and allow read/write sharing; attribute-only
access is not excluded. A held candidate keeps the directory nonempty, preventing
reparse conversion under the supported filesystem contract. Creation, final
identity/readback/address, synchronization and cleanup remain independent facts.
Copies inherit destination ACLs; other writers can change completed entries.
See [Windows publication](windows-publication.md) for the complete boundary and
required native controls.
