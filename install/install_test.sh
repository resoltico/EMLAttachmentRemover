#!/bin/bash
# SPDX-License-Identifier: AGPL-3.0-only
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
archive_rooted=$(setting archive_rooted)
archive_member() {
    if [[ "${archive_rooted}" == 1 ]]; then printf '%s/%s' "${1%.tar.gz}" "$2"; else printf '%s' "$2"; fi
}
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
native_member=$(archive_member "${native_asset}" "${command_name}")
tar -xOzf "${native_archive}" "${native_member}" > "${work}/${command_name}"
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
pack_release() {
    local target=$1 asset name digest
    shift
    asset=$(new_asset)
    local members=()
    for name in "$@"; do members+=("$(archive_member "${asset}" "${name}")"); done
    tar -czf "${target}/${asset}" -C "${target}/payload" "${members[@]}"
    digest=$(hash_file "${target}/${asset}")
    printf '%s  %s\n' "${digest}" "${asset}" > "${target}/${checksum_name}"
}
make_release() {
    local target=$1 body=$2 asset executable legal member
    asset=$(new_asset); executable=$(archive_member "${asset}" "${command_name}")
    mkdir -p "${target}/payload/${executable%"${command_name}"}"
    printf '#!/bin/sh\n%s\n' "${body}" > "${target}/payload/${executable}"
    chmod 755 "${target}/payload/${executable}"
    local members=("${command_name}")
    for legal in ${legal_files}; do
        member=$(archive_member "${asset}" "${legal}")
        printf 'fixture legal material\n' > "${target}/payload/${member}"
        members+=("${legal}")
    done
    pack_release "${target}" "${members[@]}"
}

