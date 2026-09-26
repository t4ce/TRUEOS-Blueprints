# Source notes

Checked 2026-09-25 using public upstream documentation and repository text.
These are provenance links, not a claim that the installer's Git/APT downloads
were exercised from the packaging environment. Native network downloads were
blocked there. No Windows loader binary was verified.

1. **PvPGN-PRO README** — Linux build prerequisites, explicit RoC 1.21b support,
   client-side server-signature modification requirement, default tracking.
   https://github.com/pvpgn/pvpgn-server

2. **Modern W3L README** — explicitly excludes Warcraft III versions below
   1.22a and directs earlier clients to ACiD loader v1.2. This does not establish
   that any particular third-party executable is authentic or safe.
   https://github.com/w3lh/w3l/blob/master/README.md

3. **Pinned PvPGN source** — all server builds in this kit use this exact commit,
   not a moving branch or an unverified third-party binary.
   https://github.com/pvpgn/pvpgn-server/commit/9cd173f4e02ba3d9f8f15a67ca308b5eb78723e4
   https://github.com/pvpgn/pvpgn-server/blob/9cd173f4e02ba3d9f8f15a67ca308b5eb78723e4/CMakeLists.txt
   https://github.com/pvpgn/pvpgn-server/blob/9cd173f4e02ba3d9f8f15a67ca308b5eb78723e4/ConfigureChecks.cmake
   https://github.com/pvpgn/pvpgn-server/blob/9cd173f4e02ba3d9f8f15a67ca308b5eb78723e4/cmake/Modules/DefineInstallationPaths.cmake

4. **RoC 1.21b CheckRevision configuration** — exact versionTag WAR3_121B,
   version 1.21.1.156, hash 0x1b735294; installer preserves upstream equation
   and metadata and filters other versions rather than inventing them.
   https://github.com/pvpgn/pvpgn-server/blob/9cd173f4e02ba3d9f8f15a67ca308b5eb78723e4/conf/versioncheck.json.in
   https://github.com/pvpgn/pvpgn-server/blob/9cd173f4e02ba3d9f8f15a67ca308b5eb78723e4/conf/bnetd.conf.in

5. **Support-file installation and foreground CLI** — support files are already
   part of the source tree; bnetd supports -f and -c FILE.
   https://github.com/pvpgn/pvpgn-server/blob/9cd173f4e02ba3d9f8f15a67ca308b5eb78723e4/files/CMakeLists.txt
   https://github.com/pvpgn/pvpgn-server/blob/9cd173f4e02ba3d9f8f15a67ca308b5eb78723e4/src/bnetd/cmdline.cpp

6. **Gateway format reference** — the existing upstream gateway installer shows
   the HKCU Warcraft III value name and version/index/address-zone-name layout.
   The PowerShell helper in this kit was written independently and adds backup
   handling. It has not been executed on Windows.
   https://github.com/pvpgn/battle.net-gateway-installer/blob/5d087829511db2a058cdbfb36c0d62e650b084e3/install%20gateway.bat
   https://github.com/pvpgn/pvpgn-server/blob/9cd173f4e02ba3d9f8f15a67ca308b5eb78723e4/files/bnserver-WAR3.ini

7. **Route and client-address translation examples** — separate 6200 route
   server and player game-port/NAT mappings; LAN/external exclusions.
   https://github.com/pvpgn/pvpgn-server/blob/9cd173f4e02ba3d9f8f15a67ca308b5eb78723e4/conf/address_translation.conf.in
   https://github.com/pvpgn/pvpgn-server#hosting-on-lan-or-vps-with-private-ip-address
