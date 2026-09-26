# W3Box — RoC 1.21b private-realm kit

A native Ubuntu installer, Python web control panel, and CLI for a private
PvPGN Warcraft III: Reign of Chaos realm. This kit targets the Windows **1.21b**
client, not Reforged, The Frozen Throne, or RoC 1.00.

**Read this first:** this is an online setup kit, not an offline server image.
The archive contains the installer and control software. At installation it
fetches a pinned PvPGN source revision and installs compiler dependencies from
your Ubuntu repositories. It does not contain a compiled PvPGN binary, Warcraft
files, CD keys, or a verified 1.21b-compatible Windows loader. Native compilation
and an actual Warcraft login were **not tested** in the build environment. The
included control-software tests use a simulated executable. See TEST-REPORT.md.

PvPGN upstream explicitly lists RoC 1.21b as supported, but the Windows client
requires a compatible modification for its server-signature check. The modern
`w3lh/w3l` README excludes versions below 1.22a and points earlier clients to
ACiD loader v1.2. A compatible legacy binary was not located and verified for
this kit. Do not substitute the modern loader and expect it to work. [1, 2]

## Install

Use a normal Ubuntu host or VM, with Ubuntu 22.04 or newer, running systemd,
sudo/root access, and working APT and GitHub connectivity. The installer builds
for the host architecture; no architecture-specific binary is bundled. No
Ubuntu release or architecture has been end-to-end tested for this kit. It
requires at least 1 GiB free under /var and defaults to two build jobs; this is a
preflight threshold, not a guarantee of sufficient memory or disk for every host.

With the ZIP in your current directory:

```sh
unzip w3box-roc121b.zip
sudo bash w3box/install.sh
```

The installer asks which IPv4 address or hostname your Windows PCs will use,
with a detected local address as its default. For an unattended installation:

```sh
sudo bash w3box/install.sh --address 192.168.1.50
```

Replace the example address. Use your LAN address for PCs on the same LAN,
your VPN address for VPN players, or a correctly routed public address/hostname
for Internet players. A hostname alone does not configure your router or NAT.

If `unzip` is not installed, the companion tar.gz contains the same files and
can be extracted instead:

```sh
tar -xzf w3box-roc121b.tar.gz
sudo bash w3box/install.sh
```

No Docker, Node.js, pip packages, third-party package repository, or SQL server
is required. The Python panel uses only the standard library. The native server
uses PvPGN's flat-file account storage.

The installer installs `ca-certificates`, `git`, `build-essential`, `cmake`,
`zlib1g-dev`, `python3`, and `logrotate` from your configured Ubuntu repositories.
It fetches only this PvPGN commit:

```
9cd173f4e02ba3d9f8f15a67ca308b5eb78723e4
```

It verifies the checked-out commit, builds as the unprivileged `w3box` user,
checks for required support files, installs the services, then requires the
actual child process to own its TCP 6112 listener before reporting readiness.
This listener check is not an in-game authentication test. PvPGN's source tree
already includes the support files installed by its CMake rules, including
`IX86ver1.mpq`; there is no separate legacy support-file download URL. [3–5]

The first installation still needs Ubuntu APT and HTTPS Git access to GitHub.
A GitHub login or access token is not required for the public repository. TLS
verification is not disabled. Package and Git requests have retries. Downloads
are not guaranteed to succeed on a network with broken DNS, a proxy, blocked
GitHub, unavailable packages, or a repository outage. Failures stop the
installer and point to `/var/log/w3box-install.log`; no success is fabricated.

## Open the web panel

On the Ubuntu computer, open:

```
http://127.0.0.1:8787
```

The installer prints a random panel password and saves the initial password in
`/root/w3box-credentials.txt` (root only). A password reset does not update that
initial-password note. The panel has a single administrator password; it does
not use your Warcraft game account.

From another computer, use an SSH tunnel rather than exposing the panel:

```sh
ssh -N -L 8787:127.0.0.1:8787 YOUR_USER@YOUR_UBUNTU_SERVER
```

