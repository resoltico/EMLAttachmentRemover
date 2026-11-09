#!/bin/bash
# SPDX-License-Identifier: MPL-2.0
# Exact published-script qualification against real archives, in isolated homes.
set -euo pipefail

dist=${1:?usage: install_test.sh DIST_DIR [--verify-primary]}
case "${2:-}" in '' | --verify-primary) : ;; *) echo 'Unknown installer-test mode' >&2; exit 2 ;; esac
[[ "${dist}" == /* && -f "${dist}/install.sh" ]] || { echo 'Expected an absolute dist directory with staged install.sh' >&2; exit 2; }
installer=${dist}/install.sh
work=$(mktemp -d "${TMPDIR:-/tmp}/cli-installer-test.XXXXXX")
trap 'rm -rf "${work}"' EXIT
base_path=/usr/bin:/bin:/usr/sbin:/sbin
failures=0; case_number=0; last_exit=0
setting() {
    local literal
    literal=$(awk -v key="$1" '$0=="# BEGIN PRODUCT SETTINGS" {inside=1;next} $0=="# END PRODUCT SETTINGS" {inside=0} inside && index($0,key"=")==1 {print substr($0,length(key)+2)}' "${installer}")
    case "${literal}" in \'*\') literal=${literal#\'}; printf '%s' "${literal%\'}" ;; [0-9]*) printf '%s' "${literal}" ;; *) echo "Invalid literal setting: $1" >&2; exit 1 ;; esac
}
command_name=$(setting command_name); product_name=$(setting product_name); env_prefix=$(setting env_prefix)
archive_prefix=$(setting archive_prefix); archive_separator=$(setting archive_separator); archive_versioned=$(setting archive_versioned)
macos_token=$(setting macos_token); x64_token=$(setting x64_token); arm64_token=$(setting arm64_token)
checksum_name=$(setting checksum_name); legal_files=$(setting legal_files); repo_url=$(setting repo_url)
[[ -f "${dist}/${checksum_name}" ]] || { echo "Missing ${checksum_name}" >&2; exit 2; }
hash_file() {
    local output
    if command -v sha256sum >/dev/null; then
        if output=$(sha256sum < "$1"); then :; else return 1; fi
    else
        if output=$(shasum -a 256 < "$1"); then :; else return 1; fi
    fi
    printf '%s\n' "${output%% *}"
}
expected_installer=$(awk '$2=="install.sh" || $2=="*install.sh" {count++; hash=$1} END {if(count!=1 || length(hash)!=64 || hash !~ /^[0-9a-fA-F]+$/) exit 1; print tolower(hash)}' "${dist}/${checksum_name}")
actual_installer=$(hash_file "${installer}")
[[ "${expected_installer}" == "${actual_installer}" ]] || { echo 'Staged install.sh does not match the primary release checksum' >&2; exit 1; }

case "$(uname -s)" in Darwin) os=${macos_token}; hardware=$(sysctl -in hw.optional.arm64) ;; Linux) os=linux; hardware=0 ;; *) echo 'Unix installer tests require macOS or Linux' >&2; exit 2 ;; esac
machine=$(uname -m)
[[ "${hardware}" != 1 ]] || machine=arm64
case "${machine}" in x86_64 | amd64) arch=${x64_token} ;; arm64 | aarch64) arch=${arm64_token} ;; *) echo "Unsupported native processor: ${machine}" >&2; exit 2 ;; esac
if [[ "${archive_versioned}" == 1 ]]; then
    candidates=("${dist}/${archive_prefix}"*"${archive_separator}${os}${archive_separator}${arch}.tar.gz")
    [[ ${#candidates[@]} -eq 1 && -f "${candidates[0]}" ]] || { echo 'Expected one native versioned archive' >&2; exit 2; }
    native_archive=${candidates[0]}
else native_archive=${dist}/${archive_prefix}${os}${archive_separator}${arch}.tar.gz; fi
# Verify the primary executable archive before tar or any version invocation.
# Later tampered-mirror cases deliberately use their own checksum files.
native_asset=${native_archive##*/}
expected_archive=$(awk -v name="${native_asset}" '$2==name || $2=="*"name {count++; hash=$1; if(NF!=2 || length(hash)!=64 || hash !~ /^[0-9a-fA-F]+$/) bad=1} END {if(count!=1 || bad) exit 1; print tolower(hash)}' "${dist}/${checksum_name}") || { echo 'Primary archive has no unique valid checksum entry' >&2; exit 1; }
actual_archive=$(hash_file "${native_archive}")
[[ "${expected_archive}" == "${actual_archive}" ]] || { echo 'Primary native archive does not match its release checksum' >&2; exit 1; }
mkdir "${work}/bootstrap-home"
tar -xOzf "${native_archive}" "${command_name}" > "${work}/${command_name}"
chmod 755 "${work}/${command_name}"
version_output=$(env -i HOME="${work}/bootstrap-home" PATH="${base_path}" TMPDIR="${work}" "${work}/${command_name}" --version)
expected_version=${version_output#"${command_name} "}; expected_version=${expected_version%% *}
fixture_version=${expected_version}
echo "Release under test: ${version_output}"
[[ "${2:-}" != --verify-primary ]] || exit 0

fail() { printf '    FAIL: %s\n' "$*" >&2; failures=$((failures + 1)); }
assert() { local reason=$1; shift; if "$@"; then :; else fail "${reason}"; fi; }
expect_exit() { if [[ "${last_exit}" -ne "$1" ]]; then fail "$2 (exit ${last_exit}, wanted $1)"; cat "${work}/out" >&2; fi; }
expect_failure() { if [[ "${last_exit}" -eq 0 ]]; then fail "$1 unexpectedly succeeded"; cat "${work}/out" >&2; fi; }
begin() {
    case_number=$((case_number + 1)); fixture_home=${work}/home${case_number}; mkdir -p "${fixture_home}"
    unset fixture_path download_base fixture_zdotdir
    fixture_version=${expected_version}
    printf '\n--- %s\n' "$*"
}
run_installer() {
    local login_shell=$1; shift
    local mirror=${download_base-${dist}}
    local args=("$@") pin=0 arg
    for arg in ${args[@]+"${args[@]}"}; do case "${arg}" in --version | --version=*) pin=1 ;; *) : ;; esac; done
    if [[ "${archive_versioned}" == 1 && -n "${mirror}" && "${pin}" == 0 ]]; then args=(--version "${fixture_version}" "${args[@]}"); fi
    if env -i HOME="${fixture_home}" PATH="${fixture_path-${base_path}}" SHELL="${login_shell}" TMPDIR="${work}" ZDOTDIR="${fixture_zdotdir-${fixture_home}}" \
        "${env_prefix}_DOWNLOAD_BASE=${mirror}" \
        sh -s -- ${args[@]+"${args[@]}"} < "${installer}" > "${work}/out" 2>&1; then last_exit=0; else last_exit=$?; fi
}
marker_count() { if [[ -f "$1" ]]; then grep -cF "# added by the ${product_name} installer" "$1" || :; else printf '0\n'; fi; }
resolve_in() { env -i HOME="${fixture_home}" PATH="${base_path}" "$@" 2>/dev/null | tail -n 1; }
new_asset() { if [[ "${archive_versioned}" == 1 ]]; then printf '%s%s%s%s%s%s.tar.gz' "${archive_prefix}" "${fixture_version}" "${archive_separator}" "${os}" "${archive_separator}" "${arch}"; else printf '%s%s%s%s.tar.gz' "${archive_prefix}" "${os}" "${archive_separator}" "${arch}"; fi; }
make_release() {
    local target=$1 body=$2 asset legal
    mkdir -p "${target}/payload"
    printf '#!/bin/sh\n%s\n' "${body}" > "${target}/payload/${command_name}"
    chmod 755 "${target}/payload/${command_name}"
    local members=("${command_name}")
    for legal in ${legal_files}; do printf 'fixture legal material\n' > "${target}/payload/${legal}"; members+=("${legal}"); done
    asset=$(new_asset)
    tar -czf "${target}/${asset}" -C "${target}/payload" "${members[@]}"
    local digest
    digest=$(hash_file "${target}/${asset}")
    printf '%s  %s\n' "${digest}" "${asset}" > "${target}/${checksum_name}"
}