begin 'primary archive checksum blocks execution before version bootstrap'
primary_control=${work}/primary-control; mkdir -p "${primary_control}/dist" "${primary_control}/payload" "${primary_control}/tmp"
cp "${installer}" "${primary_control}/dist/install.sh"
cp "${dist}/${checksum_name}" "${primary_control}/dist/${checksum_name}"
mkdir -p "${primary_control}/payload/${native_member%"${command_name}"}"
cat > "${primary_control}/payload/${native_member}" <<'EOF'
#!/bin/sh
candidate_dir=${0%/*}
control_root=${candidate_dir%/*}
printf 'executed\n' > "${control_root}/bootstrap-executed"
EOF
printf "printf '%%s\\n' '%s 1.2.3'\n" "${command_name}" >> "${primary_control}/payload/${native_member}"
chmod 755 "${primary_control}/payload/${native_member}"
tar -czf "${primary_control}/dist/${native_asset}" -C "${primary_control}/payload" "${native_member}"
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
    if [[ "${command_name}" == pdfconcat ]]; then
        env -i HOME="${fixture_home}" PATH="${base_path}" "${fixture_home}/.local/bin/pdfconcat" build -o "${fixture_home}/blank.pdf" --blank --fit-to A4
        assert 'installed PDFConcat creates a PDF' test -s "${fixture_home}/blank.pdf"
    fi
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

begin 'strict candidate identity and original execution diagnostics'
fixture_version=1.2.3
payload_number=0
for bad in "printf '%s\\n' '${command_name} invalid'" "printf '%s\\n' '${command_name} 1.2.3 extra'" "printf '%s\\n' '${command_name} 1.2.3' 'extra line'" "printf '%s\\n' '${command_name} 1.2.3 (nested (field))'" "printf '%s\\n' '${command_name} 1.2.3 (field) extra'" "printf '%s\\r\\n' '${command_name} 1.2.3 (field)'" "printf 'loader failure sentinel\\n' >&2; exit 9"; do
    payload_number=$((payload_number + 1)); candidate_release=${work}/candidate-${payload_number}; make_release "${candidate_release}" "${bad}"
    download_base=${candidate_release}; run_installer /bin/sh --no-modify-path; expect_failure 'invalid candidate'
done
assert 'original execution cause shown' grep -q 'loader failure sentinel' "${work}/out"

begin 'exact archive root and required regular members protect the active installation'
fixture_version=1.2.3
valid_release=${work}/valid-layout
make_release "${valid_release}" "printf '%s\\n' '${command_name} 1.2.3'"
download_base=${valid_release}; run_installer /bin/sh --no-modify-path
expect_exit 0 'valid configured archive layout'
before=$(hash_file "${fixture_home}/.local/bin/${command_name}")
state_before=$(hash_file "${fixture_home}/.local/bin/.${command_name}-install/state.sha256")
asset=$(new_asset)
required_members=("${command_name}")
for legal in ${legal_files}; do required_members+=("${legal}"); done
for layout in wrong-root alternate-layout; do
    layout_release=${work}/${layout}
    mkdir -p "${layout_release}/payload"
    if [[ "${layout}" == alternate-layout && "${archive_rooted}" == 1 ]]; then prefix=''
    elif [[ "${layout}" == alternate-layout ]]; then prefix=${asset%.tar.gz}/
    else prefix=${asset%.tar.gz}-wrong/; fi
    mkdir -p "${layout_release}/payload/${prefix}"
    layout_members=()
    for name in "${required_members[@]}"; do
        source_member=$(archive_member "${asset}" "${name}")
        cp "${valid_release}/payload/${source_member}" "${layout_release}/payload/${prefix}${name}"
        layout_members+=("${prefix}${name}")
    done
    tar -czf "${layout_release}/${asset}" -C "${layout_release}/payload" "${layout_members[@]}"
    digest=$(hash_file "${layout_release}/${asset}")
    printf '%s  %s\n' "${digest}" "${asset}" > "${layout_release}/${checksum_name}"
    download_base=${layout_release}; run_installer /bin/sh --no-modify-path; expect_failure "${layout} archive"
    expected_member=$(archive_member "${asset}" "${command_name}")
    assert 'layout failure identifies exact expected member' grep -Fq "The archive must contain exactly one ${expected_member}." "${work}/out"
    after=$(hash_file "${fixture_home}/.local/bin/${command_name}")
    state_after=$(hash_file "${fixture_home}/.local/bin/.${command_name}-install/state.sha256")
    assert 'wrong layout preserves program' test "${before}" = "${after}"
    assert 'wrong layout preserves ownership' test "${state_before}" = "${state_after}"
done
for name in "${required_members[@]}"; do
    expected_member=$(archive_member "${asset}" "${name}")
    for defect in missing duplicate symlink; do
        member_release=${work}/member-${name}-${defect}
        make_release "${member_release}" "printf '%s\\n' '${command_name} 1.2.3'"
        members=()
        for other in "${required_members[@]}"; do
            [[ "${defect}" != missing || "${other}" != "${name}" ]] || continue
            members+=("${other}")
        done
        case "${defect}" in
            missing)
                # Keep even the no-legal-files archive nonempty, without the selected member.
                printf 'unrelated\n' > "${member_release}/payload/$(archive_member "${asset}" unrelated)"
                members+=(unrelated)
                ;;
            duplicate) members+=("${name}") ;;
            symlink)
                rm "${member_release}/payload/${expected_member}"
                ln -s /bin/sh "${member_release}/payload/${expected_member}"
                ;;
            *) fail "Unknown archive member defect: ${defect}"; exit 1 ;;
        esac
        pack_release "${member_release}" "${members[@]}"
        download_base=${member_release}; run_installer /bin/sh --no-modify-path
        expect_failure "${defect} archive member ${expected_member}"
        if [[ "${defect}" == symlink ]]; then diagnostic="Archive member is not a regular file: ${expected_member}"
        else diagnostic="The archive must contain exactly one ${expected_member}."; fi
        assert 'member failure reports the intended boundary' grep -Fq "${diagnostic}" "${work}/out"
        after=$(hash_file "${fixture_home}/.local/bin/${command_name}")
        state_after=$(hash_file "${fixture_home}/.local/bin/.${command_name}-install/state.sha256")
        assert 'invalid member preserves program' test "${before}" = "${after}"
        assert 'invalid member preserves ownership' test "${state_before}" = "${state_after}"
    done
done

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
    for utility in awk cat chmod cmp cp cut dd find grep gzip head ls mkdir mktemp mv rm rmdir sed sh sort stat tail tar tr uname wc sha256sum shasum openssl sw_vers sysctl; do
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


begin 'explicit receiptless replacement preserves original bytes outside the shared bin'
replacement_dir=${fixture_home}/tools
mkdir -p "${replacement_dir}"
cat > "${replacement_dir}/${command_name}" <<'EOF'
#!/bin/sh
printf untouched > "$HOME/unowned-was-run"
exit 71
EOF
chmod 755 "${replacement_dir}/${command_name}"
original_hash=$(hash_file "${replacement_dir}/${command_name}")
printf '# user startup content\n' > "${fixture_home}/.profile"
profile_before=$(hash_file "${fixture_home}/.profile")
# Local conflicts must precede fetching even when no usable mirror is available.
download_base=${fixture_home}/does-not-exist
run_installer /bin/sh --install-dir "${replacement_dir}"; expect_failure 'receiptless default refusal before download'
assert 'receiptless diagnostic names target' grep -Fq "${replacement_dir}/${command_name}" "${work}/out"
assert 'receiptless diagnostic offers bounded option' grep -q -- '--replace-unmanaged' "${work}/out"
if grep -q 'Could not download' "${work}/out"; then fail 'receiptless local conflict was hidden by download'; fi
assert 'default refusal does not create state' test ! -e "${replacement_dir}/.${command_name}-install"
after=$(hash_file "${replacement_dir}/${command_name}"); assert 'default refusal preserves unowned bytes' test "${original_hash}" = "${after}"
bad_replacement_mirror=${fixture_home}/bad-release; mkdir "${bad_replacement_mirror}"
cp "${native_archive}" "${bad_replacement_mirror}/"
printf '%064d  %s\n' 0 "${native_asset}" > "${bad_replacement_mirror}/${checksum_name}"
download_base=${bad_replacement_mirror}
run_installer /bin/sh --install-dir "${replacement_dir}" --replace-unmanaged; expect_failure 'explicit replacement still verifies candidate checksum'
after=$(hash_file "${replacement_dir}/${command_name}"); assert 'bad candidate preserves original bytes' test "${original_hash}" = "${after}"
after=$(hash_file "${fixture_home}/.profile"); assert 'bad candidate preserves startup bytes' test "${profile_before}" = "${after}"
assert 'bad candidate creates no receipt' test ! -e "${replacement_dir}/.${command_name}-install"
unset download_base
# Force the cross-platform mktemp spelling difference on every native host.
mkdir "${fixture_home}/normalized-tools"
real_mktemp=$(command -v mktemp)
cat > "${fixture_home}/normalized-tools/mktemp" <<EOF
#!/bin/sh
created=\$('${real_mktemp}' "\$@") || exit 1
case "\$created" in */.${command_name}-stage.*) (cd "\$created" && pwd -P) ;; *) printf '%s\\n' "\$created" ;; esac
EOF
chmod 755 "${fixture_home}/normalized-tools/mktemp"
fixture_path=${fixture_home}/normalized-tools:${base_path}
run_installer /bin/sh --install-dir "${replacement_dir}/." --replace-unmanaged; expect_exit 0 'explicit backed-up replacement through a dot alias'
assert 'unowned executable never ran' test ! -e "${fixture_home}/unowned-was-run"
recovery_files=("${fixture_home}/.${command_name}-recovery."*/"${command_name}")
assert 'one exclusively named retained recovery backup' test "${#recovery_files[@]}" = 1
assert 'retained recovery backup is a regular file' test -f "${recovery_files[0]}"
backup_hash=$(hash_file "${recovery_files[0]}"); assert 'backup preserves exact original bytes' test "${original_hash}" = "${backup_hash}"
reported_backup_directory=$(cd "${recovery_files[0]%/*}" && pwd -P)
reported_backup=${reported_backup_directory}/${command_name}
assert 'physical backup path is reported' grep -Fq "${reported_backup}" "${work}/out"
assert 'new directory ownership not adopted' grep -qx 0 "${replacement_dir}/.${command_name}-install/created-dir"
count=$(marker_count "${fixture_home}/.profile"); assert 'replacement owns only new PATH line' test "${count}" = 1
run_installer /bin/sh --install-dir "${replacement_dir}" --uninstall; expect_exit 0 'replaced executable uninstall'
assert 'uninstall preserves recovery backup' test -f "${recovery_files[0]}"
backup_hash=$(hash_file "${recovery_files[0]}"); assert 'uninstall leaves original backup unchanged' test "${original_hash}" = "${backup_hash}"
after=$(hash_file "${fixture_home}/.profile"); assert 'external recovery backup does not suppress exact PATH cleanup' test "${profile_before}" = "${after}"
assert 'pre-existing bin directory retained' test -d "${replacement_dir}"
run_installer /bin/sh --install-dir "${replacement_dir}" --uninstall --replace-unmanaged; expect_failure 'replacement option is install-only'

