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

## Selected boundary

- Keep Win32 creation under the resolved, held ordinary ancestor chain. No
  write or delete sharing on pinned directories; no candidate sharing.
- Create the candidate directly in its bound destination, hold its exclusive
  synchronous DELETE/SYNCHRONIZE-capable handle, write/reread/synchronize it.
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
- NtCreateFile, handwritten native layouts, absolute-name rename, MoveFileEx,
  replacement and relaxed sharing remain excluded. Only this rename operation
  is added to the narrow reviewed native boundary.

The restricted-token probe uses the runner account's thread token with the
Administrators SID deny-only and DISABLE_MAX_PRIVILEGE. Windows retains
SeChangeNotifyPrivilege. This is actual restricted access, not a separately
qualified standard-user interactive login. Each adversarial actor thread must
establish its own intended token; impersonation is thread-local.

Full WP3 still requires actual junction/reparse mutation and pre-existing writer
controls, source/destination/candidate substitutions, entry conflicts, post-edge
failures and cleanup cases on both native CPUs. The early gate is not a waiver.

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