Keep that SSH session open, then use `http://127.0.0.1:8787` in your local
browser. An SSH server on Ubuntu and permission to connect must already exist;
the installer does not enable remote SSH access for you. Keep the local port
8787 unchanged because the panel validates the browser Host header.

The web interface provides start, stop and restart, listener/process status,
uptime, an account-file count, recent logs, configuration, local diagnostics,
private backups, and a generated Windows gateway helper. The account-file
count is **not** a live player count. There is no arbitrary shell console,
account editor, AI agent, map uploader, or hosting bot.

Saving configuration restarts a running realm. Stop, restart and backup actions
ask for confirmation because existing realm sessions disconnect. A backup
briefly stops the server to flush account state and resumes a previously
running server. No claim is made about uninterrupted in-progress matches.

## CLI controls

```sh
sudo w3ctl status
sudo w3ctl start
sudo w3ctl stop
sudo w3ctl restart
sudo w3ctl logs
sudo w3ctl doctor
sudo w3ctl backup
sudo w3ctl config
sudo w3ctl password
```

`doctor` reports local checks and returns a nonzero exit status for a failed
check. A deliberately stopped server fails its running/listener checks. It
cannot validate an external firewall, port forwarding, your Windows loader,
or an actual game session.

Examples of managed configuration changes:

```sh
sudo w3ctl set server_name 'Friends RoC Realm'
sudo w3ctl set server_address 192.168.1.50
sudo w3ctl set advertise_ip 192.168.1.50
sudo w3ctl set max_users 32
sudo w3ctl set new_accounts false
```

`server_address` is the address put in the client gateway helper.
`advertise_ip` is a separate IPv4 address for PvPGN's 6200 route service. Set it
to the correct reachable address when moving between LAN/public/VPN setups;
changing `server_address` alone does not change `advertise_ip`. An empty value
uses upstream automatic route handling:

```sh
sudo w3ctl set advertise_ip ''
```

`bind_address` defaults to `0.0.0.0` (all IPv4 interfaces). Only bind to an address
that actually exists on your Ubuntu host. Binding to a public IP that exists
only on your router will fail. The game realm is reachable on the bound
interfaces subject to your firewall; the web panel remains loopback-only.

Use `--json` for scriptable results, for example `sudo w3ctl status --json`.
The CLI communicates over a restricted local Unix socket. It cannot connect
from another machine; run it over SSH instead.

## Connect Windows RoC 1.21b

1. Use your own Warcraft III Reign of Chaos installation patched to **1.21b**.
   Back up the game folder before making client changes. All players should
   use the same compatible game version. This realm does not provide patches.
2. Supply a trusted loader specifically compatible with versions before 1.22a.
   A gateway change is not enough to bypass the client's server-signature
   check. Modern `w3lh/w3l` explicitly excludes this version; its documentation
   names ACiD loader v1.2 for earlier clients. No legacy executable is bundled,
   downloaded, or claimed safe/verified. Do not disable antivirus or execute an
   unknown download merely to get past this step. [1, 2]
3. Close Warcraft. Download the address-filled PowerShell gateway helper from
   the panel's **Connect a client** tab. Run it as your normal Windows user:

   ```powershell
   powershell.exe -NoProfile -File .\add-gateway.ps1
   ```

   The generic copy in `client/` asks for the server address. The helper changes
   only the current user's Warcraft III `Battle.net Gateways` value, appends or
   updates the private gateway, and first saves an exact backup to your Windows
   user directory. It neither patches the game nor installs a loader. If local
   execution policy blocks unsigned scripts, review the source and use your
   organization's approved process or a trusted gateway editor; this kit does
   not change system-wide policy. The script has not been run on Windows. [6]
4. Start Warcraft through your compatible loader, in Reign of Chaos mode.
   Select your private gateway and create a new **game** account there. The
   realm permits in-game registrations by default. You can close registrations
   afterward through the panel or `sudo w3ctl set new_accounts false`.

The optional `client/launch-roc.cmd` simply calls an existing `w3l.exe` with
`-classic -window` from the game folder. It does not install anything. Its
switches and your chosen legacy loader have not been Windows-tested; use your
loader's documented RoC launch method if those switches do not apply.

