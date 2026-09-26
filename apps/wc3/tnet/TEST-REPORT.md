# W3Box test report

Date: 2026-09-25. Kit version: 0.1.0 (initial, not end-to-end certified).
Packaging environment: Debian GNU/Linux 13, Python 3.13. The installer is
intentionally restricted to Ubuntu, so it was not executed against this host.

## Passed: 41 local control-software tests

Command, run after the final Python configuration changes:

```
python3 -m unittest discover -s tests -v
```

Result: **41 tests, all passed**, 26.066 seconds. Full output: tests/last-run.txt.

The test fixture is an executable Python program, NOT PvPGN. It opens local
TCP/UDP sockets, simulates process exit and graceful state flushing, and uses
small dummy files in place of real support files. Those dummy support files
are created only by tests; the production installer obtains real files from
the pinned upstream source and checks for them after building.

Coverage includes input validation, malformed/injected settings, exact 1.21b
version selection, atomic writes, password hashes and permissions, idempotent
initialization, preserving manual translation rules, process-owned listener
inspection, start/stop/restart, desired-state persistence, configuration
changes, backup state flushing and restart, crash recovery, restart-loop
cutoff, fixed command allowlists, HTTP authentication, Host/Origin validation,
CSRF requirements, session invalidation, request limits, login throttling,
cookie flags, gateway download, and refusal of browser password-reset requests.

These are functional tests, not a penetration test or proof of security.

## Passed: separate unprivileged process/CLI integration

The actual supervisor entry point was launched as Linux user `nobody`, UID
65534, with a temporary fixture installation. Its simulated server child also
ran as UID 65534. Actual CLI commands communicated through the Unix socket.
The checks passed for status/readiness, configuration-and-restart, diagnostics,
and stop/start. Output: tests/process-integration-results.json.

This does not test the systemd sandbox, actual PvPGN or an Ubuntu installation.

## Passed: offline browser interface checks

The actual bundled HTML, CSS and JavaScript were rendered in Chromium through
Playwright with explicit in-memory API fixtures. Login, configuration changes,
logs, start/stop buttons, the backup action, logout, and 390-pixel mobile layout
were exercised. There were no JavaScript errors. A disabled-button refresh
issue was found and fixed before the final successful run.

Output: tests/offline-ui-results.json. Screenshots: preview/desktop.png and
preview/mobile.png. Both show a prominent simulated-data warning.

This was **not a browser-to-real-server end-to-end test**. Attempting ordinary
Chromium navigation to the local test HTTP server was blocked by the browser's
administrator policy (ERR_BLOCKED_BY_ADMINISTRATOR). No policy was disabled.
Direct HTTP handler behavior was instead covered by the local unit tests;
browser interactions used in-memory fixture responses, not real requests.

## Passed: syntax and unit-file checks

```
bash -n install.sh
python3 -m compileall -q app
node --check app/static/app.js
systemd-analyze verify w3box.service
```

All exited successfully. Node.js and Playwright were development/test tools;
neither is required by the installed application. A systemd unit verification
is not equivalent to running the service under a booted Ubuntu system.

## Not tested or not verified

- Downloading the real source with Git from the packaging container, or APT
  downloads. Network/DNS access for those native operations was blocked.
  Public upstream repository text and commit metadata were checked separately.
- Compiling, staging or running the real PvPGN executable on any Ubuntu release.
- A real Warcraft III 1.21b login, account registration, chat, match listing,
  matchmaking, hosting, or game session.
- Windows execution of the PowerShell gateway helper or launch CMD file.
- Authenticity, safety or compatibility of a legacy loader executable. None is
  supplied; the modern W3L project explicitly excludes this client version.
- Internet/VPN/LAN routing, UFW, cloud firewalls, port forwarding, NAT or CGNAT.
- Non-x86 Ubuntu architectures, a loaded production realm, disk exhaustion,
  adversarial traffic, or long-duration operation.

The installer reports its own Git/APT/build failures and requires the launched
child to own TCP 6112 before announcing readiness. That is a runtime check on
the target machine, not evidence that this package was end-to-end tested here.
Do not describe this kit as fully offline, guaranteed turnkey, or security
audited. Use a test/private host and retain account backups.