begin 'intact ownership with missing payload supports repair and exact cleanup'
run_installer /bin/sh; expect_exit 0 'missing-payload baseline'
receipt=${fixture_home}/.local/bin/.${command_name}-install
binary_path=${fixture_home}/.local/bin/${command_name}
state_before=$(hash_file "${receipt}/state.sha256")
profile_before=$(hash_file "${fixture_home}/.profile")
rm "${binary_path}"
run_installer /bin/sh; expect_exit 0 'repair missing payload'
assert 'repair installs verified payload' test -x "${binary_path}"
state_after=$(hash_file "${receipt}/state.sha256"); assert 'repair preserves complete existing ownership' test "${state_before}" = "${state_after}"
after=$(hash_file "${fixture_home}/.profile"); assert 'repair does not duplicate PATH' test "${profile_before}" = "${after}"
rm "${binary_path}"
state_before=$(hash_file "${receipt}/state.sha256")
mkdir -p "${fixture_home}/missing-rollback-tools"
real_mv=$(command -v mv)
cat > "${fixture_home}/missing-rollback-tools/mv" <<EOF
#!/bin/sh
case "\$1" in */new-state) exit 91 ;; *) exec '${real_mv}' "\$@" ;; esac
EOF
chmod 755 "${fixture_home}/missing-rollback-tools/mv"
fixture_path=${fixture_home}/missing-rollback-tools:${base_path}
run_installer /bin/sh --no-modify-path; expect_failure 'activation failure during missing-payload repair'
assert 'repair rollback preserves prior payload absence' test ! -e "${binary_path}"
state_after=$(hash_file "${receipt}/state.sha256"); assert 'repair rollback restores intact receipt' test "${state_before}" = "${state_after}"
unset fixture_path
run_installer /bin/sh --uninstall; expect_exit 0 'uninstall missing payload'
assert 'missing-payload cleanup removes ownership state' test ! -e "${receipt}"
count=$(marker_count "${fixture_home}/.profile"); assert 'missing-payload cleanup removes owned PATH' test "${count}" = 0
run_installer /bin/sh --uninstall; expect_exit 0 'missing-payload cleanup is idempotent'