To undo the gateway change, with Warcraft closed:

```powershell
powershell.exe -NoProfile -File .\add-gateway.ps1 -Restore 'C:\Users\YOU\w3box-gateways-TIMESTAMP.json'
```

## Network and hosting model

PvPGN provides a private login/chat/game-listing service; this kit is not a
headless Warcraft simulation server. A player's game client still hosts custom
matches. A server-side install does not make every player's host reachable
through NAT. Upstream documents separate route and player-address translation
rules for these cases. [7]

| Port on Ubuntu | Purpose |
| --- | --- |
| TCP 6112 | PvPGN login, chat and game listings |
| UDP 6112 | Legacy client reachability/protocol traffic |
| TCP 6200 | Warcraft Play Game / arranged-team route service |
| TCP 8787 | Admin panel, bound only to 127.0.0.1; do not forward publicly |

The installer does **not** change UFW, cloud security groups, router forwarding,
or SSH settings. Allow only the intended players or VPN subnet through your
firewall. For a LAN or friends-only realm, a private network is the intended
starting point; do not treat this legacy-protocol software as audited public
infrastructure.

The custom-game host needs its own game port reachable by other players,
usually TCP 6112 unless changed in Warcraft. This is separate from the realm
server's port. If server and a Windows host are behind the same router, a
single public IP:port cannot be forwarded to two machines: give the game host
a distinct port and configure forwarding accordingly. Merely opening ports on
Ubuntu does not solve client NAT, double NAT or carrier-grade NAT. [7]

The panel's advertised route address adds a marked rule to
`/var/lib/w3box/conf/address_translation.conf`. This is for the route service,
not an automatic fix for custom-game hosts. Advanced LAN-plus-Internet setups
may need exclusions and manual player-address translation. Manual rules outside
the `W3BOX ROUTE` block are retained. Refer to upstream examples before editing.

## Version policy

The kit reads the exact `WAR3_121B` entry and equation from the pinned upstream
`versioncheck.json`, retaining only Windows RoC 1.21b. Upstream identifies this
as `1.21.1.156`; that is its internal version, not a different patch. Strict
checksum checking is on, unknown versions are rejected, and automatic game
upgrades and public tracking are disabled. [4]

A different executable/checksum may be rejected even when its UI says 1.21b.
Check the PvPGN log before changing anything. For a trusted private test, the
checksum check can be relaxed explicitly:

```sh
sudo w3ctl set strict_version false
```

This sets `allow_bad_version=true`; it does not add other version entries or
turn off `allow_unknown_version=false`. It does not fix an incompatible loader
or protocol. Restore strict checking with `sudo w3ctl set strict_version true`.

## Files, service, and backups

| Path | Contents |
| --- | --- |
| `/opt/w3box/app` | Root-owned panel/supervisor/CLI code |
| `/opt/w3box/pvpgn` | Native PvPGN programs; upstream config/data path symlinks |
| `/opt/w3box/templates` | Root-owned generated upstream configuration templates |
| `/var/lib/w3box/settings.json` | Managed realm settings |
| `/var/lib/w3box/auth.json` | Salted panel password hash, mode 0600 |
| `/var/lib/w3box/state.json` | Desired running/stopped state, survives reboot |
| `/var/lib/w3box/conf` | PvPGN configuration |
| `/var/lib/w3box/var` | PvPGN users, state, files and logs |
| `/var/lib/w3box/backups` | Private account/configuration archives |
| `/var/cache/w3box` | Pinned source, native build and staging cache |
| `/var/log/w3box-install.log` | Root-only installation log |
| `/run/w3box/control.sock` | Restricted CLI socket |

The `w3box.service` systemd service starts the unprivileged Python supervisor.
It runs PvPGN as a foreground child, performs bounded crash recovery, and stops
retrying after repeated failures. A manual stop remains a stop across reboot;
`sudo w3ctl start` resumes the realm. The control panel stays available while
the realm is stopped.

