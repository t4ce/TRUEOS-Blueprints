#!/usr/bin/env bash
# W3Box native Ubuntu installer. Read README.md before running as root.
set -Eeuo pipefail
umask 027
HERE="$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")" && pwd)"
PREFIX=/opt/w3box
DATA=/var/lib/w3box
CACHE=/var/cache/w3box
LOG=/var/log/w3box-install.log
PIN=9cd173f4e02ba3d9f8f15a67ca308b5eb78723e4
REPO=https://github.com/pvpgn/pvpgn-server.git
ADDRESS=""; NO_APT=0; CHECK=0; JOBS=2
usage() {
  cat <<'EOF'
Usage: sudo bash install.sh [--address IPV4_OR_HOSTNAME] [--jobs 1..8] [--no-apt] [--check]

Default: install official Ubuntu dependencies, fetch one pinned PvPGN commit,
build natively, install an unprivileged systemd service and a localhost-only UI.
--address  Address Windows players will use; otherwise asks with a LAN-IP default.
--jobs     Parallel build jobs (default 2; use 1 on memory-constrained systems).
--no-apt   Skip package downloads; fail if required packages are not installed.
--check    Basic host/network preflight only; do not install anything.

Requires normal Ubuntu 22.04 or newer with a running systemd, sudo/root,
working Ubuntu APT repositories and HTTPS access to GitHub on first install.
This is not a fully offline distribution and does not include a Windows loader.
EOF
}
while (($#)); do
  case "$1" in
    --address) [[ $# -ge 2 ]] || { usage; exit 2; }; ADDRESS="$2"; shift 2;;
    --jobs) [[ $# -ge 2 ]] || { usage; exit 2; }; JOBS="$2"; shift 2;;
    --no-apt) NO_APT=1; shift;;
    --check) CHECK=1; shift;;
    -h|--help) usage; exit 0;;
    *) echo "Unknown option: $1" >&2; usage; exit 2;;
  esac
done
[[ "$JOBS" =~ ^[1-8]$ ]] || { echo '--jobs must be 1–8.' >&2; exit 2; }
[[ -r /etc/os-release ]] || { echo 'Cannot identify this Linux distribution.' >&2; exit 1; }
# shellcheck disable=SC1091
. /etc/os-release
[[ "${ID:-}" == ubuntu ]] || { echo 'This installer targets Ubuntu, not this distribution. No changes made.' >&2; exit 1; }
dpkg --compare-versions "${VERSION_ID:-0}" ge 22.04 || { echo 'Ubuntu 22.04 or newer is required.' >&2; exit 1; }
[[ -d /run/systemd/system ]] || { echo 'A running systemd is required. Use a normal Ubuntu host/VM; an unbooted container is not sufficient.' >&2; exit 1; }
if ((CHECK)); then
  echo "Host: ${PRETTY_NAME}; architecture: $(dpkg --print-architecture)"
  command -v apt-get systemctl ip >/dev/null
  echo 'Checking github.com DNS (not a complete download/build test):'
  getent ahostsv4 github.com
  echo 'Disk space:'; df -h /var /opt
  if command -v git >/dev/null; then
    echo 'Checking repository access over HTTPS:'
    GIT_TERMINAL_PROMPT=0 timeout 45 git -c http.sslVerify=true ls-remote "$REPO" HEAD
  else
    echo 'Git is not installed yet; the full installer will install it.'
  fi
  echo 'Preflight finished. This does not validate APT downloads, compilation, or a Warcraft login.'
  exit 0