begin 'existing directory aliases preserve the original exact PATH ownership'
run_installer /bin/sh --install-dir "${fixture_home}/tools"; expect_exit 0 'alias baseline'
profile_before=$(hash_file "${fixture_home}/.profile")
ln -s tools "${fixture_home}/alias"
run_installer /bin/sh --install-dir "${fixture_home}/alias"; expect_exit 0 'physical alias update'
after=$(hash_file "${fixture_home}/.profile"); assert 'alias update keeps exact recorded PATH bytes' test "${profile_before}" = "${after}"
run_installer /bin/sh --install-dir "${fixture_home}/tools/."; expect_exit 0 'dot alias update'
after=$(hash_file "${fixture_home}/.profile"); assert 'dot alias update keeps exact recorded PATH bytes' test "${profile_before}" = "${after}"
run_installer /bin/sh --install-dir "${fixture_home}/alias" --uninstall; expect_exit 0 'physical alias uninstall'
assert 'alias itself is preserved' test -L "${fixture_home}/alias"
assert 'alias cleanup removes the installer-created empty physical directory' test ! -e "${fixture_home}/tools"
count=$(marker_count "${fixture_home}/.profile"); assert 'alias uninstall cleans original owned PATH' test "${count}" = 0

begin 'replacement option cannot bypass modified, corrupt or nonregular ownership'
run_installer /bin/sh --no-modify-path; expect_exit 0 'bounded option baseline'
receipt=${fixture_home}/.local/bin/.${command_name}-install
binary_path=${fixture_home}/.local/bin/${command_name}
printf changed >> "${binary_path}"
before=$(hash_file "${binary_path}")
state_before=$(hash_file "${receipt}/state.sha256")
run_installer /bin/sh --replace-unmanaged; expect_failure 'replacement cannot bypass modified managed payload'
after=$(hash_file "${binary_path}"); assert 'modified payload retained exactly' test "${before}" = "${after}"
assert 'modified diagnostic is specific' grep -q 'Managed executable was modified' "${work}/out"
printf corrupt >> "${receipt}/state.sha256"
state_before=$(hash_file "${receipt}/state.sha256")
run_installer /bin/sh --replace-unmanaged; expect_failure 'replacement cannot bypass corrupt receipt'
state_after=$(hash_file "${receipt}/state.sha256"); assert 'corrupt receipt retained exactly' test "${state_before}" = "${state_after}"
after=$(hash_file "${binary_path}"); assert 'corrupt-state refusal preserves payload' test "${before}" = "${after}"
# Deliberately remove only this test's own fixture state to construct a receiptless symlink.
rm -rf "${receipt}"; rm "${binary_path}"
printf unrelated > "${fixture_home}/symlink-target"
ln -s "${fixture_home}/symlink-target" "${binary_path}"
run_installer /bin/sh --replace-unmanaged; expect_failure 'replacement cannot bypass symlink target'
assert 'receiptless symlink retained' test -L "${binary_path}"
assert 'symlink target untouched' grep -qx unrelated "${fixture_home}/symlink-target"

