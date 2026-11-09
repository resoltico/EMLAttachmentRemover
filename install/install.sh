#!/bin/sh
# SPDX-License-Identifier: MPL-2.0
# Self-contained user installer. Shared engine; only this literal block differs.
# BEGIN PRODUCT SETTINGS
product_name='EML Attachment Remover'
command_name='emlattachmentremover'
repo_url='https://github.com/resoltico/EMLAttachmentRemover'
env_prefix='EMLATTACHMENTREMOVER'
archive_prefix='emlattachmentremover-'
archive_separator='-'
archive_versioned=1
macos_token='macos'
x64_token='x86_64'
arm64_token='aarch64'
checksum_name='SHA256SUMS'
minimum_macos=15
legal_files='LICENSE THIRD-PARTY-NOTICES.txt'
# END PRODUCT SETTINGS

set -eu
umask 077
docs_url=${repo_url}/blob/main/docs/INSTALL.md
marker="# added by the ${product_name} installer"
tmp='' stage='' locked=0 transition=0 binary_switched=0 state_moved=0 state_switched=0 path_locked=0

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
Usage: install.sh [--version X.Y.Z] [--install-dir DIR] [--no-modify-path] [--uninstall]
Defaults: \$XDG_BIN_HOME or ~/.local/bin; no administrator access required.
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
    for token in "${archive_prefix}" "${archive_separator}" "${macos_token}" "${x64_token}" "${arm64_token}" "${checksum_name}"; do
        case "${token}" in '' | *[!A-Za-z0-9_.-]* | . | ..) die 'Invalid installer archive setting.' ;; *) : ;; esac
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
    download_base=$(setting DOWNLOAD_BASE); modify_path=1; action=install
    no_modify=$(setting NO_MODIFY_PATH)
    [ "${no_modify}" != 1 ] || modify_path=0
    while [ "$#" -gt 0 ]; do
        case "$1" in
            --version) if [ "$#" -ge 2 ] && [ -n "$2" ]; then :; else die '--version needs a nonempty value.'; fi; version=$2; version_requested=1; shift ;;
            --version=*) version=${1#*=}; [ -n "${version}" ] || die '--version needs a nonempty value.'; version_requested=1 ;;
            --install-dir) if [ "$#" -ge 2 ] && [ -n "$2" ]; then :; else die '--install-dir needs a nonempty directory.'; fi; install_dir=$2; shift ;;
            --install-dir=*) install_dir=${1#*=}; [ -n "${install_dir}" ] || die '--install-dir needs a nonempty directory.' ;;
            --no-modify-path) modify_path=0 ;;
            --uninstall) action=uninstall ;;
            -h | --help) usage; exit 0 ;;
            *) die "Unknown option: $1" 'Run with --help for options.' ;;
        esac
        shift
    done
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
    if command -v sha256sum >/dev/null 2>&1; then digest_output=$(sha256sum < "$1") || die 'Could not read or hash the selected file.'
    elif command -v shasum >/dev/null 2>&1; then digest_output=$(shasum -a 256 < "$1") || die 'Could not read or hash the selected file.'
    elif command -v openssl >/dev/null 2>&1; then digest_output=$(openssl dgst -sha256 < "$1") || die 'Could not read or hash the selected file.'
    else die 'A SHA-256 tool (sha256sum, shasum or openssl) is required.'; fi
    printf '%s\n' "${digest_output}" | awk '{ if (length($1)==64) print tolower($1); else print tolower($NF) }'
}
require_regular() { if [ -f "$1" ] && [ ! -h "$1" ]; then :; else owned_error; fi; }
owned_error() { die "Existing files in ${install_dir} have missing, changed or invalid installer ownership; nothing was replaced or removed." 'Review and remove the old/manual installation yourself, or use a new directory.'; }

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
    if [ -d "${state}" ] && [ ! -h "${state}" ]; then :; else owned_error; fi
    for member in format owner location binary.sha256 version created-dir path-count state.sha256; do require_regular "${state}/${member}"; done
    format=$(cat "${state}/format"); owner=$(cat "${state}/owner"); location=$(cat "${state}/location")
    if [ "${format}" = 1 ] && [ "${owner}" = "${command_name}" ] && [ "${location}" = "${install_dir}" ]; then :; else owned_error; fi
    created_dir=$(cat "${state}/created-dir"); case "${created_dir}" in 0 | 1) ;; *) owned_error ;; esac
    record_count=$(cat "${state}/path-count")
    printf '%s\n' "${record_count}" | grep -Eq '^(0|[1-9][0-9]*)$' || owned_error
    [ "${record_count}" -le 100 ] || owned_error
    state_files > "${tmp}/expected-files"
    printf '%s\n' state.sha256 >> "${tmp}/expected-files"
    LC_ALL=C sort "${tmp}/expected-files" > "${tmp}/expected-sorted"
    (cd "${state}" && find . -type f | sed 's|^./||' | LC_ALL=C sort) > "${tmp}/actual-files"
    cmp -s "${tmp}/expected-sorted" "${tmp}/actual-files" || owned_error
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
    require_regular "${binary}"
    expected_binary=$(cat "${state}/binary.sha256")
    actual_binary=$(sha256_of "${binary}")
    [ "${actual_binary}" = "${expected_binary}" ] || owned_error
    old_version=$(cat "${state}/version")
}