fi
[[ $EUID -eq 0 ]] || { echo 'Run with sudo: sudo bash install.sh' >&2; exit 1; }
exec 9>/run/lock/w3box-install.lock
flock -n 9 || { echo 'Another W3Box installer is running.' >&2; exit 1; }
touch "$LOG"; chmod 600 "$LOG"
printf '\n=== W3Box install %s | %s ===\n' "$(date -Is)" "$PRETTY_NAME" >>"$LOG"
failed() {
  local rc=$? line=$1
  trap - ERR
  echo >&2
  echo "Installation stopped at line $line (exit $rc). It has NOT been declared successful." >&2
  echo "Full log: $LOG" >&2
  tail -n 65 "$LOG" >&2 || true
  echo 'Fix the reported failure and re-run this installer. Existing account data is preserved.' >&2
  exit "$rc"
}
trap 'failed "$LINENO"' ERR
say() { printf '\n==> %s\n' "$*"; printf '\n==> %s\n' "$*" >>"$LOG"; }
logged() { "$@" >>"$LOG" 2>&1; }
retry() {
  local n
  for n in 1 2 3; do
    if "$@" >>"$LOG" 2>&1; then return 0; fi
    echo "Attempt $n failed; details are in $LOG."
    ((n == 3)) || sleep $((n * 2))
  done
  return 1
}
AVAIL=$(df -Pk /var | awk 'NR==2 {print $4}')
[[ "$AVAIL" -ge 1048576 ]] || { echo 'At least 1 GiB of free space under /var is required.' >&2; exit 1; }
PACKAGES=(ca-certificates git build-essential cmake zlib1g-dev python3 logrotate)
if (( ! NO_APT )); then
  say 'Installing dependencies from your configured Ubuntu APT repositories'
  retry env DEBIAN_FRONTEND=noninteractive apt-get -o Acquire::Retries=3 -o DPkg::Lock::Timeout=180 -o APT::Update::Error-Mode=any update
  retry env DEBIAN_FRONTEND=noninteractive apt-get -o Acquire::Retries=3 -o DPkg::Lock::Timeout=180 install -y --no-install-recommends "${PACKAGES[@]}"
fi
for package in "${PACKAGES[@]}"; do
  STATUS=$(dpkg-query -W -f='${Status}' "$package" 2>/dev/null || true)
  [[ "$STATUS" == 'install ok installed' ]] || { echo "Missing dependency: $package. Remove --no-apt or fix your APT repositories." >&2; exit 1; }
done
if [[ -z "$ADDRESS" ]]; then
  ADDRESS=$(ip -4 route get 1.1.1.1 2>/dev/null | awk '{for(i=1;i<=NF;i++) if($i=="src") {print $(i+1); exit}}') || true
  ADDRESS=${ADDRESS:-127.0.0.1}
  if [[ -t 0 ]]; then
    read -r -p "Address Windows players will use [$ADDRESS]: " ENTERED
    ADDRESS=${ENTERED:-$ADDRESS}
  fi
fi
PYTHONPATH="$HERE/app" python3 - "$ADDRESS" <<'PY'
import sys
from common import valid_host
if not valid_host(sys.argv[1]):
    raise SystemExit('Invalid server address. Use an IPv4 address or DNS hostname; no URL or port.')
PY
say 'Preparing the unprivileged service account and build cache'
if getent passwd w3box >/dev/null; then
  [[ "$(getent passwd w3box | cut -d: -f6)" == "$DATA" && "$(id -u w3box)" != 0 ]] || {
    echo 'An unrelated w3box account already exists. Refusing to change it.' >&2; exit 1;
  }
else
  if getent group w3box >/dev/null; then
    logged useradd --system --home-dir "$DATA" --shell /usr/sbin/nologin --gid w3box w3box
  else
    logged useradd --system --home-dir "$DATA" --shell /usr/sbin/nologin --user-group w3box
  fi
fi
install -d -o w3box -g w3box -m 0750 "$DATA" "$CACHE"
SRC="$CACHE/source"; BUILD="$CACHE/build"; STAGE="$CACHE/stage"
if [[ ! -d "$SRC/.git" ]]; then logged runuser -u w3box -- git init "$SRC"; fi
if ! runuser -u w3box -- git -C "$SRC" remote get-url origin >/dev/null 2>&1; then
  logged runuser -u w3box -- git -C "$SRC" remote add origin "$REPO"
else
  logged runuser -u w3box -- git -C "$SRC" remote set-url origin "$REPO"