begin 'a target changed after preflight is not overwritten under stale replacement authority'
mkdir -p "${fixture_home}/.local/bin"
printf '#!/bin/sh\nexit 19\n' > "${fixture_home}/.local/bin/${command_name}"
chmod 755 "${fixture_home}/.local/bin/${command_name}"
fixture_version=1.2.3
side_effect_release=${work}/preflight-side-effect
make_release "${side_effect_release}" "printf 'changed during candidate verification\\n' > \"\${HOME}/.local/bin/${command_name}\"; printf '%s\\n' '${command_name} 1.2.3'"
download_base=${side_effect_release}
run_installer /bin/sh --replace-unmanaged --no-modify-path; expect_failure 'stale receiptless target'
assert 'changed target preserved' grep -qx 'changed during candidate verification' "${fixture_home}/.local/bin/${command_name}"
assert 'stale target leaves no receipt' test ! -e "${fixture_home}/.local/bin/.${command_name}-install"
stale_recoveries=("${fixture_home}/.local/.${command_name}-recovery."*)
assert 'stale target leaves no recovery backup' test ! -e "${stale_recoveries[0]}"
assert 'stale authority diagnosed' grep -q 'changed since preflight' "${work}/out"

begin 'failed startup creation owns no fictional PATH entry and retries safely'
mkdir -p "${fixture_home}/race-tools"
real_cp=$(command -v cp)
cat > "${fixture_home}/race-tools/cp" <<EOF
#!/bin/sh
last=''
for argument do last=\$argument; done
case "\$last" in */new-state/state.sha256) printf 'concurrent user startup\\n' > "\$HOME/.profile" ;; *) : ;; esac
exec '${real_cp}' "\$@"
EOF
chmod 755 "${fixture_home}/race-tools/cp"
fixture_path=${fixture_home}/race-tools:${base_path}
run_installer /bin/sh; expect_failure 'startup created between plan and append'
assert 'concurrent startup bytes preserved' grep -qx 'concurrent user startup' "${fixture_home}/.profile"
assert 'startup conflict leaves no active receipt' test ! -e "${fixture_home}/.local/bin/.${command_name}-install"
assert 'startup conflict leaves no active binary' test ! -e "${fixture_home}/.local/bin/${command_name}"
unset fixture_path
run_installer /bin/sh; expect_exit 0 'retry after failed startup creation'
assert 'retry preserves concurrent user content' grep -qx 'concurrent user startup' "${fixture_home}/.profile"
count=$(marker_count "${fixture_home}/.profile"); assert 'retry actually creates one owned PATH entry' test "${count}" = 1
run_installer /bin/sh --uninstall; expect_exit 0 'cleanup after startup race retry'
assert 'cleanup retains concurrent user content' grep -qx 'concurrent user startup' "${fixture_home}/.profile"
count=$(marker_count "${fixture_home}/.profile"); assert 'cleanup removes only successful retry addition' test "${count}" = 0

begin 'activation rollback removes only its bounded append while preserving concurrent content'
printf '# original user startup\n' > "${fixture_home}/.profile"
mkdir -p "${fixture_home}/rollback-tools"
real_mv=$(command -v mv)
cat > "${fixture_home}/rollback-tools/mv" <<EOF
#!/bin/sh
case "\$1" in */new-state)
    duplicate=\$(tail -n 1 "\$HOME/.profile")
    printf '%s\\n' "\$duplicate" >> "\$HOME/.profile"
    printf 'concurrent user addition\\n' >> "\$HOME/.profile"
    exit 91 ;;