begin 'primary archive checksum blocks execution before version bootstrap'
primary_control=${work}/primary-control; mkdir -p "${primary_control}/dist" "${primary_control}/payload" "${primary_control}/tmp"
cp "${installer}" "${primary_control}/dist/install.sh"
cp "${dist}/${checksum_name}" "${primary_control}/dist/${checksum_name}"
cat > "${primary_control}/payload/${command_name}" <<'EOF'
#!/bin/sh
candidate_dir=${0%/*}
control_root=${candidate_dir%/*}
printf 'executed\n' > "${control_root}/bootstrap-executed"
EOF
printf "printf '%%s\\n' '%s 1.2.3'\n" "${command_name}" >> "${primary_control}/payload/${command_name}"
chmod 755 "${primary_control}/payload/${command_name}"
tar -czf "${primary_control}/dist/${native_asset}" -C "${primary_control}/payload" "${command_name}"
if TMPDIR="${primary_control}/tmp" bash "$0" "${primary_control}/dist" --verify-primary > "${work}/out" 2>&1; then last_exit=0; else last_exit=$?; fi
expect_failure 'tampered primary archive'
assert 'primary archive failure is checksum-specific' grep -q 'Primary native archive does not match' "${work}/out"
assert 'unchecked primary program was never run' test ! -e "${primary_control}/tmp/bootstrap-executed"
# Correct archive bytes still require one well-formed manifest row.
cp "${native_archive}" "${primary_control}/dist/${native_asset}"
cp "${dist}/${checksum_name}" "${primary_control}/dist/${checksum_name}"
awk -v name="${native_asset}" '$2==name || $2=="*"name {print}' "${dist}/${checksum_name}" >> "${primary_control}/dist/${checksum_name}"
if TMPDIR="${primary_control}/tmp" bash "$0" "${primary_control}/dist" --verify-primary > "${work}/out" 2>&1; then last_exit=0; else last_exit=$?; fi
expect_failure 'duplicate primary checksum entry'
cp "${dist}/${checksum_name}" "${primary_control}/dist/${checksum_name}"
awk -v name="${native_asset}" '$2==name || $2=="*"name {$1="not-a-sha256"} {print}' "${dist}/${checksum_name}" > "${primary_control}/dist/${checksum_name}"
if TMPDIR="${primary_control}/tmp" bash "$0" "${primary_control}/dist" --verify-primary > "${work}/out" 2>&1; then last_exit=0; else last_exit=$?; fi
expect_failure 'malformed primary checksum entry'

begin 'zsh: fresh install, update, native program and exact owned PATH cleanup'
if command -v zsh >/dev/null; then
    zsh_path=$(command -v zsh)
    printf 'export KEEP=1' > "${fixture_home}/.zshrc"
    run_installer /bin/zsh; expect_exit 0 'fresh install'
    assert 'binary installed' test -x "${fixture_home}/.local/bin/${command_name}"
    assert 'reported version' grep -Fq "${product_name} ${expected_version} is installed" "${work}/out"
    assert 'original last line preserved' grep -qx 'export KEEP=1' "${fixture_home}/.zshrc"
    count=$(marker_count "${fixture_home}/.zshrc"); assert 'one PATH line' test "${count}" = 1
    resolved=$(resolve_in "${zsh_path}" -i -c "command -v ${command_name}"); assert 'new zsh finds installed program' test "${resolved}" = "${fixture_home}/.local/bin/${command_name}"
    source_mail=${fixture_home}/source.eml
    printf 'Subject: Installer smoke\r\n\r\nBody retained\r\n' > "${source_mail}"
    env -i HOME="${fixture_home}" PATH="${base_path}" "${fixture_home}/.local/bin/${command_name}" -- "${source_mail}" > "${work}/cli-smoke"
    assert 'installed CLI creates the exact body copy' cmp -s "${source_mail}" "${fixture_home}/source.no-attachments.eml"
    assert 'installer retains LICENSE' test -s "${fixture_home}/.local/bin/.${command_name}-install/legal/LICENSE"
    assert 'installer retains third-party notices' test -s "${fixture_home}/.local/bin/.${command_name}-install/legal/THIRD-PARTY-NOTICES.txt"
    run_installer /bin/zsh; expect_exit 0 'reinstall'
    assert 'reinstall reported' grep -q reinstalled "${work}/out"
    count=$(marker_count "${fixture_home}/.zshrc"); assert 'still one owned line' test "${count}" = 1
    run_installer /bin/zsh --uninstall; expect_exit 0 'uninstall'
    assert 'binary removed' test ! -e "${fixture_home}/.local/bin/${command_name}"
    count=$(marker_count "${fixture_home}/.zshrc"); assert 'owned line removed' test "${count}" = 0
    assert 'user content retained' grep -qx 'export KEEP=1' "${fixture_home}/.zshrc"
    run_installer /bin/zsh --uninstall; expect_exit 0 'second uninstall'
    assert 'no-op reported' grep -q 'Nothing was changed' "${work}/out"
else echo 'zsh unavailable; shell-resolution check skipped'; fi

begin 'bash: symlinked startup file and login shell'
mkdir -p "${fixture_home}/dotfiles"; printf 'alias keep=true\n' > "${fixture_home}/dotfiles/bashrc"
ln -s dotfiles/bashrc "${fixture_home}/.bashrc"
run_installer /bin/bash; expect_exit 0 'bash install'
count=$(marker_count "${fixture_home}/dotfiles/bashrc"); assert 'bashrc target owns one line' test "${count}" = 1
assert 'bashrc remains symlink' test -L "${fixture_home}/.bashrc"
resolved=$(resolve_in /bin/bash -l -c "command -v ${command_name}"); assert 'login bash resolves program' test "${resolved}" = "${fixture_home}/.local/bin/${command_name}"
run_installer /bin/bash --uninstall; expect_exit 0 'bash uninstall'
assert 'startup symlink preserved' test -L "${fixture_home}/.bashrc"
assert 'user alias preserved' grep -qx 'alias keep=true' "${fixture_home}/dotfiles/bashrc"
count=$(marker_count "${fixture_home}/.profile"); assert 'login PATH removed' test "${count}" = 0

begin 'two shell startup aliases of one file uninstall without stale snapshots'
mkdir -p "${fixture_home}/dotfiles"
printf 'alias keep=true\n' > "${fixture_home}/dotfiles/shared"
ln -s dotfiles/shared "${fixture_home}/.bashrc"
ln -s dotfiles/shared "${fixture_home}/.bash_profile"
shared_before=$(hash_file "${fixture_home}/dotfiles/shared")
run_installer /bin/bash; expect_exit 0 'aliased startup install'
run_installer /bin/bash --uninstall; expect_exit 0 'aliased startup uninstall'
shared_after=$(hash_file "${fixture_home}/dotfiles/shared")
assert 'shared user bytes restored exactly' test "${shared_before}" = "${shared_after}"
assert 'first startup alias preserved' test -L "${fixture_home}/.bashrc"
assert 'second startup alias preserved' test -L "${fixture_home}/.bash_profile"
assert 'aliased uninstall removes program' test ! -e "${fixture_home}/.local/bin/${command_name}"
assert 'aliased uninstall removes state' test ! -e "${fixture_home}/.local/bin/.${command_name}-install"

begin 'fish: quoted custom directory and unrelated user content preserved'
fish_install=${fixture_home}/"fish tools'\\bin"
mkdir -p "${fixture_home}/.config/fish/conf.d"
printf '# user content\n' > "${fixture_home}/.config/fish/conf.d/${command_name}.fish"
run_installer /usr/bin/fish --install-dir "${fish_install}"; expect_exit 0 'fish install'
if command -v fish >/dev/null; then
    fish_path=$(command -v fish); resolved=$(resolve_in "${fish_path}" -c "command -v ${command_name}")
    assert 'new fish resolves program' test "${resolved}" = "${fish_install}/${command_name}"
else echo 'fish unavailable; shell-resolution check skipped'; fi
run_installer /usr/bin/fish --install-dir "${fish_install}" --uninstall; expect_exit 0 'fish uninstall'
assert 'user fish content retained' grep -qx '# user content' "${fixture_home}/.config/fish/conf.d/${command_name}.fish"
count=$(marker_count "${fixture_home}/.config/fish/conf.d/${command_name}.fish"); assert 'only owned fish entry removed' test "${count}" = 0

begin 'fish cleanup distinguishes new, pre-existing empty and symlinked fragments'
fish_config=${fixture_home}/.config/fish/conf.d/${command_name}.fish
mkdir -p "${fish_config%/*}"
: > "${fish_config}"
run_installer /usr/bin/fish; expect_exit 0 'pre-existing empty fish install'
run_installer /usr/bin/fish --uninstall; expect_exit 0 'pre-existing empty fish uninstall'
assert 'pre-existing empty fish file retained' test -f "${fish_config}"
assert 'pre-existing fish file is still empty' test ! -s "${fish_config}"
rm "${fish_config}"
printf '' > "${fixture_home}/fish-user-target"
ln -s "${fixture_home}/fish-user-target" "${fish_config}"
run_installer /usr/bin/fish; expect_exit 0 'symlinked fish install'
run_installer /usr/bin/fish --uninstall; expect_exit 0 'symlinked fish uninstall'
assert 'user fish symlink retained' test -L "${fish_config}"
assert 'user fish target retained' test -f "${fixture_home}/fish-user-target"
assert 'only owned line removed from fish target' test ! -s "${fixture_home}/fish-user-target"
rm "${fish_config}"
run_installer /usr/bin/fish; expect_exit 0 'new owned fish install'
run_installer /usr/bin/fish --uninstall; expect_exit 0 'new owned fish uninstall'
assert 'new owned empty fish fragment removed' test ! -e "${fish_config}"

begin 'PATH already present and NoModifyPath leaves startup files untouched'
fixture_path=${fixture_home}/.local/bin:${base_path}; run_installer /bin/sh; expect_exit 0 'already-on-PATH install'
assert 'no startup file created' test ! -e "${fixture_home}/.profile"
assert 'already on PATH reported' grep -q 'already includes' "${work}/out"
run_installer /bin/sh --uninstall; expect_exit 0 'already-on-PATH uninstall'

begin 'custom directory with spaces and apostrophe, quoting and NoModifyPath'
custom_dir=${fixture_home}/"my tools' bin"
run_installer /bin/sh --install-dir "${custom_dir}" --no-modify-path; expect_exit 0 'custom no-PATH install'
assert 'custom binary installed' test -x "${custom_dir}/${command_name}"
assert 'startup untouched' test ! -e "${fixture_home}/.profile"
awk '/^sh install.sh --uninstall / {print}' "${work}/out" > "${work}/hint.sh"
assert 'printed uninstall command has valid shell syntax' sh -n "${work}/hint.sh"
help_command=$(sed -n 's/^Open a new terminal after a PATH change. Run: //p' "${work}/out")
assert 'printed quoted command runs installed program' env -i HOME="${fixture_home}" PATH="${base_path}" sh -c "${help_command}"
run_installer /bin/sh --install-dir "${custom_dir}" --uninstall; expect_exit 0 'custom uninstall'
assert 'installer-created folder removed' test ! -e "${custom_dir}"

begin 'unrelated marker mention and another installation PATH are preserved'
printf "printf '%%s\\n' '# added by the %s installer is documentation'\n" "${product_name}" > "${fixture_home}/.profile"
run_installer /bin/sh --install-dir "${fixture_home}/A"; expect_exit 0 'installation A'
run_installer /bin/sh --install-dir "${fixture_home}/B"; expect_exit 0 'installation B'
run_installer /bin/sh --install-dir "${fixture_home}/A" --uninstall; expect_exit 0 'uninstall A'
assert 'B remains installed' test -x "${fixture_home}/B/${command_name}"
assert 'B PATH remains' grep -Fq "${fixture_home}/B" "${fixture_home}/.profile"
assert 'unrelated marker mention retained' grep -Fq 'is documentation' "${fixture_home}/.profile"
run_installer /bin/sh --install-dir "${fixture_home}/B" --uninstall; expect_exit 0 'uninstall B'
assert 'user line still retained' grep -Fq 'is documentation' "${fixture_home}/.profile"

begin 'startup path changes are remembered; edited entries survive'
mkdir -p "${fixture_home}/original-zdot" "${fixture_home}/different-zdot"
# Public ZDOTDIR input is scoped to this one child, never the real user's shell.
fixture_zdotdir=${fixture_home}/original-zdot
run_installer /bin/zsh
expect_exit 0 'custom ZDOTDIR install'
run_installer /bin/sh --uninstall; expect_exit 0 'uninstall through another shell'
count=$(marker_count "${fixture_home}/original-zdot/.zshrc"); assert 'original actual startup file cleaned' test "${count}" = 0

begin 'NoModifyPath uninstall and shared install directory keep persistent PATH'
run_installer /bin/sh; expect_exit 0 'install for NMP uninstall'
before=$(hash_file "${fixture_home}/.profile")
run_installer /bin/sh --uninstall --no-modify-path; expect_exit 0 'NMP uninstall'
after=$(hash_file "${fixture_home}/.profile"); assert 'NMP preserves startup bytes' test "${before}" = "${after}"
run_installer /bin/sh; expect_exit 0 'install shared prefix'
printf keep > "${fixture_home}/.local/bin/unrelated-tool"
before=$(hash_file "${fixture_home}/.profile")
run_installer /bin/sh --uninstall; expect_exit 0 'shared prefix uninstall'
after=$(hash_file "${fixture_home}/.profile"); assert 'shared prefix PATH kept' test "${before}" = "${after}"
assert 'unrelated program kept' test -f "${fixture_home}/.local/bin/unrelated-tool"

begin 'checksum errors and explicit version mismatch preserve managed installation'
run_installer /bin/sh --no-modify-path; expect_exit 0 'baseline install'
before=$(hash_file "${fixture_home}/.local/bin/${command_name}")
tampered=${work}/tampered; mkdir -p "${tampered}"; cp "${native_archive}" "${tampered}/"
awk '{print "0000000000000000000000000000000000000000000000000000000000000000",$2}' "${dist}/${checksum_name}" > "${tampered}/${checksum_name}"
download_base=${tampered}; run_installer /bin/sh --no-modify-path; expect_failure 'tampered archive'
assert 'checksum mismatch reported' grep -q 'does not match its published SHA-256' "${work}/out"
after=$(hash_file "${fixture_home}/.local/bin/${command_name}"); assert 'old program untouched' test "${before}" = "${after}"
unset download_base
run_installer /bin/sh --version 999.0.0 --no-modify-path; expect_failure 'wrong requested release'
run_installer /bin/sh --version latest; expect_failure 'invalid pin'
empty=${work}/empty; mkdir "${empty}"; download_base=${empty}
run_installer /bin/sh; expect_failure 'missing archive'
assert 'download failure reported' grep -q 'Could not download' "${work}/out"

begin 'explicit empty directory/version values never redirect the operation'
run_installer /bin/sh --no-modify-path; expect_exit 0 'empty-argument baseline install'
before=$(hash_file "${fixture_home}/.local/bin/${command_name}")
state_before=$(hash_file "${fixture_home}/.local/bin/.${command_name}-install/state.sha256")
run_installer /bin/sh --uninstall --install-dir ''; expect_failure 'empty separated install directory'
run_installer /bin/sh --uninstall --install-dir=; expect_failure 'empty equals install directory'
run_installer /bin/sh --version ''; expect_failure 'empty separated version'
run_installer /bin/sh --version=; expect_failure 'empty equals version'
run_installer /bin/sh --version v; expect_failure 'empty version after tag-prefix removal'
after=$(hash_file "${fixture_home}/.local/bin/${command_name}")
state_after=$(hash_file "${fixture_home}/.local/bin/.${command_name}-install/state.sha256")
assert 'default binary untouched by empty targets' test "${before}" = "${after}"
assert 'default receipt untouched by empty targets' test "${state_before}" = "${state_after}"

begin 'receiptless, non-regular and changed targets are refused'
mkdir -p "${fixture_home}/.local/bin"; cp "${work}/${command_name}" "${fixture_home}/.local/bin/${command_name}"
before=$(hash_file "${fixture_home}/.local/bin/${command_name}")
run_installer /bin/sh; expect_failure 'receiptless update'
run_installer /bin/sh --uninstall; expect_failure 'receiptless uninstall'
after=$(hash_file "${fixture_home}/.local/bin/${command_name}"); assert 'unowned program kept' test "${before}" = "${after}"
rm "${fixture_home}/.local/bin/${command_name}"; ln -s /bin/sh "${fixture_home}/.local/bin/${command_name}"
run_installer /bin/sh; expect_failure 'symlink update'
run_installer /bin/sh --uninstall; expect_failure 'symlink uninstall'
assert 'unowned symlink kept' test -L "${fixture_home}/.local/bin/${command_name}"
rm "${fixture_home}/.local/bin/${command_name}"
run_installer /bin/sh --no-modify-path; expect_exit 0 'managed install'
printf '\nchanged\n' >> "${fixture_home}/.local/bin/${command_name}"
before=$(hash_file "${fixture_home}/.local/bin/${command_name}")
run_installer /bin/sh; expect_failure 'changed managed update'
run_installer /bin/sh --uninstall; expect_failure 'changed managed uninstall'
after=$(hash_file "${fixture_home}/.local/bin/${command_name}"); assert 'changed program preserved' test "${before}" = "${after}"

begin 'predictable staging symlink and arbitrary old file are never touched'
mkdir -p "${fixture_home}/tools"; printf private > "${fixture_home}/victim"; printf unrelated > "${fixture_home}/tools/${command_name}.old"
args=(--install-dir "${fixture_home}/tools" --no-modify-path)
[[ "${archive_versioned}" != 1 ]] || args=(--version "${fixture_version}" "${args[@]}")
dollar='$'
wrapper="ln -s \"${dollar}1\" \"${dollar}2/.${dollar}3.${dollar}${dollar}\"; test -L \"${dollar}2/.${dollar}3.${dollar}${dollar}\"; printf 'precreated staging link\\n' >&2; shift 3; exec sh -s -- \"${dollar}@\""
if env -i HOME="${fixture_home}" PATH="${base_path}" SHELL=/bin/sh TMPDIR="${work}" "${env_prefix}_DOWNLOAD_BASE=${dist}" sh -c "${wrapper}" sh "${fixture_home}/victim" "${fixture_home}/tools" "${command_name}" ${args[@]+"${args[@]}"} < "${installer}" > "${work}/out" 2>&1; then last_exit=0; else last_exit=$?; fi
expect_exit 0 'unique staging install'
assert 'staging collision was actually created' grep -q 'precreated staging link' "${work}/out"
old_staging_links=("${fixture_home}/tools/.${command_name}."*)
assert 'old predictable staging symlink remains untouched' test -L "${old_staging_links[0]}"
assert 'victim not overwritten' grep -qx private "${fixture_home}/victim"
assert 'installed executable is regular' test ! -L "${fixture_home}/tools/${command_name}"
assert 'unrelated old file kept' grep -qx unrelated "${fixture_home}/tools/${command_name}.old"

begin 'PATH failures surface and colon paths are never persisted'
fixture_zdotdir=${fixture_home}/missing/parent
run_installer /bin/zsh
expect_failure 'missing startup directory'
assert 'no false PATH success' test ! -e "${fixture_home}/missing/parent/.zshrc"
run_installer /bin/sh --install-dir "${fixture_home}/a:b"; expect_exit 0 'colon path install'
assert 'colon PATH warning' grep -q 'colon cannot be represented' "${work}/out"
assert 'colon path not persisted' test ! -e "${fixture_home}/.profile"

begin 'startup read errors stop update and uninstall without removing owned state'
run_installer /bin/sh; expect_exit 0 'startup-read baseline install'
before=$(hash_file "${fixture_home}/.local/bin/${command_name}")
state_before=$(hash_file "${fixture_home}/.local/bin/.${command_name}-install/state.sha256")
profile_before=$(hash_file "${fixture_home}/.profile")
mkdir "${fixture_home}/read-error-tools"
real_grep=$(command -v grep)
cat > "${fixture_home}/read-error-tools/grep" <<EOF
#!/bin/sh
last=''
for argument do last=\$argument; done
case "\$last" in "\$HOME/.profile" | */path-original.*) printf 'controlled startup read failure\\n' >&2; exit 2 ;; *) exec '${real_grep}' "\$@" ;; esac
EOF
chmod 755 "${fixture_home}/read-error-tools/grep"
fixture_path=${fixture_home}/read-error-tools:${base_path}
run_installer /bin/sh; expect_failure 'update with startup read error'
assert 'read error surfaced on update' grep -q 'Could not read startup file' "${work}/out"
run_installer /bin/sh --uninstall; expect_failure 'uninstall with startup read error'
assert 'read error surfaced on uninstall' grep -q 'Could not read startup file' "${work}/out"
if grep -q 'has been uninstalled' "${work}/out"; then fail 'read failure reported successful uninstall'; fi
after=$(hash_file "${fixture_home}/.local/bin/${command_name}")
state_after=$(hash_file "${fixture_home}/.local/bin/.${command_name}-install/state.sha256")
profile_after=$(hash_file "${fixture_home}/.profile")
assert 'startup read failure preserves binary' test "${before}" = "${after}"
assert 'startup read failure preserves receipt' test "${state_before}" = "${state_after}"
assert 'startup read failure preserves user startup bytes' test "${profile_before}" = "${profile_after}"
unset fixture_path
run_installer /bin/sh --uninstall; expect_exit 0 'uninstall after read error repaired'