fi
say "Obtaining pinned PvPGN source: $PIN"
if ! runuser -u w3box -- git -C "$SRC" cat-file -e "$PIN^{commit}" 2>/dev/null; then
  retry runuser -u w3box -- env GIT_TERMINAL_PROMPT=0 git -c http.sslVerify=true -c http.version=HTTP/1.1 \
    -c http.lowSpeedLimit=1024 -c http.lowSpeedTime=60 -C "$SRC" fetch --depth=1 origin "$PIN"
fi
logged runuser -u w3box -- git -c core.hooksPath=/dev/null -C "$SRC" checkout --detach --force "$PIN"
logged runuser -u w3box -- git -C "$SRC" clean -fdx
ACTUAL=$(runuser -u w3box -- git -C "$SRC" rev-parse HEAD)
[[ "$ACTUAL" == "$PIN" ]] || { echo 'Source commit mismatch. Refusing to build.' >&2; exit 1; }
logged runuser -u w3box -- git -C "$SRC" fsck --no-reflogs --connectivity-only
say "Building native PvPGN with $JOBS parallel job(s)"
# Upstream forces its own config/data paths under CMAKE_INSTALL_PREFIX.
# We honor those paths and link them to persistent state after staging.
logged runuser -u w3box -- cmake -S "$SRC" -B "$BUILD" \
  -DCMAKE_INSTALL_PREFIX="$PREFIX/pvpgn" -DCMAKE_BUILD_TYPE=Release \
  -DCMAKE_POLICY_VERSION_MINIMUM=3.5 \
  -DWITH_BNETD=ON -DWITH_D2CS=OFF -DWITH_D2DBS=OFF \
  -DWITH_LUA=OFF -DWITH_MYSQL=OFF -DWITH_SQLITE3=OFF -DWITH_PGSQL=OFF -DWITH_ODBC=OFF
logged runuser -u w3box -- cmake --build "$BUILD" --parallel "$JOBS"
rm -rf -- "$STAGE"
install -d -o w3box -g w3box -m 0750 "$STAGE"
logged runuser -u w3box -- env DESTDIR="$STAGE" cmake --install "$BUILD"
BUILT="$STAGE$PREFIX/pvpgn"
test -x "$BUILT/sbin/bnetd"
test -s "$BUILT/etc/pvpgn/bnetd.conf"
test -s "$BUILT/etc/pvpgn/versioncheck.json"
test -s "$BUILT/var/pvpgn/files/IX86ver1.mpq"
test -s "$BUILT/var/pvpgn/files/icons-WAR3.bni"
logged runuser -u w3box -- "$BUILT/sbin/bnetd" -v
say 'Installing services and preserving existing account data'
if systemctl is-active --quiet w3box; then logged systemctl stop w3box; fi
install -d -m 0755 "$PREFIX/app" "$PREFIX/templates" "$PREFIX/pvpgn" "$PREFIX/pvpgn/etc" "$PREFIX/pvpgn/var"
chmod 0755 "$PREFIX"
cp -a "$HERE/app/." "$PREFIX/app/"
for directory in bin sbin share; do
  if [[ -d "$BUILT/$directory" ]]; then
    install -d -m 0755 "$PREFIX/pvpgn/$directory"
    cp -a "$BUILT/$directory/." "$PREFIX/pvpgn/$directory/"
  fi
done
# Copy only files not already present; never overwrite saved user accounts.
python3 - "$BUILT" "$DATA" <<'PY'
import os, shutil, sys
from pathlib import Path
built, data = map(Path, sys.argv[1:])
for source, target in ((built/'etc/pvpgn', data/'conf'), (built/'var/pvpgn', data/'var')):
    for root, directories, files in os.walk(source):
        destination = target / Path(root).relative_to(source)
        destination.mkdir(parents=True, exist_ok=True)
        for name in files:
            output = destination / name
            if not output.exists(): shutil.copy2(Path(root)/name, output)
