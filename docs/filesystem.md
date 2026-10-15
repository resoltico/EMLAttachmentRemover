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
