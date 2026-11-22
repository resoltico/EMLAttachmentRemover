#!/bin/sh
# SPDX-License-Identifier: AGPL-3.0-only
# Self-contained user installer. Shared engine; only this literal block differs.
# BEGIN PRODUCT SETTINGS
product_name='EML Attachment Remover'
command_name='emlattachmentremover'
repo_url='https://github.com/resoltico/EMLAttachmentRemover'
env_prefix='EMLATTACHMENTREMOVER'
archive_prefix='emlattachmentremover-'
archive_separator='-'
archive_versioned=1
archive_rooted=0
macos_x64='macos-x86_64'
macos_arm64='macos-aarch64'
linux_x64='linux-x86_64'
linux_arm64='linux-aarch64'
checksum_name='SHA256SUMS'
minimum_macos=15
legal_files='LICENSE THIRD-PARTY-NOTICES.txt PROJECT-NOTICE.md LICENSE-COMMERCIAL.md SOURCE.txt'
# END PRODUCT SETTINGS

set -eu
umask 077
docs_revision='main'
docs_url=${repo_url}/blob/${docs_revision}/docs/INSTALL.md
marker="# added by the ${product_name} installer"
tmp='' stage='' locked=0 transition=0 binary_switched=0 state_moved=0 state_switched=0 path_locked=0 recovery='' path_preparation=''
rewrite_count=0 rewrite_owned=0 rewrite_recovery=0 cleaning=0
uninstall_transition=0 uninstall_binary_moved=0 uninstall_state_moved=0

# Names here come only from this script and its literal settings, never user data.
setting() { eval "printf '%s' \"\${${env_prefix}_$1-}\""; }
shell_quote() { printf "'"; printf '%s' "$1" | sed "s/'/'\\\\''/g"; printf "'"; }
fish_quote() { printf "'"; printf '%s' "$1" | sed -e 's/\\/\\\\/g' -e "s/'/\\\\'/g"; printf "'"; }
step() { printf '  + %-11s %s\n' "$1" "$2"; }
note() { printf '  ! %-11s %s\n' "$1" "$2"; }
die() { printf '\n  Error: %s\n' "$1" >&2; shift; for message in "$@"; do printf '    %s\n' "${message}" >&2; done; printf '    Help: %s\n' "${docs_url}" >&2; exit 1; }
usage() {
    cat <<EOF
Install, update or uninstall ${product_name} for the current user.
Usage: install.sh [--version X.Y.Z] [--install-dir DIR] [--no-modify-path] [--replace-unmanaged] [--uninstall]
Defaults: \$XDG_BIN_HOME or ~/.local/bin; no administrator access required.
--replace-unmanaged explicitly backs up and replaces only a receiptless regular executable.
--no-modify-path leaves shell startup files untouched, including on uninstall.
Environment: ${env_prefix}_VERSION, ${env_prefix}_INSTALL_DIR,
${env_prefix}_NO_MODIFY_PATH=1, ${env_prefix}_DOWNLOAD_BASE (HTTPS mirror or local directory).
EOF
}

