# Exact-source modified-host picker admission shared by CI and publication.
# Preparation is never product qualification; digests bind tested bytes only.
def clean:
  .cancelled == false and .timed_out == false and
  .cleanup_complete == true and .unexpected_survivors == 0;
def byte_identity:
  (.byte_length | type) == "number" and .byte_length > 0 and
  (.byte_length | floor) == .byte_length and
  (.sha256 | type) == "string" and (.sha256 | test("^[0-9a-f]{64}$"));
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
def unsigned($maximum):
  type == "number" and . >= 0 and . <= $maximum and floor == .;
def native_command:
  (.process | clean) and (.process | has("signal")) and .process.signal == null and
  (.process.exit_code | unsigned(2147483647));
def launchctl($arguments):
  .program == "/bin/launchctl" and .arguments == $arguments and native_command;
def service_target($uid):
  "gui/" + ($uid | tostring) + "/com.apple.iconservices.iconservicesagent";
def gui_caller:
  (.uid | unsigned(4294967295)) and .uid > 0 and
  .uid == .euid and .uid == .console_uid and
  .security_session.status == 0 and .security_session.graphic_access == true and
  (.security_session.security_session_id | unsigned(4294967294)) and
  .security_session.security_session_id > 0;
def selected_host($e):
  native_host and
  .image_os == $e.picker_host.image_os and .image_version == $e.picker_host.image_version and
  .product_version == $e.picker_host.product_version and .build_version == $e.picker_host.build_version and
  (.boot_uuid | test("^[0-9a-fA-F]{8}(-[0-9a-fA-F]{4}){3}-[0-9a-fA-F]{12}$"));
def host_binding($e):
  (has("failure") or has("error") | not) and
  identity($e) and (.picker_host | type) == "object" and .picker_host == $e.picker_host and
  (.host | selected_host($e)) and (.caller | gui_caller);
def disabled_readback($uid; $required):
  launchctl(["print-disabled", "gui/" + ($uid | tostring)]) and .process.exit_code == 0 and
  (has("override")) and (.override == null or (.override | type) == "boolean") and
  (if $required then .override == true else true end);
def absent_service($uid):
  launchctl(["print", service_target($uid)]) and .process.exit_code == 113 and
  .selected_fields == {} and
  .absence_diagnostic == ("Could not find service \"com.apple.iconservices.iconservicesagent\" in domain for user gui: " + ($uid | tostring));
def loaded_service($uid):
  .target == service_target($uid) and
  (.disabled | disabled_readback($uid; false)) and
  (.print | launchctl(["print", service_target($uid)]) and .process.exit_code == 0 and
    has("absence_diagnostic") and .absence_diagnostic == null and
    .selected_fields.path == "/System/Library/LaunchAgents/com.apple.iconservices.iconservicesagent.plist" and
    .selected_fields.program == "/System/Library/CoreServices/iconservicesagent" and
    .selected_fields.type == "LaunchAgent" and
    (.selected_fields.state | type) == "string" and (.selected_fields.state | length) > 0 and
    (.selected_fields.domain | capture("^gui/" + ($uid | tostring) + " \\[(?<audit>[0-9]+)\\]$").audit | tonumber | unsigned(4294967295) and . > 0));
def provisioned_service($uid):
  .target == service_target($uid) and (.print | absent_service($uid)) and
  (.disabled | disabled_readback($uid; true));
def native_text:
  type == "string" and length > 0 and length <= 4096;
def crash_frame:
  has("symbol") and (.symbol == null or (.symbol | native_text)) and
  (.image_name | native_text) and (.image_path | native_text);
def crash_report:
  (.file_name | type) == "string" and (.file_name | length) <= 255 and
  (.file_name | startswith("iconservicesagent-")) and (.file_name | endswith(".ips")) and
  (.file_name | contains("/") or contains("\\") | not) and
  (.identity | byte_identity) and .identity.byte_length <= 8388608 and
  (.pid | unsigned(4294967295)) and .pid > 0 and (.user_id | unsigned(4294967295)) and
  (.boot_uuid | native_text) and (.os_build | native_text) and (.capture_time | native_text) and
  (.proc_path | native_text) and (.exception_signal | native_text) and
  (.termination.namespace | native_text) and (.termination.code | unsigned(4294967295)) and
  (.faulting_thread.index | unsigned(4294967295)) and .faulting_thread.triggered == true and
  (.faulting_thread.frames | type) == "array" and
  (.faulting_thread.frames | length) > 0 and (.faulting_thread.frames | length) <= 256 and
  all(.faulting_thread.frames[]; crash_frame);
def known_crash($host; $uid):
  .user_id == $uid and .boot_uuid == $host.boot_uuid and .os_build == $host.build_version and
  .proc_path == "/System/Library/CoreServices/iconservicesagent" and .exception_signal == "SIGABRT" and
  .termination.namespace == "SIGNAL" and .termination.code == 6 and
  .faulting_thread.triggered == true and
  any(.faulting_thread.frames[];
    (.symbol | type) == "string" and (.symbol | contains("MTLLoader sliceIDForDevice:")) and
    .image_name == "Metal" and .image_path == "/System/Library/Frameworks/Metal.framework/Versions/A/Metal") and
  any(.faulting_thread.frames[];
    (.symbol | native_text) and .image_name == "RenderBox" and
    .image_path == "/System/Library/PrivateFrameworks/RenderBox.framework/Versions/A/RenderBox");
def host_preparation($e):
  try ((. as $p | .stage == "picker_host_preparation" and host_binding($e) and
    (.service_before | loaded_service($p.caller.uid)) and
    (.crash_reports | type) == "array" and (.crash_reports | length) > 0 and
    (.crash_reports | length) <= 12 and all(.crash_reports[]; crash_report) and
    any(.crash_reports[]; known_crash($p.host; $p.caller.uid)) and
    (.bootout | launchctl(["bootout", service_target($p.caller.uid)])) and
    (if .bootout.process.exit_code == 0 then has("bootout_absence") and .bootout_absence == null
     else (.bootout_absence | absent_service($p.caller.uid)) end) and
    (.disable | launchctl(["disable", service_target($p.caller.uid)]) and .process.exit_code == 0) and
    (.service_after | provisioned_service($p.caller.uid))) // false) catch false;
def host_verification($e):
  try ((. as $p | .stage == "picker_host_verification" and host_binding($e) and
    (.service | provisioned_service($p.caller.uid))) // false) catch false;
def qualification($e):
  try (((has("failure") or has("error") | not) and
    identity($e) and .stage == "picker_qualification" and .mode == "modified_host" and
    .picker_host == $e.picker_host and
    (.preparation | host_preparation($e)) and (.product | product($e)) and
    (.postverification | host_verification($e)) and
    .preparation.host == .postverification.host and .preparation.caller == .postverification.caller and
    .product.host.target == .preparation.host.target and
    .product.host.process_os == .preparation.host.process_os and
    .product.host.process_architecture == .preparation.host.process_architecture and
    .product.host.kernel_machine == .preparation.host.kernel_machine and
    .product.host.product_version == .preparation.host.product_version and
    (.product.host.actual_os | capture("(?m)^BuildVersion:[ \\t]*(?<build>[^\\s]+)[ \\t]*$").build) == .preparation.host.build_version) // false) catch false;