*) exec '${real_mv}' "\$@" ;;
esac
EOF
chmod 755 "${fixture_home}/rollback-tools/mv"
fixture_path=${fixture_home}/rollback-tools:${base_path}
run_installer /bin/sh; expect_failure 'activation failure after a completed PATH append'
assert 'failed activation removes new payload' test ! -e "${fixture_home}/.local/bin/${command_name}"
assert 'failed activation removes new receipt' test ! -e "${fixture_home}/.local/bin/.${command_name}-install"
assert 'rollback preserves original startup content' grep -qx '# original user startup' "${fixture_home}/.profile"
assert 'rollback preserves concurrent suffix' grep -qx 'concurrent user addition' "${fixture_home}/.profile"
count=$(marker_count "${fixture_home}/.profile"); assert 'rollback retains the concurrent identical user line' test "${count}" = 1


begin 'partial PATH writes retain precise recovery evidence without false ownership'
printf '# original user startup\n' > "${fixture_home}/.profile"
cp "${fixture_home}/.profile" "${work}/partial-path-before"
mkdir -p "${fixture_home}/partial-tools"
real_cat=$(command -v cat)
cat > "${fixture_home}/partial-tools/cat" <<EOF
#!/bin/sh
case "\$1" in */path-added.*) head -c 20 "\$1"; exit 93 ;; *) exec '${real_cat}' "\$@" ;; esac
EOF
chmod 755 "${fixture_home}/partial-tools/cat"
fixture_path=${fixture_home}/partial-tools:${base_path}
run_installer /bin/sh; expect_failure 'partially written startup append'
assert 'partial append is not reported restored' grep -q 'restoration was incomplete' "${work}/out"
retained_stages=("${fixture_home}/.local/bin/.${command_name}-stage."*)
assert 'partial append retains transaction evidence' test -d "${retained_stages[0]}"
cat "${work}/partial-path-before" > "${work}/partial-path-expected"
head -c 20 "${retained_stages[0]}/path-added.0" >> "${work}/partial-path-expected"
assert 'partial append and original user bytes retained exactly' cmp -s "${work}/partial-path-expected" "${fixture_home}/.profile"
assert 'partial append does not activate program' test ! -e "${fixture_home}/.local/bin/${command_name}"
assert 'partial append does not activate fictional receipt' test ! -e "${fixture_home}/.local/bin/.${command_name}-install"
unset fixture_path
before=$(hash_file "${fixture_home}/.profile")
run_installer /bin/sh; expect_failure 'retained partial transaction requires review'
assert 'retry names retained evidence' grep -Fq "${retained_stages[0]}" "${work}/out"
after=$(hash_file "${fixture_home}/.profile"); assert 'retry preserves partial-write evidence' test "${before}" = "${after}"

begin 'failed payload restoration retains the original backup and blocks unsafe retry'
run_installer /bin/sh --no-modify-path; expect_exit 0 'incomplete-restore baseline'
before=$(hash_file "${fixture_home}/.local/bin/${command_name}")
receipt=${fixture_home}/.local/bin/.${command_name}-install
state_before=$(hash_file "${receipt}/state.sha256")
fixture_version=1.2.3
restore_release=${work}/incomplete-restore-release
make_release "${restore_release}" "printf '%s\\n' '${command_name} 1.2.3'"
download_base=${restore_release}
mkdir -p "${fixture_home}/restore-tools"
real_mv=$(command -v mv); real_cp=$(command -v cp)
cat > "${fixture_home}/restore-tools/mv" <<EOF
#!/bin/sh
case "\$1" in */new-state) exit 91 ;; *) exec '${real_mv}' "\$@" ;; esac
EOF
cat > "${fixture_home}/restore-tools/cp" <<EOF
#!/bin/sh
case "\${2-}" in */previous-binary) exit 92 ;; *) : ;; esac
exec '${real_cp}' "\$@"
EOF
chmod 755 "${fixture_home}/restore-tools/mv" "${fixture_home}/restore-tools/cp"
fixture_path=${fixture_home}/restore-tools:${base_path}
run_installer /bin/sh --no-modify-path; expect_failure 'failed original payload restoration'
assert 'incomplete restoration is explicit' grep -q 'restoration was incomplete' "${work}/out"
retained_stages=("${fixture_home}/.local/bin/.${command_name}-stage."*)
assert 'incomplete restoration retains original binary backup' test -f "${retained_stages[0]}/previous-binary"
backup_hash=$(hash_file "${retained_stages[0]}/previous-binary"); assert 'retained original bytes are exact' test "${before}" = "${backup_hash}"
state_after=$(hash_file "${receipt}/state.sha256"); assert 'previous receipt restored despite payload failure' test "${state_before}" = "${state_after}"
unset fixture_path
current_hash=$(hash_file "${fixture_home}/.local/bin/${command_name}")
run_installer /bin/sh --uninstall --no-modify-path; expect_failure 'incomplete restoration cannot be silently uninstalled'
assert 'refusal names retained rollback evidence' grep -Fq "${retained_stages[0]}" "${work}/out"
after=$(hash_file "${fixture_home}/.local/bin/${command_name}"); assert 'recovery refusal preserves current executable' test "${current_hash}" = "${after}"
backup_hash=$(hash_file "${retained_stages[0]}/previous-binary"); assert 'recovery refusal preserves original backup' test "${before}" = "${backup_hash}"


