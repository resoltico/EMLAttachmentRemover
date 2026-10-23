#!/usr/bin/env bash
# Finite, exclusively owned native filesystem profiles; never format an existing device.
set -euo pipefail
oracle=$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")" && pwd)/check_name_profile.py
if [[ "$1" = --profile || "$1" = --existing-profile ]]; then
  mode=$1
  binary=$2
  directory=$3
  profile=$4
  uid=$(id -u)
  gid=$(id -g)
  mounted=false
  overlay_mounted=false
  status=0
  cleanup_target=
  cleanup_observation_status=0
  cleanup_complete=true
  cleanup_residual=false
  operation_exit_code=0
  trap '
  status=$?
  operation_exit_code=${status}
  trap - EXIT
  if command -v findmnt >/dev/null; then
    for cleanup_target in "${directory}/merged" "${directory}/mount"; do
      if findmnt --json --mountpoint "${cleanup_target}" >> "${directory}/mounts-before-cleanup.jsonl" 2>> "${directory}/cleanup.stderr"; then
        :
      else
        cleanup_observation_status=$?
        if [[ "${cleanup_observation_status}" -ne 1 ]]; then cleanup_complete=false; status=1; fi
      fi
    done
  else
    cleanup_complete=false
    status=1
  fi
  if "${overlay_mounted}"; then
    if sudo -n umount -- "${directory}/merged"; then
      overlay_mounted=false
    else
      printf "%s\n" "Owned overlay did not settle; backing mount retained." >&2
      status=1
    fi
  fi
  if "${mounted}" && ! "${overlay_mounted}"; then
    if sudo -n umount -- "${directory}/mount"; then
      mounted=false
    else
      printf "%s\n" "Owned mount did not settle; no forced/lazy detach attempted." >&2
      status=1
    fi
  fi
  if command -v findmnt >/dev/null; then
    for cleanup_target in "${directory}/merged" "${directory}/mount"; do
      if findmnt --json --mountpoint "${cleanup_target}" >> "${directory}/mounts-after-cleanup.jsonl" 2>> "${directory}/cleanup.stderr"; then
        printf "%s\n" "Residual mount at owned target: ${cleanup_target}" >&2
        status=1
        cleanup_residual=true
      else
        cleanup_observation_status=$?
        if [[ "${cleanup_observation_status}" -ne 1 ]]; then
          printf "%s\n" "Owned mount cleanup state could not be observed." >&2
          status=1
          cleanup_complete=false
        fi
      fi
    done
  fi
  if "${mounted}" || "${overlay_mounted}"; then cleanup_residual=true; fi
  python3 -c "import json,sys; d=sys.argv[1]; complete=sys.argv[2]==\"true\"; residual=sys.argv[3]==\"true\"; open(d+\"/cleanup.json\",\"w\").write(json.dumps(dict(detached=complete and not residual,residual_mounts=residual,observation_complete=complete,forced_detach=False,operation_exit_code=int(sys.argv[4])))+\"\\n\")" "${directory}" "${cleanup_complete}" "${cleanup_residual}" "${operation_exit_code}"
  printf "%s\n" "${status}" > "${directory}/exit-code"
  exit "${status}"
  ' EXIT
  if [[ "${directory}" = *","* || "${directory}" = *":"* || "${directory}" = *$'\n'* ]]; then
    printf "%s\n" "Owned mount path contains an overlay option delimiter." >&2
    exit 2
  fi
  sudo -n true
  case "${profile}" in
    tmpfs)
      sudo -n mount -t tmpfs -o size=64m,mode=0700 tmpfs "${directory}/mount"
      ;;
    *)
      size=128M
      case "${profile}" in xfs*) size=512M ;; btrfs) size=256M ;; *) ;; esac
      truncate -s "${size}" "${directory}/owned.img"
      case "${profile}" in
        ordinary-ext4-existing-only|overlay-existing-only) mkfs.ext4 -q -F "${directory}/owned.img" ;;
        ext2|ext3|ext4) "mkfs.${profile}" -q -F "${directory}/owned.img" ;;
        ext4-casefold) mkfs.ext4 -q -F -O casefold -E encoding=utf8 "${directory}/owned.img" ;;
        xfs) mkfs.xfs -f "${directory}/owned.img" ;;
        xfs-ascii-ci) mkfs.xfs -f -m crc=0 -n version=ci "${directory}/owned.img" ;;
        btrfs) mkfs.btrfs -q -f "${directory}/owned.img" ;;
        *) exit 2 ;;
      esac
      sudo -n mount -o loop "${directory}/owned.img" "${directory}/mount"
      ;;
  esac
  mounted=true
  endpoint=${directory}/mount
  if [[ "${profile}" = overlay-existing-only ]]; then
    sudo -n mkdir -- "${directory}/mount/lower" "${directory}/mount/upper" "${directory}/mount/work"
    sudo -n mount -t overlay overlay -o "lowerdir=${directory}/mount/lower,upperdir=${directory}/mount/upper,workdir=${directory}/mount/work" "${directory}/merged"
    overlay_mounted=true
    endpoint=${directory}/merged
  fi
  sudo -n chown "${uid}:${gid}" "${endpoint}"
  chmod 0700 "${endpoint}"
  sudo -n mkdir -- "${endpoint}/copies"
  sudo -n chown "${uid}:${gid}" "${endpoint}/copies"
  chmod 0700 "${endpoint}/copies"
  if [[ "${profile}" = ext4-casefold ]]; then
    chattr +F "${endpoint}/copies"
    lsattr -d "${endpoint}/copies" > "${directory}/casefold.txt"
  fi
  findmnt --json --target "${endpoint}/copies" > "${directory}/mount.json"
  stat -f -- "${endpoint}/copies" > "${directory}/filesystem.txt"
  if [[ "${mode}" = --existing-profile ]]; then
    existing_oracle=$(dirname -- "${oracle}")/check_existing_profile.py
    if [[ "${profile}" = ordinary-ext4-existing-only ]]; then
      python3 "${existing_oracle}" --require-sync-success --binary "${binary}" --source-root "${directory}/sources" --destination "${endpoint}/copies" --out "${directory}/behavior"
    else
      python3 "${existing_oracle}" --refuse-new-names --binary "${binary}" --source-root "${directory}/sources" --destination "${endpoint}/copies" --out "${directory}/behavior"
    fi
    exit 0
  fi
  python3 "${oracle}" --binary "${binary}" --source-root "${directory}/sources" --destination "${endpoint}/copies" --out "${directory}/behavior"
  exit 0