PY
for pair in "etc:conf" "var:var"; do
  PART=${pair%%:*}; TARGET=${pair##*:}; LINK="$PREFIX/pvpgn/$PART/pvpgn"
  if [[ -e "$LINK" && ! -L "$LINK" ]]; then
    echo "Unexpected real directory at $LINK. Refusing to replace it." >&2; exit 1
  fi
  ln -sfn "$DATA/$TARGET" "$LINK"
done
install -m 0644 "$BUILT/etc/pvpgn/bnetd.conf" "$PREFIX/templates/bnetd.base.conf"
install -m 0644 "$BUILT/etc/pvpgn/versioncheck.json" "$PREFIX/templates/versioncheck.upstream.json"
printf '%s\n' "$PIN" >"$PREFIX/SOURCE_COMMIT"
install -m 0644 "$SRC/LICENSE" "$PREFIX/PVPGN-LICENSE"
install -m 0644 "$HERE/README.md" "$PREFIX/README.md"
chown -R root:root "$PREFIX/app" "$PREFIX/templates"
for directory in bin sbin share; do
  if [[ -d "$PREFIX/pvpgn/$directory" ]]; then chown -R root:root "$PREFIX/pvpgn/$directory"; fi
done
find "$PREFIX/app" "$PREFIX/templates" -type d -exec chmod 755 {} +
find "$PREFIX/app" "$PREFIX/templates" -type f -exec chmod 644 {} +
chmod 755 "$PREFIX/app/cli.py"
chown -R w3box:w3box "$DATA"
chmod 750 "$DATA"
# Capture the generated secret separately so it never lands in the install log.
PASSWORD=$(runuser -u w3box -- python3 "$PREFIX/app/init_config.py" --address "$ADDRESS")
if [[ -n "$PASSWORD" ]]; then
  (umask 077; printf 'W3Box panel: http://127.0.0.1:8787\nInitial password: %s\nReset: sudo w3ctl password\n' "$PASSWORD" > /root/w3box-credentials.txt)
fi
ln -sfn "$PREFIX/app/cli.py" /usr/local/bin/w3ctl
install -m 0644 "$HERE/w3box.service" /etc/systemd/system/w3box.service
install -m 0644 "$HERE/w3box.logrotate" /etc/logrotate.d/w3box
logged systemctl daemon-reload
logged systemctl enable --now w3box
say 'Checking the supervisor and the actual PvPGN login listener'
READY=0
for _ in $(seq 1 35); do
  if /usr/local/bin/w3ctl status --json >"$CACHE/status.json" 2>>"$LOG"; then
    if python3 - "$CACHE/status.json" <<'PY'
import json, sys
s=json.load(open(sys.argv[1]))
# Respect an intentionally stopped realm when reinstalling.
ok=s['ready'] or (not s['desired_running'] and not s['last_error'])
sys.exit(0 if ok else 1)
PY
    then READY=1; break; fi
  fi
  sleep 1
done
if (( ! READY )); then
  journalctl -u w3box -n 60 --no-pager >>"$LOG" 2>&1 || true
  /usr/local/bin/w3ctl logs >>"$LOG" 2>&1 || true
  echo 'The service did not become ready. No successful installation is claimed.' >&2
  false
fi
printf '\nW3Box installed.\n'
/usr/local/bin/w3ctl status
printf '\nPanel: http://127.0.0.1:8787\n'
if [[ -n "$PASSWORD" ]]; then
  printf 'Panel password: %s\nSaved privately in /root/w3box-credentials.txt\n' "$PASSWORD"
else
  printf 'Your existing panel password was preserved. Reset: sudo w3ctl password\n'
fi
printf '\nRemote administration: run this on your OWN computer, then open the panel URL:\n'
printf '  ssh -N -L 8787:127.0.0.1:8787 YOUR_USER@%s\n' "$ADDRESS"
printf '\nCLI: sudo w3ctl status | logs | doctor | start | stop | restart | backup\n'
printf 'No router, cloud firewall or UFW rules were changed. Never expose port 8787.\n'
printf 'IMPORTANT: a trusted pre-1.22a-compatible Windows loader is still required for RoC 1.21b.\n'
printf 'See %s/README.md for client setup, networking and troubleshooting.\n' "$PREFIX"
