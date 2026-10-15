# Filesystem implementation contract corrections

The native filesystem business workflow is still being implemented. These
reviewed corrections preserve the safety requirements; they are not product
qualification evidence. The original design archive and checksum remain intact.

## POSIX namespace admission before creating an entry

A pathname identity check followed by rmdir/unlinkat is not atomic compare-and-
delete. A controlled same-UID interleaving checked owned-stage, moved it aside,
moved another owned empty directory into that name, then removed the replacement.
This demonstrated the mechanism; it was not a cross-UID exploit test. A different
UID with suitable nonsticky shared-parent permissions can perform those renames.
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
native Mac execution and the added Linux named-ACL-mask cases still require actual
run evidence. No raw credential code, account or owner-preference change is used.

Credential references: [Rust1.99 process setup](https://github.com/rust-lang/rust/blob/1.99.0/library/std/src/sys/process/unix/unix.rs#L302-L330),
[Darwin credentials](https://github.com/apple-oss-distributions/xnu/blob/main/bsd/kern/kern_prot.c).

References: [unlink/unlinkat](https://man7.org/linux/man-pages/man2/unlink.2.html),
[POSIX ACL mask and mode](https://man7.org/linux/man-pages/man5/acl.5.html),
[Apple vnode authorization](https://github.com/apple-oss-distributions/xnu/blob/main/bsd/vfs/vfs_subr.c).

## Selection spelling, links and overlap

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
Missing/unbindable roots use INPUT_ADMISSION_FAILED without writes. Check physical
output overlap before walking. OUTPUT_OVERLAP is a streamed run failure: start+
summary only, incomplete collection, known items not_processed, exit1. Unexpanded
directories are not fabricated items, so directory-only overlap can have total0.
It is distinct from NO_INPUTS and from invalid missing/non-directory output-root
requests, which remain pre-stream exit2. These rules also apply to dry run.