begin 'exclusive product and cross-product PATH locks fail promptly'
mkdir -p "${fixture_home}/tools/.${command_name}-install.lock"
printf held > "${fixture_home}/tools/.${command_name}-install.lock/sentinel"
run_installer /bin/sh --install-dir "${fixture_home}/tools"; expect_failure 'held product lock'
assert 'foreign lock left alone' test -f "${fixture_home}/tools/.${command_name}-install.lock/sentinel"
mkdir "${fixture_home}/.cli-installer-path.lock"
run_installer /bin/sh --install-dir "${fixture_home}/other"; expect_failure 'held shared PATH lock'
assert 'shared lock retained' test -d "${fixture_home}/.cli-installer-path.lock"
run_installer /bin/sh --install-dir "${fixture_home}/other" --no-modify-path; expect_exit 0 'NMP does not acquire PATH lock'

begin 'actual producer version format includes balanced commit, Go and platform provenance'
fixture_version=1.2.3
provenance_release=${work}/producer-provenance
make_release "${provenance_release}" "printf '%s\\n' '${command_name} 1.2.3 (abc123, go1.25.0, darwin/arm64)'"
download_base=${provenance_release}; run_installer /bin/sh --no-modify-path
expect_exit 0 'actual PDF producer provenance shape'

begin 'strict candidate identity, original execution diagnostics and link members'
fixture_version=1.2.3
payload_number=0
for bad in "printf '%s\\n' '${command_name} invalid'" "printf '%s\\n' '${command_name} 1.2.3 extra'" "printf '%s\\n' '${command_name} 1.2.3' 'extra line'" "printf '%s\\n' '${command_name} 1.2.3 (nested (field))'" "printf '%s\\n' '${command_name} 1.2.3 (field) extra'" "printf '%s\\r\\n' '${command_name} 1.2.3 (field)'" "printf 'loader failure sentinel\\n' >&2; exit 9"; do
    payload_number=$((payload_number + 1)); candidate_release=${work}/candidate-${payload_number}; make_release "${candidate_release}" "${bad}"
    download_base=${candidate_release}; run_installer /bin/sh --no-modify-path; expect_failure 'invalid candidate'