stable_version() { printf '%s\n' "$1" | grep -Eq '^(0|[1-9][0-9]*)\.(0|[1-9][0-9]*)\.(0|[1-9][0-9]*)$' || die "Not a stable release version: $1"; }
require_line_path() { case "$1" in *'
'* | *"$(printf '\r')"*) die 'Paths containing newline or carriage return cannot be managed.' ;; *) : ;; esac; }
validate_settings() {
    case "${env_prefix}" in '' | *[!A-Z0-9_]* | [0-9]*) die 'Invalid installer environment prefix.' ;; *) : ;; esac
    case "${command_name}" in '' | *[!A-Za-z0-9_-]*) die 'Invalid installer command name.' ;; *) : ;; esac
    case "${archive_versioned}" in 0 | 1) : ;; *) die 'Invalid archive naming setting.' ;; esac
    case "${archive_rooted}" in 0 | 1) : ;; *) die 'Invalid archive layout setting.' ;; esac
    for token in "${archive_prefix}" "${archive_separator}" "${checksum_name}"; do
        case "${token}" in '' | *[!A-Za-z0-9_.-]* | . | ..) die 'Invalid installer archive setting.' ;; *) : ;; esac
    done
    for suffix in "${macos_x64}" "${macos_arm64}" "${linux_x64}" "${linux_arm64}"; do
        case "${suffix}" in *[!A-Za-z0-9_.-]* | . | ..) die 'Invalid installer platform suffix.' ;; *) : ;; esac
    done
    for legal in ${legal_files}; do
        case "${legal}" in *[!A-Za-z0-9_.-]* | . | ..) die 'Legal filenames must be simple basenames.' ;; *) : ;; esac
    done
}
parse_args() {
    login_shell=${SHELL:-}
    version=$(setting VERSION); install_dir=$(setting INSTALL_DIR)
    version_requested=0
    [ -z "${version}" ] || version_requested=1
    download_base=$(setting DOWNLOAD_BASE); modify_path=1; action=install; replace_unmanaged=0
    no_modify=$(setting NO_MODIFY_PATH)
    [ "${no_modify}" != 1 ] || modify_path=0
    while [ "$#" -gt 0 ]; do
        case "$1" in
            --version) if [ "$#" -ge 2 ] && [ -n "$2" ]; then :; else die '--version needs a nonempty value.'; fi; version=$2; version_requested=1; shift ;;
            --version=*) version=${1#*=}; [ -n "${version}" ] || die '--version needs a nonempty value.'; version_requested=1 ;;
            --install-dir) if [ "$#" -ge 2 ] && [ -n "$2" ]; then :; else die '--install-dir needs a nonempty directory.'; fi; install_dir=$2; shift ;;
            --install-dir=*) install_dir=${1#*=}; [ -n "${install_dir}" ] || die '--install-dir needs a nonempty directory.' ;;
            --no-modify-path) modify_path=0 ;;
            --replace-unmanaged) replace_unmanaged=1 ;;
            --uninstall) action=uninstall ;;
            -h | --help) usage; exit 0 ;;
            *) die "Unknown option: $1" 'Run with --help for options.' ;;
        esac
        shift
    done
    if [ "${action}" = uninstall ] && [ "${replace_unmanaged}" = 1 ]; then die '--replace-unmanaged is install-only.'; fi
    version=${version#v}
    if [ "${version_requested}" = 1 ]; then stable_version "${version}"; fi
    [ -n "${HOME:-}" ] || die 'HOME is not set.'
    [ -n "${install_dir}" ] || install_dir=${XDG_BIN_HOME:-${HOME}/.local/bin}
    case "${install_dir}" in /*) ;; *) die 'The install directory must be absolute.' ;; esac
    require_line_path "${install_dir}"
    while [ "${install_dir}" != / ] && [ "${install_dir%/}" != "${install_dir}" ]; do install_dir=${install_dir%/}; done
    binary=${install_dir%/}/${command_name}
    state=${install_dir%/}/.${command_name}-install
    lock=${install_dir%/}/.${command_name}-install.lock
}

sha256_of() {
    if command -v sha256sum >/dev/null 2>&1; then digest_output=$(sha256sum < "$1") || die "Could not read or hash: $1"
    elif command -v shasum >/dev/null 2>&1; then digest_output=$(shasum -a 256 < "$1") || die "Could not read or hash: $1"
    elif command -v openssl >/dev/null 2>&1; then digest_output=$(openssl dgst -sha256 < "$1") || die "Could not read or hash: $1"
    else die 'A SHA-256 tool (sha256sum, shasum or openssl) is required.'; fi
    printf '%s\n' "${digest_output}" | awk '{ if (length($1)==64) print tolower($1); else print tolower($NF) }' || return 1
}
require_regular() { if [ -f "$1" ] && [ ! -h "$1" ] && [ -r "$1" ]; then :; else die "Installer-owned file is missing, nonregular or unreadable: $1" 'No program was replaced or removed. Inspect this exact file; do not delete the shared directory.'; fi; }
owned_error() { die "Cannot verify installer receipt: ${state}" 'Receipt contents, manifest or owned artifacts are incomplete or changed. No program was replaced or removed.' 'Preserve this state for inspection, or choose another install directory.'; }
# Inspect each existing ancestor: permission denial must never masquerade as absence.
check_directory() {
    checked=$1
    while [ ! -e "${checked}" ] && [ ! -h "${checked}" ]; do
        [ "${checked}" != / ] || break
        checked=${checked%/*}; [ -n "${checked}" ] || checked=/
    done
    if [ -d "${checked}" ] && [ -r "${checked}" ] && [ -x "${checked}" ]; then :; else
        die "Install directory or ancestor is nonregular or inaccessible: ${checked}"
    fi
    [ -w "${checked}" ] || die "Install directory or ancestor is not writable: ${checked}" 'Nothing was changed. Check directory permissions before retrying.'
}
refuse_unmanaged() {
    docs_url=${docs_url}#existing-executable-without-an-installer-receipt
    if [ "${action}" = uninstall ]; then
        die "Existing executable has no installer receipt: ${binary}" \
            'This installer has no record that it installed this file, so it cannot remove it.' \
            'No program or PATH entry was changed. Use the method that installed it to remove it; if its origin is unknown, inspect and preserve it before deciding what to remove.'
    fi
    retry_options="--install-dir $(shell_quote "${install_dir}")"
    if [ "${version_requested}" = 1 ]; then retry_options="${retry_options} --version $(shell_quote "${version}")"; fi
    if [ "${modify_path}" = 0 ]; then retry_options="${retry_options} --no-modify-path"; fi
    retry_options="${retry_options} --replace-unmanaged"
    # With stdin execution, $0 names the shell rather than a saved installer.
    retry_script=''
    case "${0##*/}" in sh | dash | bash | ash | ksh | zsh | -*) : ;;
        *) if [ -f "$0" ] && [ -r "$0" ]; then
            case "$0" in /*) retry_script=$0 ;; *) retry_script=$(pwd -P)/$0 ;; esac
        fi ;;
    esac
    if [ -n "${retry_script}" ]; then
        retry_instruction="To replace only this file and keep its original bytes in a recovery backup, run:"
    else
        retry_script=./install-${command_name}.sh
        retry_instruction="This installer cannot locate its saved file. Save this installer as ${retry_script}, then run this example to replace only the named executable and keep its original bytes in a recovery backup:"
    fi
    quoted_script=$(shell_quote "${retry_script}")
    die "Existing executable has no installer receipt: ${binary}" \
        'This installer has no record that it installed this file, so replacement needs your explicit choice.' \
        'No program or PATH entry was changed.' \
        "${retry_instruction}" \
        "sh ${quoted_script} ${retry_options}" \
        'The backup location will be printed. Keep it until you have verified the replacement.'
}

inventory() {
    check_directory "${install_dir}"
    if [ ! -e "${lock}" ] && [ ! -h "${lock}" ]; then :; else
        die "Installation lock exists: ${lock}" 'Nothing was changed. Inspect the owning process before removing a stale lock.'
    fi
    if [ "${modify_path}" = 1 ]; then
        if [ ! -e "${HOME}/.cli-installer-path.lock" ] && [ ! -h "${HOME}/.cli-installer-path.lock" ]; then :; else
            die "PATH lock exists: ${HOME}/.cli-installer-path.lock" 'Nothing was changed. Inspect the owning process before removing a stale lock.'
        fi
    fi
    inspect_state
}
inspect_state() {
    managed=0; binary_present=0; old_version=''; record_count=0; created_dir=0; unmanaged=0
    for retained in "${install_dir%/}/.${command_name}-stage."*; do
        [ "${retained}" != "${stage}" ] || continue
        if [ -n "${stage}" ] && [ -d "${stage}" ] && [ -d "${retained}" ]; then
            retained_identity=$(stat -L "${identity_option}" '%d:%i' "${retained}") || die "Could not identify staging path: ${retained}"
            stage_identity=$(stat -L "${identity_option}" '%d:%i' "${stage}") || die "Could not identify active staging: ${stage}"
            [ "${retained_identity}" != "${stage_identity}" ] || continue
        fi
        if [ -e "${retained}" ] || [ -h "${retained}" ]; then
            die "Incomplete prior transaction requires inspection: ${retained}" 'Nothing was changed. Preserve previous-binary/previous-state and inspect the recovery files before retrying.'
        fi
    done
    if [ -e "${state}" ] || [ -h "${state}" ]; then
        validate_state; managed=1
    elif [ -e "${binary}" ] || [ -h "${binary}" ]; then
        if [ -f "${binary}" ] && [ ! -h "${binary}" ]; then :; else
            die "Target is not an ordinary file: ${binary}" 'Nothing was changed. Inspect the target; replacement does not bypass this protection.'
        fi
        if [ -r "${binary}" ] && [ -x "${binary}" ]; then :; else
            die "Receiptless target is not a readable executable: ${binary}" 'Nothing was changed. Check its permissions or choose another directory.'
        fi
        if [ "${action}" = uninstall ] || [ "${replace_unmanaged}" = 0 ]; then
            refuse_unmanaged
        fi
        unmanaged=1; binary_present=1
    fi
    fingerprint="${managed}:${unmanaged}:${binary_present}"
    if [ "${binary_present}" = 1 ]; then
        inventory_hash=$(sha256_of "${binary}")
        inventory_identity=$(stat -L "${identity_option}" '%d:%i' "${binary}") || die "Could not identify target: ${binary}"
        fingerprint="${fingerprint}:${inventory_hash}:${inventory_identity}"
    fi
    if [ "${managed}" = 1 ]; then
        inventory_state=$(sha256_of "${state}/state.sha256")
        fingerprint="${fingerprint}:${inventory_state}"
    fi
    if [ -d "${install_dir}" ]; then
        inventory_directory=$(cd "${install_dir}" && pwd -P) || die "Could not resolve install directory: ${install_dir}"
        inventory_identity=$(stat -L "${identity_option}" '%d:%i' "${install_dir}/.") || die "Could not identify install directory: ${install_dir}"
        if [ "${unmanaged}" = 1 ] && [ "${inventory_directory}" = / ]; then die 'Cannot preserve an unmanaged recovery backup outside a filesystem-root install directory.' 'Nothing was changed. Preserve the executable manually and choose a non-root directory.'; fi
        fingerprint="${fingerprint}:${inventory_directory}:${inventory_identity}"
    fi
}

# Receipt files are data, never shell code. Its manifest covers every owned file.
state_files() {
    printf '%s\n' format owner location binary.sha256 version created-dir path-count
    for legal in ${legal_files}; do printf 'legal/%s\n' "${legal}"; done
    record_index=0
    while [ "${record_index}" -lt "${record_count}" ]; do
        printf 'paths/%s.file\npaths/%s.line\npaths/%s.created\n' "${record_index}" "${record_index}" "${record_index}"
        record_index=$((record_index + 1))
    done
}
manifest_for() {
    state_files > "${tmp}/state-files"
    while IFS= read -r member; do
        digest=$(sha256_of "$1/${member}")
        printf '%s  %s\n' "${digest}" "${member}"
    done < "${tmp}/state-files"
}
seal_state() {
    manifest_for "$1" > "${tmp}/state-manifest"
    cp "${tmp}/state-manifest" "$1/state.sha256"
}
validate_state() {
    if [ -d "${state}" ] && [ ! -h "${state}" ]; then :; else
        die "Installer state is not an ordinary directory: ${state}" 'Nothing was changed. Preserve this state for inspection.'
    fi
    if [ -r "${state}" ] && [ -x "${state}" ]; then :; else
        die "Installer state is inaccessible: ${state}" 'Nothing was changed. Check read/search permissions before retrying.'
    fi
    for member in format owner location binary.sha256 version created-dir path-count state.sha256; do require_regular "${state}/${member}"; done
    format=$(cat "${state}/format"); owner=$(cat "${state}/owner"); location=$(cat "${state}/location")
    if [ "${format}" = 1 ] && [ "${owner}" = "${command_name}" ]; then :; else owned_error; fi
    case "${location}" in /*) : ;; *) owned_error ;; esac
    require_line_path "${location}"
    if [ "${location}" != "${install_dir}" ]; then
        requested_physical=$(cd "${install_dir}" && pwd -P) || owned_error
        quoted_location=$(shell_quote "${location}")
        recorded_physical=$(cd "${location}" && pwd -P) || die "Receipt location is missing or inaccessible: ${location}" "Retry with the recorded directory after inspection: --install-dir ${quoted_location}"
        [ "${requested_physical}" = "${recorded_physical}" ] || die "Receipt location differs from requested directory: ${location}" "No program was changed. Use the recorded directory: --install-dir ${quoted_location}"
    fi
    created_dir=$(cat "${state}/created-dir"); case "${created_dir}" in 0 | 1) ;; *) owned_error ;; esac
    record_count=$(cat "${state}/path-count")
    printf '%s\n' "${record_count}" | grep -Eq '^(0|[1-9][0-9]*)$' || owned_error
    [ "${record_count}" -le 100 ] || owned_error
    # Only the current file inventory is supported; receipts are never converted.
    state_files > "${tmp}/expected-files"
    printf '%s\n' state.sha256 >> "${tmp}/expected-files"
    LC_ALL=C sort "${tmp}/expected-files" > "${tmp}/expected-sorted"
    (cd "${state}" && find . -type f | sed 's|^./||' | LC_ALL=C sort) > "${tmp}/actual-files"
    cmp -s "${tmp}/expected-sorted" "${tmp}/actual-files" || die "Installer receipt file inventory does not match this installer: ${state}" 'Nothing was changed. Use the matching installer to remove that installation, or choose a fresh directory; this installer does not convert receipts.'
    for legal in ${legal_files}; do require_regular "${state}/legal/${legal}"; done
    # No links, devices or additional (even empty) user directories may be erased.
    (cd "${state}" && find . ! -type f ! -type d) > "${tmp}/nonregular"
    [ ! -s "${tmp}/nonregular" ] || owned_error
    (cd "${state}" && find . -type d | LC_ALL=C sort) > "${tmp}/actual-dirs"
    printf '%s\n' . ./legal ./paths > "${tmp}/expected-dirs"
    cmp -s "${tmp}/expected-dirs" "${tmp}/actual-dirs" || owned_error
    manifest_for "${state}" > "${tmp}/computed-manifest"
    cmp -s "${state}/state.sha256" "${tmp}/computed-manifest" || owned_error
    record_index=0
    while [ "${record_index}" -lt "${record_count}" ]; do
        path_created=$(cat "${state}/paths/${record_index}.created")
        case "${path_created}" in 0 | 1) : ;; *) owned_error ;; esac
        record_index=$((record_index + 1))
    done
    binary_present=0
    if [ -e "${binary}" ] || [ -h "${binary}" ]; then
        require_regular "${binary}"
        binary_present=1
    fi
    expected_binary=$(cat "${state}/binary.sha256")
    printf '%s\n' "${expected_binary}" | grep -Eq '^[0-9a-f]{64}$' || owned_error
    if [ "${binary_present}" = 1 ]; then
        actual_binary=$(sha256_of "${binary}")
        [ "${actual_binary}" = "${expected_binary}" ] || die "Managed executable was modified: ${binary}" "Receipt: ${state}. Nothing was changed. Preserve your changes or choose another directory; --replace-unmanaged does not apply."
    fi
    old_version=$(cat "${state}/version")
}

# A signal must not arrive between a successful rename and its rollback flag.
# The child ignores group signals only for this move; the parent remembers
# interruption, records the completed move, then honors the original exit code.
activation_move() {
    activation_phase=$1; activation_error=$2; shift 2
    case "${activation_phase}" in previous-state | binary | ownership | uninstall-binary | uninstall-state) : ;; *) die 'Invalid activation move.' ;; esac
    activation_signal=0; activation_status=0
    trap 'activation_signal=130' INT
    trap 'activation_signal=143' TERM HUP
    (trap '' INT TERM HUP; exec mv "$@") || activation_status=$?
    if [ "${activation_status}" = 0 ]; then
        case "${activation_phase}" in
            previous-state) state_moved=1 ;;
            binary) binary_switched=1 ;;
            ownership) state_switched=1 ;;
            uninstall-binary) uninstall_binary_moved=1 ;;
            uninstall-state) uninstall_state_moved=1 ;;
            *) die 'Invalid activation move.' ;;
        esac
    fi
    trap 'exit 130' INT
    trap 'exit 143' TERM HUP
    [ "${activation_signal}" = 0 ] || exit "${activation_signal}"
    [ "${activation_status}" = 0 ] || die "${activation_error}" "mv exited ${activation_status}."
}

# In-place writes preserve symlink/hardlink identity. Immutable private images
# survive any failed write; ownership is recorded before opening the live file.
# Explicit result variables let callers handle failures without conditional
# function calls disabling errexit inside these recovery operations.
profile_rewrite() {
    profile_rewrite_status=1
    rw_file=$1; rw_before=$2; rw_after=$3
    rw_dir=${stage}/rewrite.${rewrite_count}
    mkdir "${rw_dir}" || return 0
    printf '%s\n' "${rw_file}" > "${rw_dir}/file" || return 0
    stat -L "${identity_option}" '%d:%i' "${rw_file}" > "${rw_dir}/identity" || return 0
    cp "${rw_before}" "${rw_dir}/preimage" || return 0
    cp "${rw_after}" "${rw_dir}/postimage" || return 0
    rw_identity=$(stat -L "${identity_option}" '%d:%i' "${rw_file}") || return 0
    rw_expected_identity=$(cat "${rw_dir}/identity") || return 0
    [ "${rw_identity}" = "${rw_expected_identity}" ] && cmp -s "${rw_file}" "${rw_dir}/preimage" || return 0
    rewrite_count=$((rewrite_count + 1)); rewrite_owned=1
    rw_signal=0; rw_status=0
    trap 'rw_signal=130' INT || return 0
    trap 'rw_signal=143' TERM HUP || return 0
    (trap '' INT TERM HUP; exec cat "${rw_after}") > "${rw_file}" || rw_status=$?
    if [ "${rw_status}" = 0 ]; then
        rw_identity=$(stat -L "${identity_option}" '%d:%i' "${rw_file}") || rw_status=1
        rw_expected_identity=$(cat "${rw_dir}/identity") || rw_status=1
        if [ "${rw_identity}" != "${rw_expected_identity}" ] || ! cmp -s "${rw_file}" "${rw_dir}/postimage"; then rw_status=1; fi
    fi
    if [ "${cleaning}" = 1 ]; then trap '' INT TERM HUP || return 0
    else trap 'exit 130' INT || return 0; trap 'exit 143' TERM HUP || return 0; fi
    if [ "${rw_status}" != 0 ]; then
        profile_restore "${rw_dir}"
        [ "${profile_restore_status}" = 0 ] || rewrite_recovery=1
        if [ "${rw_signal}" != 0 ] && [ "${cleaning}" = 0 ]; then exit "${rw_signal}"; fi
        # Even a successful immediate restore leaves this operation uncommitted.
        return 0
    fi
    if [ "${rw_signal}" != 0 ] && [ "${cleaning}" = 0 ]; then exit "${rw_signal}"; fi
    profile_rewrite_status=0
    return 0
}

profile_restore() {
    profile_restore_status=1
    rr_dir=$1
    if [ -f "${rr_dir}/restored" ]; then profile_restore_status=0; return 0; fi
    # A failed restoration keeps its complete target and observed bytes intact;
    # cleanup must never reconstruct them from the now-partial live file.
    [ ! -f "${rr_dir}/restore-start" ] || return 0
    : > "${rr_dir}/restore-start" || return 0
    rr_file=$(cat "${rr_dir}/file") || return 0
    rr_identity=$(stat -L "${identity_option}" '%d:%i' "${rr_file}") || return 0
    printf '%s\n' "${rr_identity}" > "${rr_dir}/observed-identity" || return 0
    [ -f "${rr_file}" ] && [ -r "${rr_file}" ] || return 0
    cat "${rr_file}" > "${rr_dir}/observed" || return 0
    rr_expected_identity=$(cat "${rr_dir}/identity") || return 0
    [ "${rr_identity}" = "${rr_expected_identity}" ] && [ -w "${rr_file}" ] || return 0
    rr_size=$(wc -c < "${rr_dir}/observed") || return 0
    rr_size=${rr_size##* }
    rr_pre_size=$(wc -c < "${rr_dir}/preimage") || return 0
    rr_pre_size=${rr_pre_size##* }
    # A preimage already restored by another writer may have a later suffix.
    # Inverting the shorter postimage in that case would duplicate original bytes.
    if [ "${rr_size}" -ge "${rr_pre_size}" ]; then
        dd if="${rr_dir}/observed" of="${rr_dir}/check" bs=1 count="${rr_pre_size}" 2>/dev/null || return 0
        if cmp -s "${rr_dir}/check" "${rr_dir}/preimage"; then : > "${rr_dir}/restored" || return 0; profile_restore_status=0; return 0; fi
    fi
    rr_post_size=$(wc -c < "${rr_dir}/postimage") || return 0
    rr_post_size=${rr_post_size##* }
    # A completed write may have a later append: preserve that exact suffix.
    # A short write is attributable only when it is an exact postimage prefix.
    if [ "${rr_post_size}" = 0 ] && [ "${rr_size}" != 0 ]; then return 0; fi
    if [ "${rr_size}" -ge "${rr_post_size}" ]; then
        dd if="${rr_dir}/observed" of="${rr_dir}/check" bs=1 count="${rr_post_size}" 2>/dev/null || return 0
        cmp -s "${rr_dir}/check" "${rr_dir}/postimage" || return 0
        cp "${rr_dir}/preimage" "${rr_dir}/restore-image" || return 0
        dd if="${rr_dir}/observed" bs=1 skip="${rr_post_size}" conv=notrunc 2>/dev/null >> "${rr_dir}/restore-image" || return 0
    else
        dd if="${rr_dir}/postimage" of="${rr_dir}/check" bs=1 count="${rr_size}" 2>/dev/null || return 0
        cmp -s "${rr_dir}/check" "${rr_dir}/observed" || return 0
        cp "${rr_dir}/preimage" "${rr_dir}/restore-image" || return 0
    fi
    rr_identity=$(stat -L "${identity_option}" '%d:%i' "${rr_file}") || return 0
    rr_expected_identity=$(cat "${rr_dir}/identity") || return 0
    [ "${rr_identity}" = "${rr_expected_identity}" ] && cmp -s "${rr_file}" "${rr_dir}/observed" || return 0
    : > "${rr_dir}/restore-attempt" || return 0
    (trap '' INT TERM HUP; exec cat "${rr_dir}/restore-image") > "${rr_file}" || return 0
    cmp -s "${rr_file}" "${rr_dir}/restore-image" || return 0
    : > "${rr_dir}/restored" || return 0
    profile_restore_status=0
    return 0
}
restore_profile_rewrites() {
    rr_index=${rewrite_count}
    : > "${stage}/rewrite-conflicts"
    while [ "${rr_index}" -gt 0 ]; do
        rr_index=$((rr_index - 1))
        rr_restore_dir=${stage}/rewrite.${rr_index}
        rr_restore_identity=$(cat "${rr_restore_dir}/identity") || { rewrite_recovery=1; continue; }
        # Earlier entries may alias an unresolved later write. Do not make
        # another destructive recovery attempt against that same physical file.
        if grep -Fxq -- "${rr_restore_identity}" "${stage}/rewrite-conflicts"; then rewrite_recovery=1; continue; fi
        profile_restore "${rr_restore_dir}"
        if [ "${profile_restore_status}" != 0 ]; then
            rewrite_recovery=1
            printf '%s\n' "${rr_restore_identity}" >> "${stage}/rewrite-conflicts"
        fi
    done
    rewrite_owned=0
}

rollback() {
    rollback_failed=0
    if [ "${state_switched}" = 1 ] && [ -d "${state}" ]; then
        if mv "${state}" "${stage}/failed-state"; then new_state=${stage}/failed-state; else rollback_failed=1; new_state=${state}; fi
    fi
    rollback_paths
    if [ "${binary_switched}" = 1 ]; then
        if [ "${binary_present}" = 1 ]; then
            if [ "${unmanaged}" = 1 ]; then previous_binary=${recovery}/${command_name}; else previous_binary=${stage}/previous-binary; fi
            cp -p "${previous_binary}" "${binary}" || rollback_failed=1
        else rm -f "${binary}" || rollback_failed=1; fi
    fi
    if [ "${state_moved}" = 1 ]; then mv "${stage}/previous-state" "${state}" || rollback_failed=1; fi
    transition=0
    if [ -n "${path_preparation}" ]; then
        printf 'Startup preparation could not be completed; inspect %s and the retained transaction evidence.\n' "${path_preparation}" >&2
        rollback_failed=1
    fi
    if [ "${rollback_failed}" = 1 ]; then
        printf 'Recovery files remain at %s; restoration was incomplete. Inspect rewrite.*/file, identity, preimage, postimage and available observed/restore-image files before retrying.\n' "${stage}" >&2
        stage=''
    fi
}
cleanup() {
    cleaning=1; trap '' INT TERM HUP
    if [ "${uninstall_transition}" = 1 ]; then
        if [ "${uninstall_state_moved}" = 1 ]; then
            if [ ! -e "${state}" ] && [ ! -h "${state}" ]; then mv "${stage}/previous-state" "${state}" || rewrite_recovery=1
            else rewrite_recovery=1; fi
        fi
        if [ "${uninstall_binary_moved}" = 1 ]; then
            if [ ! -e "${binary}" ] && [ ! -h "${binary}" ]; then mv "${stage}/previous-binary" "${binary}" || rewrite_recovery=1
            else rewrite_recovery=1; fi
        fi
    fi
    if [ "${rewrite_owned}" = 1 ]; then restore_profile_rewrites; fi
    [ "${transition}" = 0 ] || rollback
    if [ "${rewrite_recovery}" = 1 ] && [ -n "${stage}" ]; then
        printf 'Transaction recovery evidence remains at %s. Inspect previous-binary/previous-state when present and rewrite.*/file, identity, preimage, postimage, observed and restore-image when available. Review these private files before retrying.\n' "${stage}" >&2
        stage=''
    fi
    if [ -n "${stage}" ] && ! rm -rf "${stage}"; then
        printf 'Could not remove transaction cleanup files at %s; inspect the retained owned artifacts before retrying.\n' "${stage}" >&2
    fi
    if [ "${path_locked}" = 1 ]; then rmdir "${path_lock}" || printf 'Could not remove PATH lock: %s\n' "${path_lock}" >&2; fi
    if [ "${locked}" = 1 ]; then rmdir "${lock}" || printf 'Could not remove installer lock: %s\n' "${lock}" >&2; fi
    [ -z "${tmp}" ] || rm -rf "${tmp}"
    if [ "${remove_created_dir:-0}" = 1 ] && [ "${install_dir}" != / ]; then rmdir "${install_dir}" 2>/dev/null || :; fi
}
acquire() {
    created_dir=0
    if [ ! -d "${install_dir}" ]; then
        [ "${action}" = install ] || return
        mkdir -p "${install_dir}" || die 'Could not create the install directory.'
        created_dir=1
    fi
    mkdir "${lock}" 2>/dev/null || die "Installation is already locked, or the lock path exists: ${lock}" 'No retries are performed; inspect any stale lock before removing it manually.'
    locked=1
    [ "${created_dir}" = 0 ] || remove_created_dir=1
    acquired_created=${created_dir}
    inspect_state
    # A new directory is an expected result of this transaction, not a preflight conflict.
    if [ "${acquired_created}" = 0 ]; then
        [ "${fingerprint}" = "${preflight}" ] || die "Installation changed since preflight: ${binary}" 'Nothing was replaced. Inspect the current target and retry.'
    else
        [ "${managed}:${unmanaged}:${binary_present}" = '0:0:0' ] || die "Installation appeared since preflight: ${binary}" 'Nothing was replaced. Inspect and retry.'
        created_dir=1
    fi
}