fi
binary=$1
out=$2
mkdir -- "${out}"
out=$(cd -- "${out}" && pwd)
profiles=(ext2 ext3 ext4 ext4-casefold tmpfs xfs xfs-ascii-ci btrfs)
failed=0
for profile in "${profiles[@]}"; do
  directory=${out}/${profile}
  mkdir -- "${directory}" "${directory}/mount"
  if bash "${BASH_SOURCE[0]}" --profile "${binary}" "${directory}" "${profile}" > "${directory}/campaign.log" 2>&1; then
    printf '%s\tpassed\n' "${profile}" >> "${out}/profiles.tsv"
  else
    printf '%s\tfailed_or_unavailable\n' "${profile}" >> "${out}/profiles.tsv"
    failed=1
  fi
done
existing_profiles=(ordinary-ext4-existing-only overlay-existing-only)
for profile in "${existing_profiles[@]}"; do
  directory=${out}/${profile}
  mkdir -- "${directory}" "${directory}/mount" "${directory}/merged"
  if bash "${BASH_SOURCE[0]}" --existing-profile "${binary}" "${directory}" "${profile}" > "${directory}/campaign.log" 2>&1; then
    printf '%s\tpassed\n' "${profile}" >> "${out}/existing-profiles.tsv"
  else
    printf '%s\tfailed_or_unavailable\n' "${profile}" >> "${out}/existing-profiles.tsv"
    failed=1
  fi
done
python3 - "${out}" "${binary}" <<'PY'
import hashlib,json,platform,sys
from pathlib import Path
out=Path(sys.argv[1])
def records(filename):
    profiles=[]
    for row in (out/filename).read_text().splitlines():
        name,status=row.split('\t');p=out/name
        behavior=json.loads((p/'behavior/result.json').read_text()) if (p/'behavior/result.json').is_file() else None
        profiles.append({'profile':name,'status':status,'exit_code':int((p/'exit-code').read_text()),'behavior':behavior,'log':str(p/'campaign.log'),'mount':str(p/'mount.json'),'filesystem':str(p/'filesystem.txt'),'mount_record':json.loads((p/'mount.json').read_text()) if (p/'mount.json').is_file() else None,'filesystem_observation':(p/'filesystem.txt').read_text() if (p/'filesystem.txt').is_file() else None,'cleanup':json.loads((p/'cleanup.json').read_text()) if (p/'cleanup.json').is_file() else None})
    return profiles
profiles=records('profiles.tsv');existing=records('existing-profiles.tsv')
record={'schema':1,'host':platform.uname()._asdict(),'binary_sha256':hashlib.sha256(Path(sys.argv[2]).read_bytes()).hexdigest(),'profiles':profiles,'existing_profiles':existing,'passed':all(p['status']=='passed' and p['behavior'] and p['behavior']['passed'] for p in profiles+existing),'scope':'Actual finite owned loop/tmpfs naming and existing-only profiles; missing/failed profiles are unqualified. No native CPU or filesystem inference.'}
(out/'result.json').write_text(json.dumps(record,indent=2)+'\n')
PY
exit "${failed}"