done
assert 'original execution cause shown' grep -q 'loader failure sentinel' "${work}/out"
link_release=${work}/link-release; mkdir -p "${link_release}/payload"; ln -s /bin/sh "${link_release}/payload/${command_name}"
asset=$(new_asset); tar -czf "${link_release}/${asset}" -C "${link_release}/payload" "${command_name}"
digest=$(hash_file "${link_release}/${asset}"); printf '%s  %s\n' "${digest}" "${asset}" > "${link_release}/${checksum_name}"
download_base=${link_release}; run_installer /bin/sh --no-modify-path; expect_failure 'archive symlink member'
assert 'regular-file rejection reported' grep -q 'not a regular file' "${work}/out"

begin 'receipt edits, extra sidecar data and per-transaction rollback'
run_installer /bin/sh --no-modify-path; expect_exit 0 'rollback baseline install'
before=$(hash_file "${fixture_home}/.local/bin/${command_name}")
receipt=${fixture_home}/.local/bin/.${command_name}-install
state_before=$(hash_file "${receipt}/state.sha256")
printf note > "${receipt}/unrelated-note"
run_installer /bin/sh --no-modify-path; expect_failure 'extra sidecar user file'
assert 'extra sidecar data retained' test -f "${receipt}/unrelated-note"
state_after=$(hash_file "${receipt}/state.sha256"); assert 'invalid receipt not rewritten' test "${state_before}" = "${state_after}"
rm "${receipt}/unrelated-note"
printf corrupt >> "${receipt}/state.sha256"
state_before=$(hash_file "${receipt}/state.sha256")
run_installer /bin/sh --uninstall; expect_failure 'corrupt receipt uninstall'
state_after=$(hash_file "${receipt}/state.sha256"); assert 'corrupt receipt retained exactly' test "${state_before}" = "${state_after}"
# Restore the known test copy, then fail the actual activation rename externally.
cp "${receipt}/state.sha256" "${work}/corrupt-receipt"
# This fixture edit is ours; the unmodified manifest prefix is recovered as data.
sed 's/corrupt$//' "${work}/corrupt-receipt" > "${receipt}/state.sha256"
state_before=$(hash_file "${receipt}/state.sha256")
mkdir "${fixture_home}/fail-tools"
real_mv=$(command -v mv)
cat > "${fixture_home}/fail-tools/mv" <<EOF
#!/bin/sh
case "\$1" in */.${command_name}-stage.*/new-state) exit 91 ;; *) exec '${real_mv}' "\$@" ;; esac
EOF
chmod 755 "${fixture_home}/fail-tools/mv"
fixture_path=${fixture_home}/fail-tools:${base_path}
run_installer /bin/sh --no-modify-path; expect_failure 'activation rename failure'
after=$(hash_file "${fixture_home}/.local/bin/${command_name}"); assert 'old program restored' test "${before}" = "${after}"
state_after=$(hash_file "${receipt}/state.sha256"); assert 'old ownership restored' test "${state_before}" = "${state_after}"
assert 'failed activation leaves no lock' test ! -e "${fixture_home}/.local/bin/.${command_name}-install.lock"

