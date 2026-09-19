# macOS Finder and Shortcuts

Install the bundled launcher with:

```sh
/bin/sh integrations/macos-shortcuts/install.sh
```

The installer uses a private application-support directory. If a v2 bundle exists,
run its uninstaller, delete the old Shortcut manually, then install v3; v3 never
migrates a v2 bundle in place.

Create a Finder Quick Action in **Shortcuts** named **Create MIME-Pruned EML Copy**.
Configure it to receive **Files** from Finder, add **Run Shell Script**, set input to
**Shortcut Input**, select **as arguments**, and use this exact shell action:

```sh
/bin/sh "$HOME/Library/Application Support/EML Attachment Remover/run-from-finder.sh" "$@" || :
```

Immediately after it, add a **Show Result** action and pass it the Run Shell Script
output. The `|| :` is intentional only in the Finder UI transport: it lets Show
Result render a mixed or failed batch instead of Shortcuts stopping before the UI
action. The installed launcher itself retains its real processor exit status for
Terminal and automation callers.

The launcher defaults to `EML_REMOVER_EXISTING=verify`, so repeat use accepts only a
destination whose exact bytes equal the current verified candidate. It also accepts
`error`; removed values `skip` and `force` produce migration diagnostics. It forwards
HUP/INT/TERM to its child, waits for cleanup/reporting, validates the complete
schema-3 terminal receipt/count/exit contract, and reveals only `created` or
`existing_verified` paths whose address receipt succeeded. Its visible Shortcuts
result always shows created, existing-verified, failed, not-run,
published-with-error, and cancelled totals, followed by bounded source-qualified
errors, warnings, batch errors, and interruption details. Do not substitute Quick
Look for the required Apple Mail field check.

Retained HTML is not sanitized. A related component can be removed while retained HTML
still references it; Apple Mail may show a missing-resource placeholder. This is an
expected MIME-pruning boundary, not a claim that appearance is unchanged.