begin 'receipts own verified legal inventory while new packages require all current legal files'
saved_installer=${installer}; saved_legal_files=${legal_files}
small_legal_installer=${work}/small-legal-install.sh
expanded_legal_installer=${work}/expanded-legal-install.sh
for fixture_legal_files in LICENSE 'LICENSE EXTRA-NOTICE'; do
    destination=${small_legal_installer}
    [[ "${fixture_legal_files}" == LICENSE ]] || destination=${expanded_legal_installer}
    # Vary only one literal product setting; the common engine remains exact.
    awk -v value="${fixture_legal_files}" '
        $0=="# BEGIN PRODUCT SETTINGS" {inside=1}
        $0=="# END PRODUCT SETTINGS" {inside=0}
        inside && /^legal_files=/ {print "legal_files=" sprintf("%c",39) value sprintf("%c",39); count++; next}
        {print}
        END {if(count!=1) exit 1}
    ' "${saved_installer}" > "${destination}"
done
fixture_version=1.2.3
legal_files='LICENSE EXTRA-NOTICE'
legal_release=${work}/legal-inventory-release
make_release "${legal_release}" "printf '%s\\n' '${command_name} 1.2.3'"
download_base=${legal_release}
installer=${small_legal_installer}
run_installer /bin/sh --install-dir "${fixture_home}/update-tools" --no-modify-path; expect_exit 0 'smaller legal inventory install for update'
run_installer /bin/sh --install-dir "${fixture_home}/remove-tools" --no-modify-path; expect_exit 0 'smaller legal inventory install for removal'
receipt=${fixture_home}/update-tools/.${command_name}-install
assert 'smaller receipt owns its actual LICENSE file' test -f "${receipt}/legal/LICENSE"
assert 'smaller receipt does not claim future legal file' test ! -e "${receipt}/legal/EXTRA-NOTICE"
installer=${expanded_legal_installer}
run_installer /bin/sh --install-dir "${fixture_home}/remove-tools" --uninstall --no-modify-path; expect_exit 0 'expanded requirements can uninstall smaller verified inventory'
assert 'smaller-inventory uninstall removes actual owned state' test ! -e "${fixture_home}/remove-tools/.${command_name}-install"
run_installer /bin/sh --install-dir "${fixture_home}/update-tools" --no-modify-path; expect_exit 0 'expanded package upgrades smaller verified inventory'
assert 'new receipt contains current added legal requirement' test -f "${receipt}/legal/EXTRA-NOTICE"
assert 'new receipt seals current added legal requirement' grep -q '  legal/EXTRA-NOTICE$' "${receipt}/state.sha256"
before=$(hash_file "${fixture_home}/update-tools/${command_name}")
state_before=$(hash_file "${receipt}/state.sha256")
cp "${receipt}/legal/LICENSE" "${work}/owned-license-before"
rm "${receipt}/legal/LICENSE"
run_installer /bin/sh --install-dir "${fixture_home}/update-tools" --no-modify-path; expect_failure 'missing previously owned legal file is not treated as a future requirement'
after=$(hash_file "${fixture_home}/update-tools/${command_name}"); assert 'missing owned legal file preserves binary' test "${before}" = "${after}"
state_after=$(hash_file "${receipt}/state.sha256"); assert 'missing owned legal file preserves receipt' test "${state_before}" = "${state_after}"
cp "${work}/owned-license-before" "${receipt}/legal/LICENSE"
printf changed >> "${receipt}/legal/LICENSE"
changed_legal_hash=$(hash_file "${receipt}/legal/LICENSE")
run_installer /bin/sh --install-dir "${fixture_home}/update-tools" --uninstall --no-modify-path; expect_failure 'changed owned legal file refuses cleanup'
after=$(hash_file "${receipt}/legal/LICENSE"); assert 'changed legal bytes retained' test "${changed_legal_hash}" = "${after}"
cp "${work}/owned-license-before" "${receipt}/legal/LICENSE"
printf user-note > "${receipt}/legal/UNOWNED-NOTE"
run_installer /bin/sh --install-dir "${fixture_home}/update-tools" --uninstall --no-modify-path; expect_failure 'unrecorded legal sidecar refuses cleanup'
assert 'unrecorded legal sidecar retained' grep -qx user-note "${receipt}/legal/UNOWNED-NOTE"
state_after=$(hash_file "${receipt}/state.sha256"); assert 'extra legal file preserves receipt' test "${state_before}" = "${state_after}"
rm "${receipt}/legal/UNOWNED-NOTE"
run_installer /bin/sh --install-dir "${fixture_home}/update-tools" --uninstall --no-modify-path; expect_exit 0 'expanded legal inventory normal cleanup'
installer=${saved_installer}; legal_files=${saved_legal_files}