begin 'credential, query and fragment mirror bases are rejected without echoing secrets'
for bad_base in 'https://name:secret-marker@example.invalid/releases' 'https://example.invalid/releases?token=secret-marker' 'https://example.invalid/releases#secret-marker'; do
    download_base=${bad_base}
    run_installer /bin/sh --no-modify-path; expect_failure 'unsupported mirror credentials'
    if grep -q secret-marker "${work}/out"; then fail 'mirror secret echoed'; fi
done

for downloader in curl wget; do
    begin "${downloader}: latest resolved once, immutable-tag HTTPS fetches"
    fixture_version=1.2.3
    mock=${fixture_home}/mock; mkdir -p "${mock}/tools"
    make_release "${mock}/dist" "printf '%s\\n' '${command_name} 1.2.3'"
    printf '%s\n' "${repo_url}" > "${mock}/repo"
    printf '%s/releases/tag/v1.2.3\n' "${repo_url}" > "${mock}/effective"
    # Isolate tool discovery so both supported downloaders are exercised, even
    # when the host has curl. These are external-dependency stubs, not product hooks.
    for utility in awk cat chmod cmp cp cut find grep gzip head mkdir mktemp mv rm rmdir sed sh sort tail tar tr uname wc sha256sum shasum openssl sw_vers sysctl; do
        if tool_path=$(command -v "${utility}"); then ln -s "${tool_path}" "${mock}/tools/${utility}"; fi
    done
    if [[ "${downloader}" == curl ]]; then
        cat > "${mock}/tools/curl" <<'EOF'
