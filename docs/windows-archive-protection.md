# Windows artifact enclosure

Archive extraction uses a new private directory under a canonical, natively
admitted ancestor chain. Each ordinary ancestor stays open without delete
sharing. Security snapshots require an admitted owner and refuse any applicable
untrusted allow ACE that can delete children, change attributes/ACL/owner or
otherwise mutate the existing namespace. Inherit-only ACEs do not grant access
to that existing directory. Denials are never used to excuse an unsafe allowance;
unknown current-object ACE forms fail conservatively.

The trusted principals are the effective creator UserSID, SYSTEM, administrators
and TrustedInstaller on OS ancestors. Current-user and privileged OS interference
is excluded, as for the Unix artifact enclosure's trusted UID/root. This does not
weaken Core's separate Windows P-W guarantees against the tested same-SID actor.

Creation supplies an explicit protected DACL and actual creator UserSID through
CreateDirectoryW. Only the creator, SYSTEM and administrators receive inherited
file/directory access. Native readback verifies the owner, protected DACL,
ordinary type and full identity. Parent admission is essential: a private child
DACL alone does not prevent deletion/replacement through a writable parent.
Existing directories are never chmodded, re-owned or assigned another ACL.
Drop closes handles and retains outputs/failure resources without a path sweep.

The implementation uses safe eml-fs capabilities from xtask. Native SDK buffers,
ACL entries and SIDs are checked against retained allocation extents. Source
Clippy on both Windows targets is not integration proof.

The required native test creates only a temporary account on a fresh hosted VM,
authenticates synthetic in-memory credentials, verifies a genuinely different
effective UserSID with no administrator membership, and makes actual path opens
under that identity. A successful writable-control operation accompanies private
read/write denials. No profile is loaded, existing account or policy changed,
or credential written to a command line/environment/log. The original thread
identity is restored and verified before further work. Only a still-matching
temporary account SID may be deleted; failed restoration aborts and failed
cleanup fails the test. A same-SID restricted token is not this proof.

Native x64/ARM execution and complete Windows packaging remain required.

The first native run rejected the default hosted workspaces: x64's D: owner was
outside the admitted principals, and ARM's C:\a allowed untrusted mutation.
Those are real failed prerequisites, not exceptions to the namespace policy.
Hosted verification now prepares a new exact-source checkout beneath an admitted
user-profile temporary enclosure. Existing workspace ACLs stay unchanged. Tests,
compiler scratch and retained evidence live under that checkout's target/. The
new location must pass native admission and the distinct-user test before it can
establish a protected extraction gate.