begin 'final locked inspection propagates a managed receipt read failure'
run_installer /bin/sh --no-modify-path; expect_exit 0 'final inspection baseline'
binary_path=${fixture_home}/.local/bin/${command_name}
receipt=${fixture_home}/.local/bin/.${command_name}-install
before=$(hash_file "${binary_path}"); state_before=$(hash_file "${receipt}/state.sha256")
mkdir "${fixture_home}/inspection-tools"
real_cat=$(command -v cat)
cat > "${fixture_home}/inspection-tools/cat" <<EOF
#!/bin/sh
case "\$1" in */.${command_name}-install/version)
    for pending in "\$HOME/.local/bin/.${command_name}-stage."*; do
        if [ -d "\$pending" ]; then printf 'injected receipt read failure\\n' >&2; exit 75; fi
    done ;;
*) : ;;
esac
exec '${real_cat}' "\$@"
EOF
chmod 755 "${fixture_home}/inspection-tools/cat"
fixture_path=${fixture_home}/inspection-tools:${base_path}
run_installer /bin/sh --no-modify-path; expect_failure 'locked inspection read failure'
after=$(hash_file "${binary_path}"); state_after=$(hash_file "${receipt}/state.sha256")
assert 'read failure preserves active executable' test "${before}" = "${after}"
assert 'read failure preserves active ownership' test "${state_before}" = "${state_after}"
assert 'read failure exposes the actual cause' grep -q 'injected receipt read failure' "${work}/out"
assert 'read failure releases the owned lock' test ! -e "${fixture_home}/.local/bin/.${command_name}-install.lock"
unset fixture_path

begin 'PATH append read errors never become false ownership'
printf '# original startup\n' > "${fixture_home}/.profile"
profile_before=$(hash_file "${fixture_home}/.profile")
mkdir "${fixture_home}/append-read-tools"
real_grep=$(command -v grep)
cat > "${fixture_home}/append-read-tools/grep" <<EOF
#!/bin/sh
if [ "\$1" = -Fxq ]; then
    for pending in "\$HOME/.local/bin/.${command_name}-stage."*/new-state/state.sha256; do
        if [ -f "\$pending" ]; then printf 'injected append read failure\\n' >&2; exit 2; fi
    done
fi
exec '${real_grep}' "\$@"
EOF
chmod 755 "${fixture_home}/append-read-tools/grep"
fixture_path=${fixture_home}/append-read-tools:${base_path}
run_installer /bin/sh; expect_failure 'append boundary read error'
after=$(hash_file "${fixture_home}/.profile")
assert 'append read failure leaves startup bytes unchanged' test "${profile_before}" = "${after}"
assert 'append read failure creates no active ownership' test ! -e "${fixture_home}/.local/bin/.${command_name}-install"
assert 'append read failure creates no active program' test ! -e "${fixture_home}/.local/bin/${command_name}"
assert 'append read failure exposes the actual cause' grep -q 'Could not recheck startup file' "${work}/out"
unset fixture_path

printf '\n'
if [[ "${failures}" -gt 0 ]]; then printf '%s check(s) failed.\n' "${failures}" >&2; exit 1; fi
printf 'All installer checks passed.\n'