#!/bin/sh
set -eu
output='' url='' proto=0 redirects=0 write_effective=0
while [ "$#" -gt 0 ]; do
    case "$1" in
        --output) output=$2; shift 2 ;;
        --proto) [ "$2" = '=https' ]; proto=1; shift 2 ;;
        --proto-redir) [ "$2" = '=https' ]; redirects=1; shift 2 ;;
        --write-out) [ "$2" = '%{url_effective}' ]; write_effective=1; shift 2 ;;
        --connect-timeout) shift 2 ;;
        --*) shift ;;
        *) url=$1; shift ;;
    esac
done
[ "$proto" = 1 ] && [ "$redirects" = 1 ]
repo=$(cat "$HOME/mock/repo")
printf '%s\n' "$url" >> "$HOME/mock/requests"
case "$url" in
    "$repo/releases/latest") [ "$write_effective" = 1 ] && [ "$output" = /dev/null ]; cat "$HOME/mock/effective" ;;
    "$repo/releases/download/v1.2.3/"*) name=${url##*/}; cp "$HOME/mock/dist/$name" "$output" ;;
    *) exit 92 ;;
esac
EOF
    else
        cat > "${mock}/tools/wget" <<'EOF'
#!/bin/sh
set -eu
output='' url='' https=0 response=0
while [ "$#" -gt 0 ]; do
    case "$1" in
        --output-document=*) output=${1#*=}; shift ;;
        --https-only) https=1; shift ;;
        --server-response) response=1; shift ;;
        --*) shift ;;
        *) url=$1; shift ;;
    esac
