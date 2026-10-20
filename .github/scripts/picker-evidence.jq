# Exact-source picker admission shared by normal observation, controlled-host
# preparation, final CI selection and release. Digests bind tested bytes only.
def clean:
  .cancelled == false and .timed_out == false and
  .cleanup_complete == true and .unexpected_survivors == 0;
def byte_identity:
  (.byte_length | type) == "number" and .byte_length > 0 and
  (.byte_length | floor) == .byte_length and
  (.sha256 | type) == "string" and (.sha256 | test("^[0-9a-f]{64}$"));
def probe_identity:
  (.path | type) == "string" and
  (.signature_display | type) == "string" and
  (.signature_display | contains("Mach-O thin (x86_64)")) and
  (.signature_display | contains("Signature=adhoc")) and
  (.files["Contents/Info.plist"] | byte_identity) and
  (.files["Contents/MacOS/NativePanelDiagnostic"] | byte_identity) and
  (.files["Contents/_CodeSignature/CodeResources"] | byte_identity);
def identity($e):
  .source == $e.source and .target == $e.target and
  .run == $e.run and .attempt == $e.attempt and
  .workflow_ref == $e.workflow_ref and .workflow_sha == $e.workflow_sha and
  .archive == $e.archive and .reference == $e.reference;
def passed:
  .totalTestCount == 1 and .passedTests == 1 and
  .failedTests == 0 and .skippedTests == 0 and .expectedFailures == 0;
def native_host:
  .target == "x86_64-apple-darwin" and .process_os == "macos" and
  .process_architecture == "x86_64" and .kernel_machine == "x86_64" and
  (.product_version | split(".")[0]) == "26";
def normal_owner:
  has("failure") and has("source_check_failure") and
  .failure == null and .timed_out == false and .cancelled == false and
  .runner_reaped == true and .runner_status == 0 and
  .owned_app_terminated == true and .forced_app_cleanup == false and
  .originals_captured == 4 and .originals_unchanged == true and
  .source_check_failure == null and .owned_pasteboard_released == true and
  .unexpected_application_pids == [];
def product($e):
  . as $p |
  .stage == "picker_compatibility" and .source == $e.source and
  .target == $e.target and (.host | native_host) and
  .verification.stage == "picker_compatibility" and
  .verification.snapshot == $e.archive and
  (.verification.picker as $c |
    $c.source == $e.source and $c.case == "picker" and
    $c.test_method == "testOwnedNativePickerCompatibility" and
    ($c.summary | passed) and ($c.process | clean) and
    ($c.process | has("signal")) and $c.process.exit_code == 0 and $c.process.signal == null and
    ($c.lifetime_owner | normal_owner) and
    $c.picker_observation.schema == 1 and
    $c.picker_observation.scenario == "picker" and
    ($c.picker_observation.owned_pid | type) == "number" and $c.picker_observation.owned_pid > 0 and
    ($c.picker_observation.owned_pid | floor) == $c.picker_observation.owned_pid and
    ($c.picker_observation.message | type) == "string" and
    ($c.picker_observation.folder | type) == "string" and
    $c.picker_observation.owned_pid == $c.ownership_receipt.pid and
    ($c.ownership_receipt.launch_date | type) == "number" and $c.ownership_receipt.launch_date > 0 and
    $c.app == $c.ownership_receipt.bundle_path and
    $c.ownership_receipt == $c.lifetime_owner.ownership_receipt and
    $c.persistent_preferences.comparison.real_pickers_observed == true and
    $c.persistent_preferences.comparison.application_and_other_preferences_unchanged == true);
def known_failure($e):
  identity($e) and .stage == "picker_normal_observation" and
  .outcome == "known_panel_failure" and .original_exit_code == 1 and
  (.process | clean) and (.process | has("signal")) and .process.exit_code == 1 and .process.signal == null and
  (.owner as $o |
    $o.failure != null and $o.runner_reaped == true and $o.runner_status == 65 and
    $o.owned_app_terminated == true and $o.forced_app_cleanup == false and
    $o.timed_out == false and $o.cancelled == false and
    $o.originals_captured == 4 and $o.originals_unchanged == true and
    ($o | has("source_check_failure")) and $o.source_check_failure == null and $o.owned_pasteboard_released == true and
    $o.native_picker_diagnostic_settled == true and $o.unexpected_application_pids == [] and
    $o.fixture_output_relay.enabled == true and $o.fixture_output_relay.eof_observed == true and
    $o.fixture_output_relay.drain_within_deadline == true and $o.fixture_output_relay.failure == null and
    $o.native_picker_owned_process_log.pid == $o.ownership_receipt.pid) and
  (.xctest_log | contains("error: -[DesktopInteraction.DesktopInteractionTests testOwnedNativePickerCompatibility] : failed - Native input panel did not appear")) and
  (.owned_log | contains("Advance to configuration phase semaphore timed out"));
def preparation($s):
  .stage == "observed" and .failure == null and
  .original_failure_remains_authoritative == true and .baseline.source == $s and
  .baseline.host.target == "x86_64-apple-darwin" and
  .baseline.host.process_os == "macos" and .baseline.host.process_architecture == "x86_64" and
  .baseline.host.kernel_machine == "x86_64" and
  (.baseline.host.actual_os | test("ProductVersion:\\s+26\\.")) and
  (.baseline | has("entitlements")) and .baseline.entitlements == null and
  (.baseline.identity | probe_identity) and
  (.baseline.process | clean) and .baseline.process.exit_code == 2 and
  (.unchanged_repeat.process | clean) and .unchanged_repeat.process.exit_code == 2 and
  (.bootout.process | clean) and .bootout.process.exit_code == 0 and
  (.intervention_probe.process | clean) and .intervention_probe.process.exit_code == 0 and
  (.gui_uid | type) == "number" and .gui_uid > 0 and
  (.gui_uid | floor) == .gui_uid and .gui_uid <= 4294967295 and
  .target == ("gui/" + (.gui_uid | tostring) + "/com.apple.iconservices.iconservicesagent") and
  .bootout.program == "/bin/launchctl" and .bootout.arguments == ["bootout", .target] and
  .baseline.identity == .unchanged_repeat.identity and
  .baseline.identity == .unchanged_repeat.identity_after and
  .baseline.identity == .intervention_probe.identity and
  .baseline.identity == .intervention_probe.identity_after;
def normal_observation($e):
  identity($e) and
  (if .outcome == "normal_pass" then
    .stage == "picker_normal_observation" and .original_exit_code == 0 and (.product | product($e))
   else known_failure($e) end);
def modified_qualification($e):
  identity($e) and .stage == "picker_modified_qualification" and
  (.normal | known_failure($e)) and (.preparation | preparation($e.source)) and
  .preparation.purpose == "picker_qualification_preparation" and
  .preparation.baseline.run == $e.run and .preparation.baseline.attempt == $e.attempt and
  (.product | product($e));
def qualification($e):
  identity($e) and .stage == "picker_qualification" and
  (.normal | normal_observation($e)) and (.product | product($e)) and
  (if .mode == "normal" then
    .normal.outcome == "normal_pass" and .modified == null and .product == .normal.product
   elif .mode == "modified_host" then
    (.normal | known_failure($e)) and (.modified | modified_qualification($e)) and
    .modified.normal == .normal and .product == .modified.product
   else false end);
