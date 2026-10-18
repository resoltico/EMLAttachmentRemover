# Windows destination boundary

The owner-approved Windows/CI correction selects WD1a: documented Win32 creation
and no-replace rename, with detection before message bytes. This boundary is
implemented and passed the complete product gate on both native gnullvm CPUs at
[39f7ee3](https://github.com/resoltico/EMLAttachmentRemover/actions/runs/39091719923).
This filesystem evidence does not qualify Windows archives or process supervision.

The required property, P-W, is that this app never stages or publishes message
bytes outside the bound destination, and never reports Created without final
copy/address qualification. It does not promise metadata immutability or prevent
other writers from changing final entries after qualification. Windows copies
inherit the destination ACL; owner-only Windows access is not promised.

## Approved design

Use generated windows-sys0.61.2 bindings. Hold the resolved ordinary ancestor
chain top-down with read/write sharing and no delete sharing. Upper ancestors
need attributes/traverse/synchronize access; modified directories additionally
need creation/flush access. Refuse reparse components and verify full identities.
No-delete sharing prevents ordinary move/delete while held. It does not exclude
an attribute-only open or conversion of an empty directory.

For one candidate file:

1. Capture the parent's normalized DOS/UNC path while checking its ordinary type
   and full identity. Validate a single component and the complete native length.
2. Create with CreateFileW/CREATE_NEW, read/write/delete access, no sharing and no
   inherited handle. The returned handle establishes this file's creation
   ownership. No message bytes have been written.
3. Check disk/ordinary/no-reparse type, full volume/file identity and zero length.
   Compare the candidate's OS-reported path with the intended parent's path plus
   that component, using identical normalized/DOS flags. Recheck the live parent
   identity/type and captured-path relation. A mismatch is AddressChanged;
   unavailable observation is AddressUnverified. Native tests must establish the
   original-parent relationship independently; these strings alone are not proof
   of that property under the tested interleavings.
4. Only after containment is established may message bytes be written. The held
   zero-sharing file keeps an entry in that directory; the documented nonempty
   directory rule prevents conversion while the ordinary pins prevent relocation.
   Keep ordinary-file/reparse/full-ID/length checks before writing and publishing.
5. Write, read back, compare and flush the held candidate. Publish by
   SetFileInformationByHandle/FileRenameInfo with an absolute final path,
   ReplaceIfExists false and RootDirectory null. Never enable replacement.
6. Preserve actual publication, file/namespace synchronization, requested-address
   and cleanup observations independently. Keep exact object identity alive
   across exclusive-candidate release and final reopening; the metadata-only pin
   supplies this lifetime without retaining the candidate's data-sharing denial.
   Published or Unknown never authorizes rollback.

A containment refusal may briefly create an EMPTY owned file outside the intended
parent. Cleanup uses that exact file handle, never a fresh deletion pathname.
Disposition, checked close and observable removal are separate facts; disposition
success alone is not Complete cleanup. Preserve the primary error when cleanup
also fails. Inconclusive observation remains Unknown.

## Mirrored directories and the occupancy guard

CreateDirectoryW returns a Boolean, not the new directory's handle. Opening its
name afterwards cannot prove that the opened object is the one just created:
another writer can move or replace it. Directory creation therefore yields a
bound/provenance observation, not an owned-directory deletion capability.

For each current mirror edge, first create and qualify an owned zero-byte file
in its parent by the candidate containment procedure above. Retain that exact
handle while creating the mirror with CreateDirectoryW, opening it no-follow,
checking ordinary type/identity/intended-parent relation, and pinning it without
delete sharing. The guard's presence makes the parent nonempty throughout that
window. Release the guard only after the child is bound, using checked owned
cleanup; any unresolved guard cleanup prevents successful admission/publication.
Guard count/lifetime is bounded to the current edge, not the entire tree.

A successful create call establishes that an edge was created, not that the
subsequently opened mirror is owned. Namespace work remains required independently
of deletion authority. Account for the guard's create/delete observations and the
actual held child/parent flushes. An unowned possibly-created edge still receives
its required synchronization. Do not infer synchronization from cleanup or vice
versa.

On pre-edge failure or cancellation, release unowned mirror handles and report
Unknown cleanup without deleting or guessing a residual component. The possible
mirror's ownership, location and contents are unverified: it need not still be
empty or at the originally requested address. Do not advise removal of a guessed
pathname. Keep true-owned file/guard cleanup and its exact native-error controls.
There is no recursive sweep. After Published or Unknown publication, keep layout;
a later error does not roll it back.

This ownership refinement and the narrow dependent test revisions were approved
by command central under the owner's WD1a/best-judgment authorization, in chat
01a1418d-b7a1-7830-936d-c9b5826b4388. It preserves P-W and the existing rule that
cleanup may delete only owned resources. It does not select WD1b/WD1c or change
the Windows floor. CreateDirectory2W returns a handle but requires Windows11
24H2/Server2025, so it is not the solution at the current floor.

## Design challenge and required evidence

- The sharing premise is withdrawn. Both native CPUs already demonstrated that
  an attribute-only handle can convert an empty directory despite write-sharing
  exclusion. Check the actual relied-on replacement fact with a zero-byte entry:
  conversion succeeds when empty and refuses with the documented nonempty error
  while that entry is held.
- GetFinalPathNameByHandleW documentation does not prove the original-parent
  relationship after conversion. Exercise conversion after the LAST validation
  and before the real creation call; observe the held original directory and the
  independent redirect target before cleanup erases evidence. Cover files,
  mirror guards/directory creation and replacement preservation.
- Captured and live paths must agree with the held identity. Do not silently
  replace paired observations with an unexplained cached path. Bound lengths;
  query/type/disposition/close failures retain truthful classifications.
- An ABA variant restores the parent's reparse state after redirected creation.
  It prevents a different early reparse check from masking the containment
  obligation. A containment-disabled, uncommitted negative control must produce
  actual outside byte effects detected by the SAME independent product oracle.
- Observe byte effects during staging, not just after publication/cleanup can move
  or remove the entry. P-W forbids message bytes outside, not every temporary
  empty entry. A same-ID check alone does not prove location.
- Tag an empty candidate before its first write; refuse nonordinary/reparse
  candidates. Preserve normal qualified Created, original bytes, no-replace,
  full-ID, final-address and actual flush controls on both native CPUs.
- A same-SID restricted token is not a different-user ACL boundary. Keep those
  scopes distinct. Native execution, including the independent namespace actor,
  is required; local cross-target checks cannot replace it.
- If a native run disproves a relied-on premise, stop and obtain the owner decision
  after that run. Do not keep an impossible assertion as a red required gate.

## Existing-only native child-open contract

The creation and publication paths no longer use `NtCreateFile` or
`NtSetInformationFile`. The separate existing-only child primitive retains
`NtOpenFile` with one validated relative UTF-16 component under the held parent.
It cannot create an entry. Observation opens the reparse leaf for attributes;
traversal and mutation pins use `OBJ_DONT_REPARSE`, directory-only and synchronous
nonalert options. The returned status, rather than the initially zero IOSB,
determines completion and ownership. Failed status retains NTSTATUS; a successful
call without a usable handle is refused. There is no asynchronous or oplock-
bypass mode, pending-stack-buffer retry or absolute NT namespace fallback.

This completion basis follows the I/O manager's synchronous IRP_MJ_CREATE
contract, including lower-driver pending completion described by Microsoft's
[Breaking Oplocks](https://learn.microsoft.com/en-us/windows-hardware/drivers/ifs/breaking-oplocks).
The [`NtOpenFile` contract](https://learn.microsoft.com/en-us/windows/win32/api/winternl/nf-winternl-ntopenfile)
and fixed options remain distinct from Win32 creation. Do not describe the
removal of the two mutation calls as removal of every NT API.

## Native acceptance

Both CPUs passed the unfiltered workspace tests, exact-name sentinels and
standalone native primitive at the source linked above. The product scenarios
exercise redirected candidate creation, redirected mirror/guard creation,
ordinary Created with hand-authored golden bytes, unchanged originals, actual
file/namespace synchronization, no-replace conflicts, tagged-candidate refusal
and unowned mirror replacement/cancellation without deletion.

The one-off compiled-out and ignored sentinel experiments were refused by name
on both CPUs. With only the containment predicate disabled in an owned clone,
the same byte-effect oracle observed 513 bytes at the redirect target and the
native test exited101. The protected run observed zero. Target placement was
independently established with the exact returned full file ID before restoring
the parent's reparse state; live file-handle size avoids stale directory-entry
metadata. These experiments are detection evidence, not qualifying artifacts.

## Primary contracts

- [CreateFileW](https://learn.microsoft.com/en-us/windows/win32/api/fileapi/nf-fileapi-createfilew): attribute access is outside data-sharing restrictions; CREATE_NEW, access/sharing and returned-handle semantics.
- [MS-FSA FSCTL_SET_REPARSE_POINT](https://learn.microsoft.com/en-us/openspecs/windows_protocols/ms-fsa/4aeefef8-92c3-4abc-af7a-a610caf8a165): eligible attribute access and nonempty-directory refusal.
- [CreateDirectoryW](https://learn.microsoft.com/en-us/windows/win32/api/fileapi/nf-fileapi-createdirectoryw): separate creation result and later open; no atomic handle ownership proof.
- [GetFinalPathNameByHandleW](https://learn.microsoft.com/en-us/windows/win32/api/fileapi/nf-fileapi-getfinalpathnamebyhandlew): normalized DOS/UNC path observations and their failure conditions.
- [CreateDirectory2W](https://learn.microsoft.com/en-us/windows/win32/api/fileapi/nf-fileapi-createdirectory2w): newer runtime floor; not used here.

## Earlier observations

At [b68680d Windows run39082273887](https://github.com/resoltico/EMLAttachmentRemover/actions/runs/39082273887), both native CPUs passed the metadata-pin/no-replace/readback case and49 library cases with the old assertion separately filtered. That assertion failed on both CPUs because empty-directory conversion succeeded. The earlier generic-write sharing denial did not establish attribute-only exclusion. These observations are retained as evidence; the disproved assertion is replaced by a passing platform-fact test, not retained as an intentional required failure or waived as passing.

Earlier relative-root FileRenameInfo returned error87; the absolute-name attempt
returned error32 under write-sharing exclusion. The selected absolute-name route
under read/write/no-delete pins passed natively on both CPUs, including normal
Created and conflict controls. No success is inferred
from the draft sketch's MSVC type check, generated layout checks or hashes.