Do not manually change the generated `bnetd.conf`, `versioncheck.json`,
`autoupdate.conf`, or `bnserver*.ini` files and expect those changes to survive:
managed files are rendered at startup and on settings changes. Advanced changes
to the root-owned template can be made by an administrator but are replaced by
reinstallation. Back up before making such changes.

Backups contain `conf/`, `var/`, `settings.json`, `state.json`, and `auth.json`.
They exclude current server/startup logs. They are not encrypted and contain
sensitive account data and password hashes. Keep them private. They do not
include Ubuntu packages or the build cache and are not full machine images.
There is no automated restore endpoint. For a restore, stop `w3box.service`,
keep a copy of the current data directory, inspect and extract **your own
trusted backup** into a separate empty directory, and replace the backed-up
items under `/var/lib/w3box`, retaining ownership `w3box:w3box`. Restart the
service and run `sudo w3ctl doctor`. Never extract an untrusted tar archive as
root. Restoring `auth.json` also restores the panel password from that backup.

## Troubleshooting

**Install failure:** inspect the precise command/error in
`sudo less /var/log/w3box-install.log`. The first failure stops installation.
Retrying preserves existing account files. Reinstallation builds first, then
replaces the installed programs; it is not a transactional automatic rollback.
On low-memory machines try `sudo bash w3box/install.sh --jobs 1`.

**Panel unavailable:** use `sudo systemctl status w3box --no-pager` and
`sudo journalctl -u w3box -n 100 --no-pager`. Access `127.0.0.1:8787`, not the
Ubuntu LAN address. For remote access, establish the SSH tunnel first. A local
port conflict or restrictive SSH forwarding policy needs to be resolved.

**Process running but not ready:** run `sudo w3ctl logs` and `sudo w3ctl doctor`.
Check port conflicts (`sudo ss -lntup`), missing support files, bind-address
mistakes, and permissions. The kit tests that the child owns the listener,
not just that something is using 6112.

**Login fails despite ready status:** verify Windows RoC 1.21b, your private
gateway, a genuinely compatible pre-1.22a loader, and the checksum result in
PvPGN's logs. Local readiness does not prove client compatibility.

**Can chat but cannot join a match:** this is distinct from server login. Check
the custom-game host's reachable port and NAT mapping, all clients' versions,
and the correct route/player translation rules. [7]

**Forgot the panel password:** run `sudo w3ctl password`. It prompts securely
and invalidates existing panel sessions. The service must be running for the
CLI. Game passwords and game-account administration are separate.

**Preflight without installing:** `bash w3box/install.sh --check` checks the
Ubuntu/systemd prerequisites, DNS, available tools, disk, and Git repository
access if Git is already installed. It does not prove that a full fetch, APT
installation or compilation will succeed. `--no-apt` is available only when all
listed dependencies already exist; it is not an offline installer mode.

## Remove or disable

To disable both the panel and game realm while preserving everything:

```sh
sudo systemctl disable --now w3box
```

To remove the installed launcher/unit after disabling:

```sh
sudo rm -f /usr/local/bin/w3ctl /etc/systemd/system/w3box.service /etc/logrotate.d/w3box
sudo systemctl daemon-reload
```

The data, application, build cache and service account remain deliberately.
Remove them only after making a private backup and confirming you no longer
need them. No destructive data-removal command is run by this kit.

## Tests and provenance

Run the included local control tests with:

```sh
cd w3box
python3 -m unittest discover -s tests -v
```

The fixtures briefly use localhost TCP/UDP 6112 and TCP 6200. Do not run them
alongside a live realm on the same host. They do not test Warcraft protocol or
native compilation. Full results and limitations are in TEST-REPORT.md.

References [1]–[7], including source paths and the pinned revision, are in
SOURCES.md. The independently written installer/control/helper files use the
included MIT license. PvPGN is a separate GPL-2.0-or-later upstream program;
installation retains its source in the cache and copies its license into
`/opt/w3box/PVPGN-LICENSE`. No Blizzard game client or verified legacy W3L binary
is redistributed here. This is an unofficial kit with no affiliation to
Blizzard or the PvPGN maintainers.
