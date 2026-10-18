# Desktop worker ownership

The Slint product uses one minimal Darwin owner for its private core-backed
worker. No command-line product is involved. `eml-macos` contains the native
spawn/signal/reap boundary; eml-app owns request, control and completion policy.

The positive PID comes directly from successful posix_spawn. Polling and signals
share one tiny mutex. A terminal waitpid result revokes authority before unlock;
traced stops retain it, and unexpected ECHILD revokes it and fails explicitly.
There is no background reaper, delayed PID notification, process-name lookup or
negative/group-zero signal. Native operations are named Interrupt/Kill; the
supervisor owns cooperative/second-interrupt and bounded-Quit escalation policy.

Spawn requires a stable default SIGCHLD disposition and exclusive reaping. It
checks without changing the parent's handler. Child defaults and an empty signal
mask are explicit; Rust Command's inherited mask is insufficient. Standard
streams use stable CLOEXEC copies above2 and Darwin's close-default spawn policy.
The owned request pipe suppresses SIGPIPE on its open file description. Parent child-only ends
are closed once with errors retained, without retrying numeric descriptors.

Desktop processing adds a private child-writer pipe at descriptor3; all stable
source copies are above3. A bounded `R` byte acknowledges that the handler and
request reader are ready before the parent may feed its immutable request. The
first interrupt stops Core before writing `F`; only that actual observation
permits a second interrupt. These bytes are private lifecycle transport, separate
from the shared JSON outcome protocol. A completed normal worker may close after
`R` without receiving an interrupt. Invalid control bytes, live premature EOF and
missing readiness/interrupt acknowledgments fail explicitly.

An Ok SpawnObservation can retain both a real child and cleanup issues. The
supervisor must immediately retain that child and settle it before returning a
startup failure. Earlier pipe/dup failures use ordinary RAII and do not supply
checked-close receipts. Dropping OwnedChild does not terminate or reap it.

Stop/closing intent must remain monotonic through an in-flight launch. Apply it
before feeding a late successful child. No pipe I/O, blocking wait, reader join
or UI acknowledgement belongs under the native authority lock. The no-grandchild
worker contract is an ownership requirement, not a reason to retain a process
group identifier after its leader is reaped.

Completion requires terminal reap, request-writer settlement/explicit input
closure, both output EOF/reader settlements and final model application. Report
backpressure cannot block Stop or the event loop. Unknown lifetime closes later
admission. First Stop preserves completed outcomes; Force Stop sends the second
interrupt after confirmation. Confirmed Quit additionally has its bounded final
Kill path and retains explicit uncertainty if settlement cannot be proved.

The supervisor retains its positive-PID owner and endpoint transfer slots outside
its panic boundary. Reader/writer setup failures and panics cannot discard the
endpoints; the owner joins completed roles and checks their closes. Unresolved
native authority remains owned with a bounded pending warning. Report locking and
pipe backpressure do not share that authority lock. A pending warning remains a
failure after late settlement, and terminal observations remain terminal on
repeated polls. Known item outcomes are retained independently of lifecycle
failure or cancellation intent.

Five owned native tests passed locally: real stdio/EOF/normal exit, Interrupt
while waiting for input, live-child Kill, deliberately blocked spawning-thread
mask reset, and failed-spawn/no-authority behavior. Post-reap signal attempts
were rejected. Test cleanup failures abort later cases visibly and preserve the
original failure. These observations do not establish complete production
supervisor, inherited-descriptor, GUI or pinned native2 qualification.