done
[ "$https" = 1 ]
repo=$(cat "$HOME/mock/repo")
printf '%s\n' "$url" >> "$HOME/mock/requests"
case "$url" in
    "$repo/releases/latest") [ "$response" = 1 ] && [ "$output" = /dev/null ]; effective=$(cat "$HOME/mock/effective"); printf 'HTTP/1.1 302 Found\nLocation: %s\nHTTP/1.1 200 OK\n' "$effective" >&2 ;;
    "$repo/releases/download/v1.2.3/"*) name=${url##*/}; cp "$HOME/mock/dist/$name" "$output" ;;
    *) exit 92 ;;
esac
EOF
    fi
    chmod 755 "${mock}/tools/${downloader}"
    fixture_path=${mock}/tools; download_base=''
    run_installer /bin/sh --no-modify-path; expect_exit 0 "${downloader} pinned download"
    latest_count=$(grep -cF "${repo_url}/releases/latest" "${mock}/requests")
    request_count=$(wc -l < "${mock}/requests" | tr -d ' ')
    pinned_count=$(grep -cF "${repo_url}/releases/download/v1.2.3/" "${mock}/requests")
    assert 'latest queried once' test "${latest_count}" = 1
    assert 'one resolution plus two downloads' test "${request_count}" = 3
    assert 'both downloads pinned to one tag' test "${pinned_count}" = 2
    before=$(hash_file "${fixture_home}/.local/bin/${command_name}")
    printf 'http://github.com/unsafe/releases/tag/v1.2.3\n' > "${mock}/effective"
    run_installer /bin/sh --no-modify-path; expect_failure 'HTTP latest destination'
    printf '%s/releases/tag/v01.2.3\n' "${repo_url}" > "${mock}/effective"
    run_installer /bin/sh --no-modify-path; expect_failure 'noncanonical latest version'
    after=$(hash_file "${fixture_home}/.local/bin/${command_name}")
    assert 'bad redirect leaves installed binary untouched' test "${before}" = "${after}"
done

printf '\n'
if [[ "${failures}" -gt 0 ]]; then printf '%s check(s) failed.\n' "${failures}" >&2; exit 1; fi
printf 'All installer checks passed.\n'
