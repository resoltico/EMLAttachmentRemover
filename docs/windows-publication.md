# Windows same-directory publication correction

This explicitly supersedes only the Windows rename mechanism in the supplied
v6 design pack's filesystem contract §6 step 5 and the matching Win32-only
restriction. The original design archive and checksum are unchanged.

## Design and challenge

Both native Rust 1.99 GNU LLVM Windows hosts rejected the specified
SetFileInformationByHandle/FileRenameInfo relative-root call with error 87.
An absolute-name variant failed with error 32 while the destination's write
sharing remained excluded. Allowing write sharing enabled rename but also a
concurrent directory write handle, removing the required in-place reparse
protection. That diagnostic did not demonstrate a successful junction redirect.
The unsafe sharing change is excluded from the product.

The independent “Audit Rust rebuild proposal” chat reviewed the generated
structure layout, aligned buffers, access rights, lengths and actual native
runs. It recommended the narrower native same-directory operation. At
[e79277b](https://github.com/resoltico/EMLAttachmentRemover/actions/runs/38759803940),
both actual NTFS hosts passed its protected primitive, conflict, identity,
readback, file/namespace flush and owned cleanup checks. This proves that
focused boundary; it does not qualify the full product or all adversarial cases.

## Reviewed rename boundary; creation remains unresolved

- The proposed Win32 pathname-creation route is not qualified. Native evidence
  below disproves the claim that its directory sharing restrictions alone keep
  the requested creation address bound. Product creation is on hold.
- The reviewed rename operation requires a complete verified candidate held by
  its exclusive synchronous DELETE/SYNCHRONIZE-capable handle. This requirement
  does not establish a safe candidate-creation mechanism.
- Publish through generated windows-sys NtSetInformationFile and native
  FILE_RENAME_INFORMATION with FileRenameInformation (10), replacement false,
  RootDirectory null, and only a single final basename. Native same-directory
  semantics use the held candidate's directory; there is no pathname target
  reopen or temporary sharing relaxation.
- Reject empty, dot, dot-dot, NUL, separator and colon names and enforce the
  observed filesystem component-unit limit. Check size arithmetic, use native
  alignment and a conservative sizeof(struct)+name bytes+2 allocation.
- Retain buffers and handles through any pending completion. Immediate negative
  NTSTATUS is authoritative even when IO_STATUS_BLOCK was not populated. Keep
  raw status evidence; map through RtlNtStatusToDosError, never stale LastError.
- Preserve final identity/address, namespace synchronization, close/reopen,
  no-overwrite and identity-safe cleanup requirements and all typed outcomes.
- Only this rename operation has been added to the reviewed native boundary.
  No alternative creation mechanism is implemented or qualified. Handwritten
  native layouts, absolute-name rename, MoveFileEx, replacement and relaxed
  sharing remain excluded.

The restricted-token probe uses the runner account's thread token with the
Administrators SID deny-only and DISABLE_MAX_PRIVILEGE. Windows retains
SeChangeNotifyPrivilege. This is actual restricted access, not a separately
qualified standard-user interactive login. Each adversarial actor thread must
establish its own intended token; impersonation is thread-local.

Full WP3 still requires actual junction/reparse mutation and pre-existing writer
controls, source/destination/candidate substitutions, entry conflicts, post-edge
failures and cleanup cases on both native CPUs. The early gate is not a waiver.

## Native creation-protection gate failed

The generic-write sharing refusal does not establish attribute-only exclusion.
The documented filesystem control accepts FILE_WRITE_ATTRIBUTES as an alternative
to FILE_WRITE_DATA, and CreateFile sharing rules do not classify attribute/EA
access as data-write sharing. Product pathname-based creation is on hold; repeated
identity checks do not repair this missing guarantee.

The test uses the same restricted runner-account actor on two empty owned
directories. It first proves an attribute-only handle with fully compatible
sharing can set the real mount-point tag, retain the same ID and read a unique
owned target sentinel. It reuses that already prepared native buffer against the
strict destination pin, observes tag/held/current-entry identities and cleans
owned fixtures before adjudicating. Only actual native access/sharing denial can
count as exclusion; setup, unsupported, resource or other errors fail qualification.
At exact commit `81332eb24ef9b593f71a838daa7784c7b90f5ccf`,
[both native Windows CPUs failed this gate](https://github.com/resoltico/EMLAttachmentRemover/actions/runs/38804671370).
Each job executed one test: zero passed, one failed, zero ignored and nine filtered.
The observed address changed while held/current entry identities remained equal;
owned fixture cleanup completed before the safety assertion failed. The result
disproves the proposed sharing-only protection. Later naming and library test
steps did not execute and are not qualified by this run.

A replacement must preserve bound-destination creation and complete verified
regular-file publication. No replacement design or native implementation is
qualified here. Candidate metadata guarantees also remain unresolved. Keep this
gate failed while independently implementable business and transport work
continues; earlier rename-only success cannot substitute for creation evidence.

The verification token retains the runner UserSID, observes Administrators as
deny-only and only SeChangeNotify enabled, using the actual effective thread
token. It refuses preexisting impersonation unchanged and restores the verified
prior no-impersonation state on every entered exit. The guard cannot move across
threads. This is restricted-owner evidence, not a separate-SID interactive login.

## Proposed creation-boundary revision (unqualified)

This is a design for independent challenge, not an implemented or supported
Windows publication route. The failed gate above remains required until a
replacement proves the same product properties on both native Windows CPUs.
The extracted design pack remains unchanged. Its filesystem contract §6 final
paragraph calls for a challenged revision when native evidence disproves its
Win32 route; this proposal retains the owner's fixed behavior and release scope.

The proposal replaces only §6's post-root pathname creation in steps 1–3 and
the associated claim that directory sharing excludes every in-place reparse
change. Resolve an explicitly linked output root once, preserve its requested
address and bound identity, reject subsequent reparse components, and retain
only the current item's bounded ancestor handles. Open/create each later child
with a single validated native component relative to the *held parent handle*.
Use create-only disposition for a new mirrored directory or random candidate,
and open-only disposition for an existing directory; never infer ownership from
an open-or-create result. Validate the returned object type, reparse state,
volume/file identity and actual create disposition. A competing entry is
accepted only under the existing ordinary-directory rules. No absolute
`CreateDirectoryW` or pathname `CREATE_NEW` fallback may follow a failed
relative operation. The candidate remains a held, exclusive object; its bounded
name, complete bytes, independent verification and synchronization precede the
already reviewed no-replace rename. Cleanup remains handle/identity based.

The focused API candidate is generated `windows-sys` 0.61.2 `NtCreateFile`
with `OBJECT_ATTRIBUTES.RootDirectory`, a single-component `UNICODE_STRING`,
create/open dispositions and explicit file/directory and reparse options.
[Microsoft documents relative names and these dispositions](https://learn.microsoft.com/en-us/windows/win32/api/winternl/nf-winternl-ntcreatefile),
and [documents `OBJ_DONT_REPARSE`](https://learn.microsoft.com/en-us/windows/win32/api/ntdef/ns-ntdef-_object_attributes).
Generated bindings for the function and structures exist in the pinned crate;
that is source feasibility, not a successful GNU LLVM link or native behavior.
The user-mode documentation describes `RootDirectory` through an earlier
`NtCreateFile`, but a second root capture is not required merely by that wording.
The reviewed read-only interoperability basis below uses the already held
ordinary Win32 directory handle. Challenge the creation route's exact
access, sharing, completion,
buffer-lifetime and ownership rules before adding product FFI. Any accepted
revision must record explicit supersession of the extracted contract's §6
`NtCreateFile` exclusion, WP3's Win32-only wording and §10's reviewed unsafe
scope in this implementation addendum. The existing native
`NtSetInformationFile` rename correction remains in force.

Unchanged requirements are source/existing-destination nonmutation, bound
destination confinement, complete verified regular candidate, atomic
no-replace publication of that held candidate, final destination-inherited
Windows ACL, file/namespace synchronization, requested-address and identity
checks, and truthful published/unknown/cleanup outcomes. Relative creation
alone does not prove the candidate's metadata remains ordinary while its
temporary access follows an inheritable destination ACL. That property needs
a separate challenged protection mechanism or native proof; do not infer it
from exclusive data sharing.

Qualification first needs bounded synthetic tests on both native Windows CPUs
showing that each relative mirror-directory and candidate-file creation under
a changed held parent either stays in the originally bound directory or fails,
with no entry or bytes at the redirected target. Prove actual create-only
ownership, conflict/no-replace behavior, candidate and final identities,
requested-address truth, synchronization and owned cleanup. Then run the full
filesystem contract §10 cases and release-runtime checks. Keep the old-method
failure as explicit evidence and a negative control; only an implemented
replacement with an equivalent-or-stronger passing native gate can change the
product qualification status. A compile-only result cannot do so.

## Candidate ACL decision for review (no policy change)

The current requirement remains: filesystem contract §1 requires a complete,
verified regular candidate, publication of that same held object, and Windows
copies with the destination directory's inherited ACL. Section 6 creates the
candidate directly in that directory with its inherited ACL; §7 requires
truthful post-edge outcomes, §8 allows forced termination after publication,
and §10 requires shared-directory, inherited-ACL and forced-stop evidence.
Protocol 1 has fixed publication receipts and error codes, with no ACL-state
receipt. This section proposes no exception to those requirements.

The candidate question is whether another authority granted
`FILE_WRITE_ATTRIBUTES` by an inherited ACL could make the staged file a
reparse object between its last ordinary-file check and the rename edge.
[Microsoft's file-system protocol](https://learn.microsoft.com/en-us/openspecs/windows_protocols/ms-fsa/4aeefef8-92c3-4abc-af7a-a610caf8a165)
permits reparse-setting access through `FILE_WRITE_ATTRIBUTES` or
`FILE_WRITE_DATA` and describes a data-file reparse operation, with a specific
nonempty-stream refusal for the symlink tag. A reparse file can change later
open behavior. The failed native **directory** test establishes that sharing
is not a universal barrier to attribute-only opens; it does not prove this
candidate-file race on the supported NTFS hosts. Nor does attribute-only access
by itself establish changed message bytes. Candidate integrity remains an
open technical question requiring a separate proof or refutation.

Two policy paths need different evidence:

1. **Keep the strict inherited-ACL requirement.** Qualify the existing
   inherited-ACL candidate policy, or find a mechanism that protects its
   ordinary-file metadata through the no-replace edge while retaining the
   final inherited ACL at that edge and after a forced stop. The currently
   reviewed APIs do not yet prove either. A last-moment metadata check alone
   does not close a possible gap, and the old directory result cannot stand in
   for candidate evidence. Keep the Windows gate failed until both address
   confinement and candidate integrity are qualified on each native CPU.
2. **Possible owner-level contract change, not accepted:** create the candidate
   with a temporary restrictive security descriptor, publish the held complete
   object, then apply and verify an ACL derived from the actual bound parent
   through the held file handle. [NtCreateFile allows a creation security
   descriptor](https://learn.microsoft.com/en-us/windows/win32/api/winternl/nf-winternl-ntcreatefile),
   [CreatePrivateObjectSecurityEx derives inherited ACEs](https://learn.microsoft.com/en-us/windows/win32/api/securitybaseapi/nf-securitybaseapi-createprivateobjectsecurityex),
   and [SetSecurityInfo changes security through a handle](https://learn.microsoft.com/en-us/windows/win32/api/aclapi/nf-aclapi-setsecurityinfo).
   [Rename does not assign a new default ACL](https://learn.microsoft.com/en-us/windows/win32/fileio/file-security-and-access-rights).
   A successful `Created` could require final ACL verification. If the process
   stops or ACL application fails after the edge, however, a complete final
   copy may remain with the temporary restrictive ACL. That is a changed
   permission guarantee, not merely ordinary failure reporting. A reported
   failure would use the existing `Failed(Published(...))` effect, but that
   representation does not itself describe the copy's ACL; a forced stop can
   leave no item event. Protocol 1 has no structured ACL observation. Concrete
   protocol review must decide whether existing code/detail is sufficient or
   an added field is necessary; synchronization and cleanup receipts must not
   be repurposed as ACL evidence. Reruns cannot silently rewrite an existing
   copy's ACL. If this path later proves necessary, the owner-level decision is
   the changed permission/crash guarantee and recovery behavior, not a choice
   of wire fields. Its contract must state the ACL invariant before, at and
   after the edge, the forced-stop residual, `Created` conditions, observable
   failed-published state and recovery action before implementation.

No reviewed API establishes an atomic ACL transition with the existing
no-replace rename, but this is not a claim that no Windows solution exists.
First resolve the exact candidate risk under the existing policy and challenge
the strict path. Do not use the possible policy change to bypass that work or
turn the current failed Windows gate green.

References: [reparse control access](https://learn.microsoft.com/en-us/openspecs/windows_protocols/ms-fsa/4aeefef8-92c3-4abc-af7a-a610caf8a165),
[CreateFile sharing categories](https://learn.microsoft.com/en-us/windows/win32/api/fileapi/nf-fileapi-createfilew).

## Read, destination and existing-copy handle lifetimes

The reviewed product lifecycle releases inventory directory bindings before
writes, retaining their identities and reservations. The current source keeps
its read-only FILE_SHARE_READ file handle and change signature; it does not
retain a conflicting readonly directory pin when the destination needs write
access to that same directory. The writable destination uses the strict pinned
chain above. Win32 sharing checks are symmetric, so reopening another strict
readonly handle is not a harmless way to observe an already write-pinned parent.

Source revalidation checks both the current requested parent expression against
its stored parent identity and the source file/signature. The same file ID under
another parent through a retargeted junction/hardlink is not an unchanged binding.
An observation-only parent handle may advertise write sharing to coexist with
the existing strict pin; it requests no write access and never supplies creation
protection. The strict pin continues denying incompatible writers. Reusing a
destination pin still requires checking that the requested expression reaches it.

Existing-copy resynchronization consumes/closes the read comparison handle,
then opens the existing file with read/write access and FILE_SHARE_READ only.
This is a fresh binding after a gap, not uninterrupted exclusion. Require the
same full volume/file identity and change signature, reject reparse/special
objects, repeat all collected-source and same-run-created-output alias guards,
recompare all candidate bytes plus EOF and before/after stability, flush, then
qualify parent/requested address while the new handle and destination chain live.
Do not close the selected source's handle to make this open succeed. No attribute,
ACL or data changes are allowed. Access, sharing, replacement, alias, stability
or address failures retain an unsuccessful existing observation without new
publication. Dry run requests neither write access nor flush.

Both native CPUs must test beside-source creation without self-sharing failure,
the directory-pin handoff, same-file/different-parent retargeting, controlled
replacement in the comparison/reopen gap (different bytes, identical bytes with
different identity, source aliases and same-run output aliases), denied-write
resync and successful identical-copy resync. This lifecycle clarification is
reviewed design, not execution evidence for the still-incomplete product backend.

## Existing-only native child-open contract

This narrow reviewed addendum permits generated `NtOpenFile` for existing child
observations and directory traversal. It supersedes the pack's Win32-only API
scope only for these read-only operations. The source-reviewed implementation
awaits native validation. It does not admit `NtCreateFile`, creation dispositions,
write/delete/ACL rights or publication, and does not close the failed creation
gate or the separate unqualified creation proposal above.

Reuse the already owned ordinary Win32 root handle as `RootDirectory`.
[Pinned Rust1.99 directory code](https://github.com/rust-lang/rust/blob/1.99.0/library/std/src/sys/fs/windows/dir.rs)
corroborates that handle interoperability; no unstable Rust API is used and no
second directory capture or absolute NT-name conversion follows from it.

Completion is based on the OS contract rather than successful samples.
[IRP_MJ_CREATE](https://learn.microsoft.com/en-us/previous-versions/windows/drivers/ifs/irp-mj-create)
covers existing-object opens and uses the synchronous-API request flag.
[Microsoft's completion description](https://learn.microsoft.com/en-us/windows-hardware/drivers/ifs/breaking-oplocks)
states that the I/O manager blocks these always-synchronous requests instead of
returning pending status to the caller. Lower-driver pending is handled inside
the I/O manager. The generic
[IO_STATUS_BLOCK rule](https://learn.microsoft.com/en-us/windows-hardware/drivers/ddi/wdm/ns-wdm-_io_status_block)
is conditional on an API returning pending; it does not override that fixed-open
contract. The wrapper must use `FILE_SYNCHRONOUS_IO_NONALERT` with `SYNCHRONIZE`,
without alertable, async or oplock-bypass options. Keep native name/structures,
IOSB and owned handles alive through the synchronous return. Returned final
NTSTATUS is authoritative; zero-initialized IOSB is never a success oracle.
Use Microsoft's [NT_SUCCESS semantics](https://learn.microsoft.com/en-us/windows-hardware/drivers/kernel/using-ntstatus-values):
success/informational values are nonnegative, warnings/errors negative. No exact
zero-only predicate or speculative adoption of failed output handles is added.
No async ownership framework, leak, forced library exit or early unsupported
return substitutes for completed I/O. An OS-blocked call still needs the
supervisor's owned-process hard stop; no bounded I/O latency is promised.

Leaf observation uses read-attributes/synchronize, observation sharing and
`FILE_OPEN_REPARSE_POINT` without `OBJ_DONT_REPARSE`. It reports positive reparse
kind and full identity before any ordinary-file signature. Ordinary-directory
traversal uses list/read-attributes/traverse/synchronize, observation sharing,
`FILE_DIRECTORY_FILE` and `OBJ_DONT_REPARSE`. Both use the fixed nonalert synchronous
option. The selected Win32 root now requests traverse access for this actual child
operation. No inheritance, kernel-only or backup-intent option is added to native
child opens. Enumeration names are passed exactly, without forced case folding.

Neither operation accepts a caller-provided flag mask or multi-component name.
Validate NUL, separators, dot components, streams/devices and the checked
`UNICODE_STRING`16-bit byte extent before the call. Native lookup remains
authoritative for filesystem-specific existing-name limits; destination naming's
actual component-limit query is still separate work. Native full128-bit identity
is never inferred from enumeration's64-bit record ID. `NtOpenError` retains exact
NTSTATUS and its secondary Win32 translation inside the I/O error payload; the
translation is not fabricated as that I/O error's primary raw code.

As with the Unix held-child primitive, the collector must compare the returned
directory identity with its earlier collected entry identity before descent.
The primitive does not claim a complete collector or atomic directory snapshot.

## Captured native path normalization

For Windows ordinary DOS/UNC inputs, capture the OS's Win32 normalization once
against the captured working-directory/drive state. Preserve the original native
input separately. Use GetFullPathNameW on the anchored absolute ordinary expression,
validate its native UTF-16 again, then form internal extended DOS/UNC syntax.
Do not blindly prefix a raw ordinary input or build a POSIX-style component walker.
This is an explicit Windows-only correction; POSIX symlink/.. traversal stays
unchanged. The supported user-verbatim subset is absolute DOS/UNC with canonical
backslashes and no raw dot/dot-dot components, rejected before I/O if outside that
subset. This is a conservative supported subset, not a claim about every path
Windows can interpret.

Before and after normalization reject NUL, ADS, device namespaces and reserved DOS
stems, including COM/LPT superscript digits and conservative console aliases.
Preserve unpaired UTF-16 surrogates. Apply the same authoritative component
validator to generated names after truncation: CONX truncated to CON is still
unsafe to open as CON.no-attachments.eml. Refuse rather than silently renaming.
Both native CPUs must compare direct ordinary CreateFileW with captured paths on
owned junction/.. fixtures, and test captured relative/root-relative/drive-relative
bases, UNC, non-Unicode and ordinary/verbatim long paths. No registry changes.

Capture uses one bounded GetFullPathNameW call per expression, never a relative
sizing/retry query. Rooted resolution and the current drive derive from the same
captured working expression; other per-drive states are captured once during an
admission phase in which the caller must not mutate cwd/drive state. There is no
OS atomic multi-drive snapshot. The operational extended content ceiling is
32766 UTF-16 units excluding NUL, including its prefix, below the documented
approximate32767-character API maximum. The core's broader32768-unit request
resource bound does not promise that every resulting native expression fits.

UNC share names PIPE, MAILSLOT and IPC$ are refused before I/O, case-insensitively,
in ordinary and verbatim forms. For this conservative namespace comparison only,
trailing ASCII spaces/periods are ignored; original native units are preserved.
This is not a universal SMB alias claim or a ban on disk shares ending in dollar.
Server/share namespace components receive structural validation; DOS filename
device rules apply to file/directory components rather than DNS server names.

## Newly observed mirrored directories

CreateDirectoryW success followed by open does not prove that the opened object
is the object created. Accepting another ordinary creator applies only to an
absent/unbound mirror edge; never rebaseline an already bound expected identity.
Before any descendant/candidate write, reject reparse/special children, validate
usable full IDs and retain each strict ordinary-directory pin through sync/address
checks. Preexisting incompatible writers/deleters fail binding.

Record creation-call success and the subsequently observed object separately.
Do not chmod, rewrite ACLs or delete a directory without proved creation ownership.
Unproven observed empty mirrored directories are retained as an explicit cleanup
limitation, separate from candidate-file residue. Conservatively flush child and
parent for every acknowledged or possibly-new edge. Retention is not namespace
rollback. Candidate CREATE_NEW still supplies its atomic owned handle and the
reviewed FileDisposition cleanup. Native tests must inject ordinary/reparse
replacement before pinning, preexisting writers/deleters and cancellation/errors,
proving all unowned directories and ACLs remain untouched. No broader NT creation
API is added.

Native API references: [FILE_RENAME_INFORMATION](https://learn.microsoft.com/en-us/windows-hardware/drivers/ddi/ntifs/ns-ntifs-_file_rename_information),
[NtSetInformationFile](https://learn.microsoft.com/en-us/windows-hardware/drivers/ddi/ntifs/nf-ntifs-ntsetinformationfile),
[CreateRestrictedToken](https://learn.microsoft.com/en-us/windows/win32/api/securitybaseapi/nf-securitybaseapi-createrestrictedtoken),
[CreateFile sharing](https://learn.microsoft.com/en-us/windows/win32/api/fileapi/nf-fileapi-createfilew),
[FlushFileBuffers access](https://learn.microsoft.com/en-us/windows/win32/api/fileapi/nf-fileapi-flushfilebuffers).

Path references: [normalization](https://learn.microsoft.com/dotnet/standard/io/file-path-formats#path-normalization),
[GetFullPathNameW](https://learn.microsoft.com/windows/win32/api/fileapi/nf-fileapi-getfullpathnamew),
[native naming](https://learn.microsoft.com/windows/win32/fileio/naming-a-file),
[long paths](https://learn.microsoft.com/windows/win32/fileio/maximum-file-path-limitation).

IPC references: [pipe names](https://learn.microsoft.com/en-us/windows/win32/ipc/pipe-names),
[mailslot names](https://learn.microsoft.com/en-us/windows/win32/ipc/mailslot-names),
[IPC$ scope](https://learn.microsoft.com/en-us/openspecs/windows_protocols/ms-wpo/4de75e21-36fd-440a-859b-75accc74487c).
