# macOS Finder and Shortcuts

For the native progress and report window, use the [macOS application build and installation guide](../macos-ui/README.md). Existing shortcuts should replace the shell command with the application launch command and remove Show Content. The direct launcher below remains suitable for Terminal and synchronous automation; the legacy Show Content setup remains available for installations without the native application.

Install the bundled launcher with:

```sh
/bin/sh integrations/macos-shortcuts/install.sh
```

The installer uses a private application-support directory and replaces only files it
owns through its managed-installation marker.

Create a Finder Quick Action in **Shortcuts** named **Create MIME-Pruned EML Copy**.
Configure it to receive **Files** from Finder, add **Run Shell Script**, set input to
**Shortcut Input**, select **as arguments**, and use this exact shell action:

```sh
/bin/sh "$HOME/Library/Application Support/EML Attachment Remover/run-from-finder.sh" "$@" || :
```

Immediately after it, add the current **Show Content** action and pass it the Run Shell
Script output. The `|| :` is intentional only in the Finder UI transport: it lets the
result action render a mixed or failed batch instead of Shortcuts stopping before the
UI action. The installed launcher itself retains its real processor exit status for
Terminal and automation callers.
Show Content is interactive: its Done button can keep the Shortcuts presentation
step open after the processor has completed. Use the installed launcher or zipapp
directly for unattended automation; their reports and statuses do not require
clicking a Shortcuts report. Do not use a modal presentation action as an unattended
completion signal.

The launcher defaults to `EML_REMOVER_EXISTING=verify`, so repeat use accepts only a
destination whose exact bytes equal the current verified candidate. It also accepts
`error`; unsupported values produce a configuration diagnostic. It forwards
HUP/INT/TERM to its child and validates the complete schema-3 terminal
receipt/count/exit contract. A signal to the launcher still displays complete
available results before temporary report cleanup, suppresses reveal actions, and
preserves the launcher signal status (129, 130, or 143). The child has 12 seconds to
finish; a repeated signal kills it immediately. A malformed or incomplete report is
rejected even during cancellation. When only the processor is interrupted, the
allowed disagreement is exit status 130 with a complete report whose `exit_code`
differs: a signal that arrived
while the finished report was being delivered; the receipts are kept and the
summary says the run was cut short. During an ordinary run it reveals only `created` or
`existing_verified` paths whose address receipt succeeded. Its visible Shortcuts
result always shows created, existing-verified, failed, not-run,
published-with-error, and cancelled totals, followed by bounded source-qualified
errors, warnings, batch errors, and interruption details. Do not substitute Quick
Look for the required Apple Mail field check.

CPython may instead exit 120 when buffered standard-stream flushing fails during
interpreter shutdown. The launcher validates any complete report, retains its
processing evidence, displays an output-finalization failure notice, suppresses
reveal, and exits 120. It rejects incomplete or invalid reports as before. The
report's processing `exit_code` can differ from this final process status; 120
always means the invocation failed, even when the available item receipts succeeded.

The launcher validates ordinary text addresses against their native representation.
For a POSIX address that cannot be represented as portable Unicode text, it uses the
strictly decoded `native_base64` value and never falls back to the display string.

Retained HTML is not sanitized. A related component can be removed while retained HTML
still references it; Apple Mail may show a missing-resource placeholder. This is an
expected MIME-pruning boundary, not a claim that appearance is unchanged.