rollback() {
    rollback_failed=0
    if [ "${state_switched}" = 1 ] && [ -d "${state}" ]; then mv "${state}" "${stage}/failed-state" || rollback_failed=1; fi
    if [ "${binary_switched}" = 1 ]; then
        if [ "${managed}" = 1 ]; then mv -f "${stage}/previous-binary" "${binary}" || rollback_failed=1
        else rm -f "${binary}" || rollback_failed=1; fi
    fi
    if [ "${state_moved}" = 1 ]; then mv "${stage}/previous-state" "${state}" || rollback_failed=1; fi
    transition=0
    if [ "${rollback_failed}" = 1 ]; then
        printf 'Recovery files remain at %s; restoration was incomplete.\n' "${stage}" >&2
        stage=''
    fi
}
cleanup() {
    [ "${transition}" = 0 ] || rollback
    [ -z "${stage}" ] || rm -rf "${stage}"
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
    managed=0; old_version=''; record_count=0
    if [ -e "${binary}" ] || [ -h "${binary}" ] || [ -e "${state}" ] || [ -h "${state}" ]; then
        validate_state
        managed=1
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
            os=${macos_token}; macos_version=$(sw_vers -productVersion)
            case "${macos_version%%.*}" in '' | *[!0-9]*) die 'Could not read the macOS version.' ;; *) : ;; esac
            [ "${macos_version%%.*}" -ge "${minimum_macos}" ] || die "${product_name} requires macOS ${minimum_macos} or later."
            arm_hardware=$(sysctl -in hw.optional.arm64)
            [ "${arm_hardware}" != 1 ] || machine=arm64
            system="macOS ${macos_version} · ${machine}"
            ;;
        Linux) os=linux; system="Linux · ${machine}" ;;
        MINGW* | MSYS* | CYGWIN* | Windows_NT) die 'Use install.ps1 from PowerShell on Windows.' ;;
        *) die "${product_name} has no release for ${kernel}." ;;
    esac
    case "${machine}" in x86_64 | amd64) arch=${x64_token} ;; arm64 | aarch64) arch=${arm64_token} ;; *) die "Unsupported processor: ${machine}" ;; esac
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
reported_version() {
    if "$1" --version > "${tmp}/version.out" 2> "${tmp}/version.err"; then :
    else
        if [ -s "${tmp}/version.err" ]; then
            printf 'Program diagnostic:\n' >&2
            head -c 4096 "${tmp}/version.err" >&2
            printf '\n' >&2
        fi
        die 'The verified program could not run. Nothing was installed.'
    fi
    identity_lines=$(awk 'END {print NR}' "${tmp}/version.out")
    [ "${identity_lines}" = 1 ] || die 'The program returned an invalid version identity.'
    identity=$(cat "${tmp}/version.out")
    carriage_return=$(printf '\r')
    case "${identity}" in *"${carriage_return}"*) die 'The program returned an invalid version identity.' ;; *) : ;; esac
    numeric='(0|[1-9][0-9]*)'; pre='(0|[1-9][0-9]*|[0-9]*[A-Za-z-][0-9A-Za-z-]*)'
    semver="${numeric}\.${numeric}\.${numeric}(-${pre}(\.${pre})*)?(\+[0-9A-Za-z-]+(\.[0-9A-Za-z-]+)*)?"
    printf '%s\n' "${identity}" | grep -Eq "^${command_name} ${semver}( \([^()]+\))?$" || die 'The program returned an invalid version identity.'
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
    asset=${asset}${os}${archive_separator}${arch}.tar.gz
    fetch "${checksum_name}" "${tmp}/checksums"
    fetch "${asset}" "${tmp}/archive.tar.gz"
    expected=$(awk -v name="${asset}" '$2==name || $2=="*"name { if (NF!=2 || length($1)!=64 || $1 !~ /^[0-9a-fA-F]+$/) bad=1; count++; hash=tolower($1) } END { if (count!=1 || bad) exit 1; print hash }' "${tmp}/checksums") || die "The checksum file has no unique valid SHA-256 entry for ${asset}."
    actual=$(sha256_of "${tmp}/archive.tar.gz")
    [ "${actual}" = "${expected}" ] || die 'The download does not match its published SHA-256 checksum.' 'Nothing was installed.'
    step Downloaded "${asset}"; step Verified 'SHA-256 matches the release checksums'
    mkdir "${tmp}/extract"
    tar -tzf "${tmp}/archive.tar.gz" > "${tmp}/archive-list" || die 'The archive cannot be read.'
    for member in "${command_name}" ${legal_files}; do
        count=$(grep -Fxc -- "${member}" "${tmp}/archive-list" || :)
        [ "${count}" = 1 ] || die "The archive must contain exactly one ${member}."
        tar -tvzf "${tmp}/archive.tar.gz" "${member}" > "${tmp}/member-type" || die "Could not inspect ${member}."
        metadata_lines=$(awk 'END {print NR}' "${tmp}/member-type")
        [ "${metadata_lines}" = 1 ] || die "Ambiguous archive member: ${member}"
        first=$(cut -c 1 "${tmp}/member-type")
        [ "${first}" = - ] || die "Archive member is not a regular file: ${member}"
        # -O streams only this regular member; archive paths/links are never written.
        tar -xOzf "${tmp}/archive.tar.gz" "${member}" > "${tmp}/extract/${member}" || die "Could not extract ${member}."
        if [ "${member}" != "${command_name}" ] && [ ! -s "${tmp}/extract/${member}" ]; then die "Required legal material is empty: ${member}"; fi
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
            if [ "${previous_file}" = "${file}" ] && [ "${previous_line}" = "${line}" ]; then known=1; fi
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
append_owned_paths() {
    while IFS= read -r record_index; do
        file=$(cat "${state}/paths/${record_index}.file"); line=$(cat "${state}/paths/${record_index}.line")
        path_created=$(cat "${state}/paths/${record_index}.created")
        if [ "${path_created}" = 1 ]; then
            # Exclusive creation makes the receipt's creation claim true at
            # the write boundary, rather than adopting a file created later.
            (set -C; printf '%s\n' "${line}" > "${file}") || die "Program installed, but startup creation failed: ${file}"
            path_ready=1; step PATH "created an owned startup entry in ${file}"
            continue
        fi
        last_byte_lines=1
        if [ -s "${file}" ]; then last_byte_lines=$(tail -c 1 "${file}" | wc -l | tr -d ' '); fi
        if [ "${last_byte_lines}" = 0 ]; then
            printf '\n' >> "${file}" || die "Program installed, but could not update PATH in ${file}."
        fi
        printf '%s\n' "${line}" >> "${file}" || die "Program installed, but could not update PATH in ${file}."
        path_ready=1; step PATH "added an owned startup entry in ${file}"
    done < "${stage}/path-actions"
}
install() {
    acquire
    acquire_path_lock
    stage=$(mktemp -d "${install_dir%/}/.${command_name}-stage.XXXXXX") || die 'Could not create private staging beside the destination.'
    new_state=${stage}/new-state; mkdir -p "${new_state}/legal" "${new_state}/paths"
    if [ "${managed}" = 1 ]; then
        cp -p "${binary}" "${stage}/previous-binary"
        record_index=0
        while [ "${record_index}" -lt "${record_count}" ]; do
            cp "${state}/paths/${record_index}.file" "${new_state}/paths/${record_index}.file"
            cp "${state}/paths/${record_index}.line" "${new_state}/paths/${record_index}.line"
            cp "${state}/paths/${record_index}.created" "${new_state}/paths/${record_index}.created"
            record_index=$((record_index + 1))
        done
    fi
    cp "${candidate}" "${stage}/new-binary"; chmod 755 "${stage}/new-binary"
    for legal in ${legal_files}; do cp "${tmp}/extract/${legal}" "${new_state}/legal/${legal}"; done
    printf '1\n' > "${new_state}/format"; printf '%s\n' "${command_name}" > "${new_state}/owner"
    printf '%s\n' "${install_dir}" > "${new_state}/location"; printf '%s\n' "${new_version}" > "${new_state}/version"
    sha256_of "${stage}/new-binary" > "${new_state}/binary.sha256"
    printf '%s\n' "${created_dir}" > "${new_state}/created-dir"
    plan_path
    printf '%s\n' "${record_count}" > "${new_state}/path-count"; seal_state "${new_state}"
    transition=1
    if [ "${managed}" = 1 ]; then state_moved=1; mv "${state}" "${stage}/previous-state" || die 'Could not stage the previous installer state.'; fi
    binary_switched=1; mv -f "${stage}/new-binary" "${binary}" || die 'Could not activate the program.'
    state_switched=1; mv "${new_state}" "${state}" || die 'Could not activate installer ownership.'
    transition=0
    remove_created_dir=0
    append_owned_paths
    if [ -n "${old_version}" ] && [ "${old_version}" = "${new_version}" ]; then step Installed "${binary} (reinstalled ${new_version})"
    elif [ -n "${old_version}" ]; then step Installed "${binary} (updated from ${old_version})"
    else step Installed "${binary}"; fi
    if [ "${modify_path}" = 0 ]; then note PATH 'persistent and current-session PATH left unchanged'
    elif [ "${path_ready}" = 0 ]; then note PATH 'No startup entry was added; use the full path or configure PATH manually.'; fi
    printf '\n%s %s is installed.\n' "${product_name}" "${new_version}"
    found=$(command -v "${command_name}" 2>/dev/null || true)
    if [ -n "${found}" ] && [ "${found}" != "${binary}" ]; then note PATH "The current terminal finds another ${command_name}: ${found}"; fi
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
        cat "${file}" > "${tmp}/path-current.${record_index}" || die "Could not read startup file: ${file}"
        match_status=0; grep -Fxq -- "${line}" "${tmp}/path-current.${record_index}" || match_status=$?
        case "${match_status}" in 0) : ;; 1) continue ;; *) die "Could not read startup file: ${file}" ;; esac
        filter_status=0; grep -vFx -- "${line}" "${tmp}/path-current.${record_index}" > "${tmp}/path-updated.${record_index}" || filter_status=$?
        [ "${filter_status}" -le 1 ] || die "Could not read startup file: ${file}"
        cmp -s "${file}" "${tmp}/path-current.${record_index}" || die "Startup file changed or became unreadable: ${file}"
        cat "${tmp}/path-updated.${record_index}" > "${file}" || die "Could not remove owned PATH entry: ${file}"
        case "${file}" in
            */fish/conf.d/"${command_name}".fish)
                if [ "${path_created}" = 1 ] && [ -f "${file}" ] && [ ! -h "${file}" ] && [ ! -s "${file}" ]; then rm -f "${file}"; fi
                ;;
            *) : ;;
        esac
        step PATH "removed this installation's exact entry from ${file}"
    done < "${tmp}/path-removals"
}
uninstall() {
    if [ ! -d "${install_dir}" ]; then printf '%s is not installed in %s. Nothing was changed.\n' "${product_name}" "${install_dir}"; return; fi
    acquire
    acquire_path_lock
    if [ "${managed}" = 0 ]; then printf '%s is not installed in %s. Nothing was changed.\n' "${product_name}" "${install_dir}"; return; fi
    shared=0
    for entry in "${install_dir}"/* "${install_dir}"/.[!.]* "${install_dir}"/..?*; do
        [ -e "${entry}" ] || [ -h "${entry}" ] || continue
        case "${entry}" in "${binary}" | "${state}" | "${lock}") ;; *) shared=1 ;; esac
    done
    if [ "${modify_path}" = 0 ]; then note PATH 'left unchanged by --no-modify-path; remove owned startup entries manually if needed'
    elif [ "${shared}" = 1 ]; then note PATH 'left unchanged because other files share this directory'
    else remove_owned_paths; fi
    rm -f "${binary}"; rm -rf "${state}"
    remove_created_dir=${created_dir}
    printf '%s has been uninstalled. Your documents and unrelated files were not touched.\n' "${product_name}"
}

main() {
    validate_settings
    parse_args "$@"
    tmp=$(mktemp -d "${TMPDIR:-/tmp}/${command_name}-install.XXXXXX") || die 'Could not create a temporary directory.'
    trap cleanup EXIT
    trap 'exit 130' INT
    trap 'exit 143' TERM HUP
    if [ "${action}" = uninstall ]; then uninstall; return; fi
    printf '\n%s installer\n\n' "${product_name}"
    detect_platform; step System "${system}"
    download_and_verify
    install
}
main "$@"
