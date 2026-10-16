# Helper process ownership

The app uses one narrow Darwin child owner for its bundled helper's launch,
signals and reap. Foundation still supplies pipes, data, URLs and the transport
model. The design pack's Foundation Process mechanism was changed to preserve
its ownership-safe final SIGKILL requirement: Foundation does not expose an
unconditional launch-bound hard-kill capability, and a later PID lookup is not
such a capability. The owner delegated the resolution with “qa and proceed per
your best judgment”; the separately challenged QA decision selected this minimal
native ownership boundary. The CLI protocol and Rust business libraries are
unchanged by that decision.

HelperChild obtains its positive PID directly from posix_spawn. One shared
Mutex-protected reference holds the revocable authority. The supervisor actor
alone polls waitpid with WNOHANG and sends INT/KILL; terminal reap clears authority
inside the same critical section before another signal can use it. Traced stop
status retains ownership; unexpected ECHILD revokes authority and fails explicit
qualification. Aliases share revocation. There is no background reaper with a
delayed PID notification, process-name lookup, group-zero send, Mach capability
exchange, debugger entitlement or watchdog process.

The app requires a stable default SIGCHLD disposition and no competing wait-family
calls. Spawn checks the current disposition without changing it; continued
stability and exclusive reaping remain application-wide requirements. Child
signal defaults/mask are explicit. The executable's native path comes from the
bundled URL, with no shell or PATH fallback. Standard descriptors are copied above 2 with
CLOEXEC before child redirection; parent child-only pipe ends close on success
and failure. A successfully spawned child remains owned if later startup cleanup
fails and is settled before that error returns. Request-pipe SIGPIPE suppression
is per descriptor.

Each request has one monotonic HelperRunControl. A UI Stop records its intent
synchronously before notifying the actor, so admission scheduling cannot lose it.
Native notifications remain bound to the admitted reference and launch UUID.
Before request feeding, stopped input is withheld; once beginFeeding grants
permission, input may have been sent, even before a physical write. That boundary
cannot support a no-work claim. A stop attempt against no child does not consume
the delivery latch; successful handoff rechecks intent. Forced prelaunch
settlement may use the retained native KILL authority and is not described as two
delivered CLI callbacks.

Blocking launch and pipe operations use dedicated threads. The actor's wait poll
does not block for future child state, so it remains able to handle controls.
Stdout decoding uses the real bounded LF splitter/reducer, with at most one
unacknowledged MainActor snapshot. Stderr drains independently while retaining
at most 64 KiB. Errors notify the correct actor phase before presentation waits;
drainage continues without further decoding after malformed output. Completion
requires terminal reap, writer settlement, both EOF paths and final acknowledged
presentation. A generic valid dry-run stream remains valid model data but is
refused by this app's Create-only reader.

The version probe drains/reaps before real processing and requires exact output
and normal success; its monotonic deadline uses the same owned kill path. Normal
run completion preserves Core's summary/actual-exit agreement; a late Stop does
not rewrite completed outcomes. Confirmed-quit admission gating and its 10 s / 2 s
escalation are still being connected to this owner and are not qualified yet.

Local real-helper fixtures have exercised golden Create/UpToDate, pre-stream
refusal, pre-admission stop/force, control-character native paths, owned KILL9,
shared revocation and pipe/ack settlement. These are additional installed-Xcode
ARM results, distinct from signed GUI/app/archive and pinned native-two-platform
qualification. Exact spawn-inflight scheduling remains source-reviewed rather
than claimed as a deterministic native test. The Rust macos-process-tests command
selects the pinned Xcode/default SDK/toolchain and derives target/minimum/version
facts from existing authorities; useful failure records remain under target/.