acquire_path_lock() {
    [ "${modify_path}" = 1 ] || return 0
    path_lock=${HOME}/.cli-installer-path.lock
    mkdir "${path_lock}" 2>/dev/null || die "Another installer is editing PATH, or this lock path exists: ${path_lock}" 'No retries are performed; inspect stale locks before removing them manually.'
    path_locked=1
}

detect_platform() {
    kernel=$(uname -s); machine=$(uname -m)
    case "${kernel}" in
        Darwin)
            macos_version=$(sw_vers -productVersion)
            case "${macos_version%%.*}" in '' | *[!0-9]*) die 'Could not read the macOS version.' ;; *) : ;; esac
            [ "${macos_version%%.*}" -ge "${minimum_macos}" ] || die "${product_name} requires macOS ${minimum_macos} or later."
            arm_hardware=$(sysctl -in hw.optional.arm64)
            [ "${arm_hardware}" != 1 ] || machine=arm64
            system="macOS ${macos_version} · ${machine}"
            ;;
        Linux) system="Linux · ${machine}" ;;
        MINGW* | MSYS* | CYGWIN* | Windows_NT) die 'Use install.ps1 from PowerShell on Windows.' ;;
        *) die "${product_name} has no release for ${kernel}." ;;
    esac
    case "${kernel}:${machine}" in
        Darwin:x86_64 | Darwin:amd64) platform_suffix=${macos_x64} ;;
        Darwin:arm64 | Darwin:aarch64) platform_suffix=${macos_arm64} ;;
        Linux:x86_64 | Linux:amd64) platform_suffix=${linux_x64} ;;
        Linux:arm64 | Linux:aarch64) platform_suffix=${linux_arm64} ;;
        *) die "Unsupported processor: ${machine}" ;;
    esac
    [ -n "${platform_suffix}" ] || die "${product_name} has no release for ${kernel} on ${machine}." 'Nothing was changed. Choose a supported system and processor.'
}
resolve_latest() {
    if command -v curl >/dev/null 2>&1; then
        effective=$(curl --proto '=https' --proto-redir '=https' --tlsv1.2 --fail --silent --show-error --location --connect-timeout 20 --output /dev/null --write-out '%{url_effective}' "${repo_url}/releases/latest") || die 'Could not resolve the latest stable release.'
    elif command -v wget >/dev/null 2>&1; then
        wget --https-only --timeout=20 --quiet --server-response --output-document=/dev/null "${repo_url}/releases/latest" 2> "${tmp}/redirects" || { cat "${tmp}/redirects" >&2; die 'Could not resolve the latest stable release.'; }
        effective=$(awk 'tolower($1)=="location:" { location=$2 } END {print location}' "${tmp}/redirects")
        case "${effective}" in /*) effective=https://github.com${effective} ;; *) : ;; esac
    else die 'Install curl or wget to download releases.'; fi
    case "${effective}" in "${repo_url}"/releases/tag/v*) version=${effective#"${repo_url}/releases/tag/v"} ;; *) die "Unexpected latest-release destination: ${effective}" ;; esac
    stable_version "${version}"
}
fetch() {
    case "${base}" in
        https://*)
            if command -v curl >/dev/null 2>&1; then curl --proto '=https' --proto-redir '=https' --tlsv1.2 --fail --silent --show-error --location --connect-timeout 20 --output "$2" "${base}/$1" || die "Could not download $1."
            elif command -v wget >/dev/null 2>&1; then wget --https-only --timeout=20 --quiet --output-document="$2" "${base}/$1" || die "Could not download $1."
            else die 'Install curl or wget to download releases.'; fi
            ;;
        /*) cp "${base}/$1" "$2" || die "Could not download $1." ;;
        *) die 'The release mirror must be HTTPS or an absolute local directory.' ;;
    esac
}
candidate_error() {
    for stream in out err; do
        if [ -s "${tmp}/version.${stream}" ]; then
            printf 'Candidate %s (first 4096 bytes):\n' "${stream}" >&2
            head -c 4096 "${tmp}/version.${stream}" >&2; printf '\n' >&2
        fi
    done
    die "$1"
}
reported_version() {
    if "$1" --version > "${tmp}/version.out" 2> "${tmp}/version.err"; then :
    else
        execution_status=$?
        candidate_error "The verified program could not run (exit ${execution_status}). Nothing was installed."
    fi
    identity_lines=$(awk 'END {print NR}' "${tmp}/version.out")
    [ "${identity_lines}" = 1 ] || candidate_error 'The verified program returned an invalid version identity. Nothing was installed.'
    identity=$(cat "${tmp}/version.out")
    carriage_return=$(printf '\r')
    case "${identity}" in *"${carriage_return}"*) candidate_error 'The verified program returned an invalid version identity. Nothing was installed.' ;; *) : ;; esac
    numeric='(0|[1-9][0-9]*)'; pre='(0|[1-9][0-9]*|[0-9]*[A-Za-z-][0-9A-Za-z-]*)'
    semver="${numeric}\.${numeric}\.${numeric}(-${pre}(\.${pre})*)?(\+[0-9A-Za-z-]+(\.[0-9A-Za-z-]+)*)?"
    printf '%s\n' "${identity}" | grep -Eq "^${command_name} ${semver}( \([^()]+\))?$" || candidate_error 'The verified program returned an invalid version identity. Nothing was installed.'
    identity=${identity#"${command_name} "}; printf '%s\n' "${identity%% *}"
}
download_and_verify() {
    if [ -n "${download_base}" ]; then
        base=${download_base%/}
        case "${base}" in
            https://*)
                authority=${base#https://}; authority=${authority%%/*}
                case "${authority}" in *@*) die 'HTTPS mirror bases must not contain credentials.' ;; *) : ;; esac
                case "${base}" in *'?'* | *'#'*) die 'HTTPS mirror bases must not contain queries or fragments.' ;; *) : ;; esac
                ;;
            /*) : ;;
            *) die 'The release mirror must be HTTPS or an absolute local directory.' ;;
        esac
        if [ "${archive_versioned}" = 1 ] && [ -z "${version}" ]; then die 'Versioned mirror archives require --version X.Y.Z.'; fi
    else
        [ -n "${version}" ] || resolve_latest
        base=${repo_url}/releases/download/v${version}
    fi
    asset=${archive_prefix}
    [ "${archive_versioned}" = 0 ] || asset=${asset}${version}${archive_separator}
    asset=${asset}${platform_suffix}.tar.gz
    fetch "${checksum_name}" "${tmp}/checksums"
    fetch "${asset}" "${tmp}/archive.tar.gz"
    expected=$(awk -v name="${asset}" '$2==name || $2=="*"name { if (NF!=2 || length($1)!=64 || $1 !~ /^[0-9a-fA-F]+$/) bad=1; count++; hash=tolower($1) } END { if (count!=1 || bad) exit 1; print hash }' "${tmp}/checksums") || die "The checksum file has no unique valid SHA-256 entry for ${asset}."
    actual=$(sha256_of "${tmp}/archive.tar.gz")
    [ "${actual}" = "${expected}" ] || die 'The download does not match its published SHA-256 checksum.' 'Nothing was installed.'
    step Downloaded "${asset}"; step Verified 'SHA-256 matches the release checksums'
    mkdir "${tmp}/extract"
    tar -tzf "${tmp}/archive.tar.gz" > "${tmp}/archive-list" || die 'The archive cannot be read.'
    member_prefix=''
    [ "${archive_rooted}" = 0 ] || member_prefix=${asset%.tar.gz}/
    for name in "${command_name}" ${legal_files}; do
        member=${member_prefix}${name}
        count=$(grep -Fxc -- "${member}" "${tmp}/archive-list" || :)
        [ "${count}" = 1 ] || die "The archive must contain exactly one ${member}."
        tar -tvzf "${tmp}/archive.tar.gz" "${member}" > "${tmp}/member-type" || die "Could not inspect ${member}."
        metadata_lines=$(awk 'END {print NR}' "${tmp}/member-type")
        [ "${metadata_lines}" = 1 ] || die "Ambiguous archive member: ${member}"
        first=$(cut -c 1 "${tmp}/member-type")
        [ "${first}" = - ] || die "Archive member is not a regular file: ${member}"
        # -O streams only this regular member; archive paths/links are never written.
        tar -xOzf "${tmp}/archive.tar.gz" "${member}" > "${tmp}/extract/${name}" || die "Could not extract ${member}."
        if [ "${name}" != "${command_name}" ] && [ ! -s "${tmp}/extract/${name}" ]; then die "Required legal material is empty: ${member}"; fi
    done
    candidate=${tmp}/extract/${command_name}; chmod 755 "${candidate}"
    new_version=$(reported_version "${candidate}")
    if [ -n "${version}" ] && [ "${new_version}" != "${version}" ]; then die "The downloaded program reports version ${new_version}, not ${version}."; fi
    step Tested "${command_name} ${new_version} runs on this system"
}

posix_line() {
    quoted=$(shell_quote "${install_dir}")
    dollar='$'
    printf 'case ":%s{PATH}:" in *:%s:*) ;; *) export PATH=%s:"%sPATH" ;; esac %s' "${dollar}" "${quoted}" "${quoted}" "${dollar}" "${marker}"
}
fish_line() {
    quoted=$(fish_quote "${install_dir}")
    dollar='$'
    printf 'if not contains -- %s %sPATH; set -gx PATH %s %sPATH; end %s' "${quoted}" "${dollar}" "${quoted}" "${dollar}" "${marker}"
}
startup_files() {
    case "${login_shell}" in
        */zsh) printf '%s\n' "${ZDOTDIR:-${HOME}}/.zshrc" ;;
        */bash)
            printf '%s\n' "${HOME}/.bashrc"
            for login in "${HOME}/.bash_profile" "${HOME}/.bash_login" "${HOME}/.profile"; do if [ -e "${login}" ]; then printf '%s\n' "${login}"; return; fi; done
            printf '%s\n' "${HOME}/.profile" ;;
        */fish) ;;
        *) printf '%s\n' "${HOME}/.profile" ;;
    esac
}
plan_path() {
    path_ready=0; : > "${stage}/path-actions"
    case "${install_dir}" in *:*) note PATH 'A colon cannot be represented in PATH; startup files left unchanged.'; modify_path=0 ;; *) : ;; esac
    case ":${PATH}:" in *":${install_dir}:"*) path_ready=1 ;; *) : ;; esac
    if [ "${modify_path}" = 0 ]; then return; fi
    if [ "${path_ready}" = 1 ]; then step PATH "already includes ${install_dir}"; return; fi
    startup_files > "${tmp}/startup-files"
    fish=${XDG_CONFIG_HOME:-${HOME}/.config}/fish/conf.d/${command_name}.fish
    if [ "${login_shell}" != "${login_shell%/fish}" ] || [ -d "${fish%/conf.d/*}" ]; then
        mkdir -p "${fish%/*}" || die 'Could not create the fish configuration directory.'
        printf '%s\n' "${fish}" >> "${tmp}/startup-files"
    fi
    while IFS= read -r file; do
        require_line_path "${file}"
        case "${file}" in /*) ;; *) die 'Startup paths must be absolute; use --no-modify-path.' ;; esac
        path_created=1; match_status=1
        if [ -e "${file}" ] || [ -h "${file}" ]; then
            [ -f "${file}" ] || die "Startup path is not a regular file: ${file}"
            if [ -r "${file}" ] && [ -w "${file}" ]; then :; else die "Startup file is not readable and writable: ${file}"; fi
            path_created=0
        else
            if [ -d "${file%/*}" ] && [ -w "${file%/*}" ]; then :; else die "Startup directory is not writable: ${file%/*}"; fi
        fi
        case "${file}" in "${fish}") line=$(fish_line) ;; *) line=$(posix_line) ;; esac
        if [ "${path_created}" = 0 ]; then
            match_status=0; grep -Fxq -- "${line}" "${file}" || match_status=$?
            case "${match_status}" in 0 | 1) : ;; *) die "Could not read startup file: ${file}" ;; esac
        fi
        known=0; record_index=0
        while [ "${record_index}" -lt "${record_count}" ]; do
            previous_file=$(cat "${new_state}/paths/${record_index}.file")
            previous_line=$(cat "${new_state}/paths/${record_index}.line")
            if [ "${previous_line}" = "${line}" ]; then
                if [ "${previous_file}" = "${file}" ]; then known=1
                elif [ -f "${previous_file}" ] && [ -f "${file}" ]; then
                    previous_identity=$(stat -L "${identity_option}" '%d:%i' "${previous_file}")
                    planned_identity=$(stat -L "${identity_option}" '%d:%i' "${file}")
                    if [ "${previous_identity}" = "${planned_identity}" ]; then known=1; fi
                fi
            fi
            record_index=$((record_index + 1))
        done
        if [ "${known}" = 1 ]; then
            if [ "${match_status}" = 0 ]; then path_ready=1
            else note PATH "Recorded startup entry was edited or removed; left alone: ${file}"; fi
        elif [ "${match_status}" = 0 ]; then
            path_ready=1
        else
            [ "${record_count}" -lt 100 ] || die 'Too many recorded startup paths; use --no-modify-path.'
            printf '%s\n' "${file}" > "${new_state}/paths/${record_count}.file"
            printf '%s\n' "${line}" > "${new_state}/paths/${record_count}.line"
            printf '%s\n' "${path_created}" > "${new_state}/paths/${record_count}.created"
            printf '%s\n' "${record_count}" >> "${stage}/path-actions"
            record_count=$((record_count + 1))
        fi
    done < "${tmp}/startup-files"
}
rollback_paths() {
    [ -f "${stage}/applied-paths" ] || return 0
    while IFS= read -r record_index; do
        file=$(cat "${new_state}/paths/${record_index}.file")
        path_created=$(cat "${new_state}/paths/${record_index}.created")
        if [ ! -e "${file}" ] && [ ! -h "${file}" ]; then continue; fi
        if [ ! -f "${file}" ] || [ ! -r "${file}" ] || [ ! -w "${file}" ]; then rollback_failed=1; continue; fi
        current_identity=$(stat -L "${identity_option}" '%d:%i' "${file}") || { rollback_failed=1; continue; }
        expected_identity=$(cat "${stage}/path-identity.${record_index}") || { rollback_failed=1; continue; }
        if [ "${current_identity}" != "${expected_identity}" ]; then rollback_failed=1; continue; fi
        before=${stage}/path-before.${record_index}; added=${stage}/path-added.${record_index}
        before_size=$(wc -c < "${before}" | tr -d ' '); added_size=$(wc -c < "${added}" | tr -d ' ')
        end=$((before_size + added_size))
        cat "${file}" > "${stage}/rollback-current.${record_index}" || { rollback_failed=1; continue; }
        cat "${before}" "${added}" > "${stage}/rollback-prefix.${record_index}"
        dd if="${stage}/rollback-current.${record_index}" of="${stage}/rollback-check.${record_index}" bs=1 count="${end}" 2>/dev/null || { rollback_failed=1; continue; }
        # Only this exact byte range can be attributed to our append. Changed
        # prefixes and partial writes require inspection, never whole-file restore.
        if ! cmp -s "${stage}/rollback-prefix.${record_index}" "${stage}/rollback-check.${record_index}"; then rollback_failed=1; continue; fi
        cat "${before}" > "${stage}/rollback-updated.${record_index}"
        dd if="${stage}/rollback-current.${record_index}" bs=1 skip="${end}" conv=notrunc 2>/dev/null >> "${stage}/rollback-updated.${record_index}" || { rollback_failed=1; continue; }
        profile_rewrite "${file}" "${stage}/rollback-current.${record_index}" "${stage}/rollback-updated.${record_index}"
        if [ "${profile_rewrite_status}" != 0 ]; then rollback_failed=1; continue; fi
        # A completed rollback rewrite is the desired final state, not an effect
        # to undo if another profile requires manual recovery.
        : > "${rw_dir}/restored"
        rewrite_owned=0
        if [ "${path_created}" = 1 ] && [ ! -s "${file}" ]; then rm -f "${file}" || rollback_failed=1; fi
    done < "${stage}/applied-paths"
}
append_owned_paths() {
    : > "${stage}/applied-paths"
    while IFS= read -r record_index; do
        file=$(cat "${new_state}/paths/${record_index}.file"); line=$(cat "${new_state}/paths/${record_index}.line")
        path_created=$(cat "${new_state}/paths/${record_index}.created")
        if [ "${path_created}" = 1 ]; then
            # A failed exclusive create never claims a concurrent user's file.
            (set -C; : > "${file}") || die "Startup file appeared or could not be created: ${file}" 'Installation activation was cancelled; concurrent content was preserved.'
            path_preparation=${file}
        else
            if [ -f "${file}" ] && [ -r "${file}" ] && [ -w "${file}" ]; then :; else
                die "Startup file became nonregular or inaccessible: ${file}"
            fi
            match_status=0; grep -Fxq -- "${line}" "${file}" || match_status=$?
            case "${match_status}" in
                0) die "Startup entry appeared since planning: ${file}" 'Installation activation was cancelled; concurrent content was preserved.' ;;
                1) : ;;
                *) die "Could not recheck startup file before PATH append: ${file}" 'Installation activation was cancelled; existing content was preserved.' ;;
            esac
        fi
        stat -L "${identity_option}" '%d:%i' "${file}" > "${stage}/path-identity.${record_index}"
        cat "${file}" > "${stage}/path-before.${record_index}" || die "Could not snapshot startup entry boundary: ${file}"
        last_byte_lines=1
        if [ -s "${file}" ]; then last_byte_lines=$(tail -c 1 "${file}" | wc -l | tr -d ' '); fi
        : > "${stage}/path-added.${record_index}"
        if [ "${last_byte_lines}" = 0 ]; then printf '\n' >> "${stage}/path-added.${record_index}"; fi
        printf '%s\n' "${line}" >> "${stage}/path-added.${record_index}"
        printf '%s\n' "${record_index}" >> "${stage}/applied-paths"
        path_preparation=''
        cmp -s "${file}" "${stage}/path-before.${record_index}" || die "Startup file changed before PATH append: ${file}"
        cat "${stage}/path-added.${record_index}" >> "${file}" || die "Could not update PATH in ${file}; inspect retained transaction evidence."
        path_ready=1; step PATH "added an owned startup entry in ${file}"
    done < "${stage}/path-actions"
}
# BSD stat defaults to link metadata. Paths are fixed product paths or a
# checked digest component, never an arbitrary receipt path.
legacy_mac_stat() { /usr/bin/stat -f "$1" "$2" 2>/dev/null; }
legacy_real_directory() { [ ! -L "$1" ] && [ -d "$1" ]; }
legacy_no_acl() {
    legacy_acl=$(/bin/ls -lde "$1" 2>/dev/null) || return 1
    case "${legacy_acl}" in *'
'*) return 1 ;; *) return 0 ;; esac
}
# Recognition predicates explicitly reject failed checks, including when tested
# by the advisory. They do not depend on errexit.
# shellcheck disable=SC2310
legacy_directory() {
    legacy_real_directory "$1" || return 1
    legacy_metadata=$(legacy_mac_stat '%u %Lp' "$1") || return 1
    [ "${legacy_metadata}" = '0 755' ] && legacy_no_acl "$1"
}
# shellcheck disable=SC2310
legacy_link() {
    [ -L "$1" ] || return 1
    legacy_metadata=$(legacy_mac_stat '%u %Lp' "$1") || return 1
    [ "${legacy_metadata}" = '0 755' ] && legacy_no_acl "$1" || return 1
    legacy_target=$(/usr/bin/readlink "$1" 2>/dev/null) || return 1
    [ "${legacy_target}" = "$2" ]
}
# shellcheck disable=SC2310
legacy_regular() {
    [ -f "$1" ] && [ ! -L "$1" ] || return 1
    legacy_metadata=$(legacy_mac_stat '%u %Lp %l' "$1") || return 1
    [ "${legacy_metadata}" = "0 $2 1" ] && legacy_no_acl "$1"
}
# shellcheck disable=SC2310
legacy_bounded() {
    legacy_size=$(legacy_mac_stat '%z' "$1") || return 1
    case "${legacy_size}" in '' | *[!0-9]*) return 1 ;; *) ;; esac
    [ "${#legacy_size}" -le 8 ] && [ "${legacy_size}" -le "$2" ]
}
legacy_hash() {
    legacy_digest=$(/usr/bin/shasum -a 256 < "$1" 2>/dev/null) || return 1
    printf '%s\n' "${legacy_digest%% *}"
}
# Missing or unverified evidence is handled by explicit predicate results.
# shellcheck disable=SC2310
legacy_system_advice() (
    legacy_prefix=$1
    legacy_root=${legacy_prefix}/libexec/pdfconcat
    legacy_command=${legacy_prefix}/bin/pdfconcat
    legacy_trusted=1
    # The old setup accepted real destination parents with administrator-
    # chosen ownership and modes. Their contents cannot authorize a command.
    for legacy_dir in "${legacy_prefix%/*}" "${legacy_prefix}" "${legacy_prefix}/bin" "${legacy_prefix}/libexec"; do
        legacy_real_directory "${legacy_dir}" || return 0
        legacy_directory "${legacy_dir}" || legacy_trusted=0
    done
    legacy_directory "${legacy_root}" && legacy_directory "${legacy_root}/releases" || return 0
    legacy_current=$(/usr/bin/readlink "${legacy_root}/current" 2>/dev/null) || return 0
    legacy_id=${legacy_current#releases/}
    [ "${legacy_current}" = "releases/${legacy_id}" ] && [ "${#legacy_id}" = 64 ] || return 0
    case "${legacy_id}" in *[!0-9a-f]*) return 0 ;; *) ;; esac
    legacy_link "${legacy_root}/current" "${legacy_current}" || return 0
    legacy_link "${legacy_command}" "${legacy_root}/current/payload/pdfconcat" || return 0

    legacy_generation=${legacy_root}/releases/${legacy_id}
    legacy_receipt=${legacy_generation}/receipt
    legacy_payload=${legacy_generation}/payload/pdfconcat
    legacy_guide=${repo_url}/blob/v0.6.2/docs/MACOS.md#uninstall-pdfconcat
    legacy_directory "${legacy_generation}" && legacy_directory "${legacy_generation}/payload" || return 0
    legacy_regular "${legacy_payload}" 755 || return 0
    legacy_regular "${legacy_receipt}" 644 && legacy_bounded "${legacy_receipt}" 131072 || return 0
    if [ "${legacy_trusted}" = 0 ]; then
        # Do not open files through a parent whose entries can be replaced.
        note 'System copy' "Retired managed PDFConcat candidate: ${legacy_command}"
        note 'Trust' 'A parent directory failed the ownership, permission or ACL checks. The receipt and executable could not be verified.'
        note 'Removal' "${legacy_guide}"
        return 0
    fi
    legacy_receipt_hash=$(legacy_hash "${legacy_receipt}") || return 0
    [ "${legacy_receipt_hash}" = "${legacy_id}" ] || return 0
    legacy_version=$(LC_ALL=C /usr/bin/awk '
        NR == 1 { if ($0 != "PDFCONCAT-MACOS-1") exit 1 }
        NR == 2 { if ($0 !~ /^version v(0|[1-9][0-9]{0,8})\.(0|[1-9][0-9]{0,8})\.(0|[1-9][0-9]{0,8})$/) exit 1; version = $2 }
        NR == 3 { if ($0 !~ /^architecture (arm64|amd64)$/) exit 1; print version; exit }
    ' "${legacy_receipt}") || return 0
    [ -n "${legacy_version}" ] || return 0
    legacy_payload_hash=$(LC_ALL=C /usr/bin/awk '
        NR > 3 && $NF == "payload/pdfconcat" {
            count++
            if ($0 == "f 755 " $3 " payload/pdfconcat" && length($3) == 64 && $3 ~ /^[0-9a-f]+$/) hash = $3
        }
        END { if (count != 1 || hash == "") exit 1; print hash }
    ' "${legacy_receipt}") || legacy_payload_hash=''
    legacy_actual=''
    if [ -n "${legacy_payload_hash}" ] && legacy_bounded "${legacy_payload}" 67108864; then
        legacy_actual=$(legacy_hash "${legacy_payload}") || legacy_actual=''
    fi
    if [ -z "${legacy_actual}" ] || [ "${legacy_actual}" != "${legacy_payload_hash}" ]; then
        note 'System copy' "Retired managed PDFConcat candidate: ${legacy_command}"
        if [ -n "${legacy_actual}" ]; then
            note 'Trust' "Receipt version ${legacy_version}. Executable bytes do not match the receipt."
        else
            note 'Trust' "Receipt version ${legacy_version}. The executable could not be verified against its receipt."
        fi
        note 'Removal' "${legacy_guide}"
        return 0
    fi
    note 'System copy' "Retired managed PDFConcat ${legacy_version}: ${legacy_command}"
    note 'Removal' "${legacy_guide}"

    case "${legacy_version}" in
        v0.6.1) legacy_expected=b57b51cc465ad1daaf121346a63e7ce9fac54c072fc7dc30885dd9a76d9a2565 ;;
        v0.6.2) legacy_expected=4cb122a8fdf38ffd2a8fdd5d66615d5f0ab12dbac42ca7bd27cdb28444711ab3 ;;
        *) return 0 ;;
    esac
    legacy_helper=${legacy_generation}/setup.sh
    legacy_regular "${legacy_helper}" 755 && legacy_bounded "${legacy_helper}" 131072 || return 0
    legacy_helper_hash=$(legacy_hash "${legacy_helper}") || return 0
    [ "${legacy_helper_hash}" = "${legacy_expected}" ] || return 0
    legacy_helper_count=$(/usr/bin/grep -Fxc "f 755 ${legacy_expected} setup.sh" "${legacy_receipt}") || return 0
    [ "${legacy_helper_count}" = 1 ] || return 0
    note 'Legacy setup' "Historical removal command: /bin/bash ${legacy_root}/current/setup.sh uninstall"
)
report_command_paths() {
    found=$(command -v "${command_name}" 2>/dev/null || true)
    if [ -n "${found}" ] && [ "${found}" != "${binary}" ]; then note PATH "The current terminal finds another ${command_name}: ${found}"; fi
    if [ "${identity_kernel}" = Darwin ] && [ "${command_name}" = pdfconcat ]; then
        legacy_system_advice "$1"
    fi
}

install() {
    acquire
    locked_inventory=${fingerprint}
    # Preserve the recorded spelling used by existing exact PATH records.
    if [ "${managed}" = 1 ]; then install_dir=${location}; fi
    acquire_path_lock
    stage=$(mktemp -d "${install_dir%/}/.${command_name}-stage.XXXXXX") || die 'Could not create private staging beside the destination.'
    new_state=${stage}/new-state; mkdir -p "${new_state}/legal" "${new_state}/paths"
    if [ "${managed}" = 1 ]; then
        if [ "${binary_present}" = 1 ]; then cp -p "${binary}" "${stage}/previous-binary"; fi
        record_index=0
        while [ "${record_index}" -lt "${record_count}" ]; do
            cp "${state}/paths/${record_index}.file" "${new_state}/paths/${record_index}.file"
            cp "${state}/paths/${record_index}.line" "${new_state}/paths/${record_index}.line"
            cp "${state}/paths/${record_index}.created" "${new_state}/paths/${record_index}.created"
            record_index=$((record_index + 1))
        done
    fi
    if [ "${unmanaged}" = 1 ]; then
        note Replacing "Only ${binary} will be replaced; existing PATH entries remain unowned."
        recovery=$(mktemp -d "${inventory_directory%/*}/.${command_name}-recovery.XXXXXX") || die 'Could not exclusively create a recovery directory outside the install directory.'
        cp -p "${binary}" "${recovery}/${command_name}" || die "Could not preserve the unmanaged executable: ${binary}"
        note Recovery "Original executable retained at ${recovery}/${command_name}; uninstall will not delete it."
    fi
    cp "${candidate}" "${stage}/new-binary"; chmod 755 "${stage}/new-binary"
    for legal in ${legal_files}; do cp "${tmp}/extract/${legal}" "${new_state}/legal/${legal}"; done
    printf '1\n' > "${new_state}/format"; printf '%s\n' "${command_name}" > "${new_state}/owner"
    printf '%s\n' "${install_dir}" > "${new_state}/location"; printf '%s\n' "${new_version}" > "${new_state}/version"
    sha256_of "${stage}/new-binary" > "${new_state}/binary.sha256"
    printf '%s\n' "${created_dir}" > "${new_state}/created-dir"
    plan_path
    printf '%s\n' "${record_count}" > "${new_state}/path-count"; seal_state "${new_state}"
    (
        inspect_state
        [ "${fingerprint}" = "${locked_inventory}" ] || die "Installation changed before activation: ${binary}" 'Nothing was replaced. Inspect the current target and retry.'
    )
    transition=1
    append_owned_paths
    if [ "${managed}" = 1 ]; then activation_move previous-state "Could not stage previous installer state: ${state}" "${state}" "${stage}/previous-state"; fi
    activation_move binary "Could not activate the program: ${binary}" -f "${stage}/new-binary" "${binary}"
    activation_move ownership "Could not activate installer ownership: ${state}" "${new_state}" "${state}"
    transition=0
    remove_created_dir=0
    if [ -n "${old_version}" ] && [ "${old_version}" = "${new_version}" ]; then step Installed "${binary} (reinstalled ${new_version})"
    elif [ -n "${old_version}" ]; then step Installed "${binary} (updated from ${old_version})"
    else step Installed "${binary}"; fi
    if [ "${modify_path}" = 0 ]; then note PATH 'persistent and current-session PATH left unchanged'
    elif [ "${path_ready}" = 0 ]; then note PATH 'No startup entry was added; use the full path or configure PATH manually.'; fi
    printf '\n%s %s is installed.\n' "${product_name}" "${new_version}"
    report_command_paths /usr/local
    case "${login_shell}" in
        */fish) quoted_binary=$(fish_quote "${binary}"); quoted_install=$(fish_quote "${install_dir}") ;;
        *) quoted_binary=$(shell_quote "${binary}"); quoted_install=$(shell_quote "${install_dir}") ;;
    esac
    printf 'Open a new terminal after a PATH change. Run: %s --help\n' "${quoted_binary}"
    printf 'To uninstall, save/reuse this reviewed installer as install.sh, then run:\n'
    printf 'sh install.sh --uninstall --install-dir %s\n' "${quoted_install}"
    printf 'Guide: %s\n' "${docs_url}"
}
remove_owned_paths() {
    : > "${tmp}/path-removals"
    record_index=0
    # Prepare all reads first. An unreadable later startup file must not turn
    # into partial PATH cleanup followed by a falsely successful uninstall.
    while [ "${record_index}" -lt "${record_count}" ]; do
        file=$(cat "${state}/paths/${record_index}.file"); line=$(cat "${state}/paths/${record_index}.line")
        if [ -e "${file}" ] || [ -h "${file}" ]; then
            if [ -f "${file}" ] && [ -r "${file}" ]; then :; else die "Could not read startup file: ${file}"; fi
            cat "${file}" > "${tmp}/path-original.${record_index}" || die "Could not read startup file: ${file}"
            match_status=0; grep -Fxq -- "${line}" "${tmp}/path-original.${record_index}" || match_status=$?
            case "${match_status}" in
                0)
                    filter_status=0; grep -vFx -- "${line}" "${tmp}/path-original.${record_index}" > "${tmp}/path-filtered.${record_index}" || filter_status=$?
                    [ "${filter_status}" -le 1 ] || die "Could not read startup file: ${file}"
                    printf '%s\n' "${record_index}" >> "${tmp}/path-removals"
                    ;;
                1) : ;;
                *) die "Could not read startup file: ${file}" ;;
            esac
        fi
        record_index=$((record_index + 1))
    done
    while IFS= read -r record_index; do
        file=$(cat "${state}/paths/${record_index}.file")
        line=$(cat "${state}/paths/${record_index}.line")
        path_created=$(cat "${state}/paths/${record_index}.created")
        # Earlier entries can alias this same physical file. Read current bytes
        # rather than restoring a stale prepared image or reintroducing lines.
        if [ ! -e "${file}" ] && [ ! -h "${file}" ]; then continue; fi
        if [ -f "${file}" ] && [ -r "${file}" ]; then :; else die "Could not read startup file: ${file}"; fi
        cat "${file}" > "${stage}/path-current.${record_index}" || die "Could not read startup file: ${file}"
        match_status=0; grep -Fxq -- "${line}" "${stage}/path-current.${record_index}" || match_status=$?
        case "${match_status}" in 0) : ;; 1) continue ;; *) die "Could not read startup file: ${file}" ;; esac
        filter_status=0; grep -vFx -- "${line}" "${stage}/path-current.${record_index}" > "${stage}/path-updated.${record_index}" || filter_status=$?
        [ "${filter_status}" -le 1 ] || die "Could not read startup file: ${file}"
        cmp -s "${file}" "${stage}/path-current.${record_index}" || die "Startup file changed or became unreadable: ${file}"
        profile_rewrite "${file}" "${stage}/path-current.${record_index}" "${stage}/path-updated.${record_index}"
        [ "${profile_rewrite_status}" = 0 ] || die "Could not remove owned PATH entry: ${file}"
        printf '%s\n' "${path_created}" > "${rw_dir}/created"
        step PATH "removed this installation's exact entry from ${file}"
    done < "${tmp}/path-removals"
}
uninstall() {
    inventory; preflight=${fingerprint}
    if [ ! -d "${install_dir}" ]; then printf '%s is not installed in %s. Nothing was changed.\n' "${product_name}" "${install_dir}"; return; fi
    acquire
    acquire_path_lock
    if [ "${managed}" = 0 ]; then printf '%s is not installed in %s. Nothing was changed.\n' "${product_name}" "${install_dir}"; return; fi
    install_dir=${location}
    binary=${install_dir%/}/${command_name}
    state=${install_dir%/}/.${command_name}-install
    lock=${install_dir%/}/.${command_name}-install.lock
    shared=0
    for entry in "${install_dir}"/* "${install_dir}"/.[!.]* "${install_dir}"/..?*; do
        [ -e "${entry}" ] || [ -h "${entry}" ] || continue
        case "${entry}" in "${binary}" | "${state}" | "${lock}") ;; *) shared=1 ;; esac
    done
    stage=$(mktemp -d "${install_dir%/}/.${command_name}-stage.XXXXXX") || die 'Could not create private staging beside the destination.'
    if [ "${modify_path}" = 0 ]; then note PATH 'left unchanged by --no-modify-path; remove owned startup entries manually if needed'
    elif [ "${shared}" = 1 ]; then note PATH 'left unchanged because other files share this directory'
    else remove_owned_paths; fi
    uninstall_transition=1
    if [ "${binary_present}" = 1 ]; then activation_move uninstall-binary "Could not stage the owned executable for removal: ${binary}" "${binary}" "${stage}/previous-binary"; fi
    activation_move uninstall-state "Could not stage installer ownership for removal: ${state}" "${state}" "${stage}/previous-state"
    # One assignment command is the commit point for files and PATH together.
    uninstall_transition=0 rewrite_owned=0
    # Empty installer-created fish fragments are removed only after PATH commits.
    record_index=0
    while [ "${record_index}" -lt "${rewrite_count}" ]; do
        rw_remove=${stage}/rewrite.${record_index}
        file=$(cat "${rw_remove}/file")
        path_created=$(cat "${rw_remove}/created")
        case "${file}" in
            */fish/conf.d/"${command_name}".fish)
                if [ "${path_created}" = 1 ] && [ -f "${file}" ] && [ ! -h "${file}" ] && [ ! -s "${file}" ]; then
                    current_identity=$(stat -L "${identity_option}" '%d:%i' "${file}")
                    removal_identity=$(cat "${rw_remove}/identity")
                    if [ "${current_identity}" = "${removal_identity}" ]; then rm -f "${file}"; fi
                fi ;;
            *) : ;;
        esac
        record_index=$((record_index + 1))
    done
    remove_created_dir=${created_dir}
    printf '%s has been uninstalled. Your documents and unrelated files were not touched.\n' "${product_name}"
}

main() {
    validate_settings
    identity_kernel=$(uname -s)
    case "${identity_kernel}" in Darwin) identity_option=-f ;; *) identity_option=-c ;; esac
    parse_args "$@"
    tmp=$(mktemp -d "${TMPDIR:-/tmp}/${command_name}-install.XXXXXX") || die 'Could not create a temporary directory.'
    trap cleanup EXIT
    trap 'exit 130' INT
    trap 'exit 143' TERM HUP
    if [ "${action}" = uninstall ]; then uninstall; return; fi
    printf '\n%s installer\n\n' "${product_name}"
    detect_platform; step System "${system}"
    inventory; preflight=${fingerprint}
    if [ "${unmanaged}" = 1 ]; then note Replacing "Explicit replacement selected for ${binary}; the original will be retained outside the install directory."; fi
    download_and_verify
    install
}
main "$@"
