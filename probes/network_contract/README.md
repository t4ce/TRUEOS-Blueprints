# Socket and WebSocket contract probe

This small Blueprint checks real std/POSIX, Tokio, rustls and Tungstenite calls
against a deterministic LAN peer. It sends synthetic witnesses, never an API
key or model request. There is no fallback from a failed case to another API.
Each case logs BEGIN and an explicit PASS/FAIL; missing final suite completion
is not a pass. Socket loops and async cases have deadlines, but an externally
stuck executor still needs an external watchdog.

Start the companion server on the development host:

```sh
python3 probes/network_contract/fixture/server.py --bind 192.168.178.111 --seconds 600
```

Build and publish directly into the Appstore's Apps folder:

```sh
probes/network_contract/build_publish.sh
```

It stays registered only in `probes.json`; the script directs this probe's
publish to `smb://t4ce@pdjb/home-share/TRUEOS_SITE/apps`. It replaces only older
versions of `network_contract.bp` and preserves the hash used by Appstore.
Launch `§§network_contract` on the rig. The socket fixes require the rebuilt
kernel; downloading a new Blueprint alone does not update kernel ABI handlers.
To build without publishing:

```sh
TRUEOS_BLUEPRINT_SKIP_APPS_PUBLISH=1 cargo bpp network_contract
```

The artifact is `dist/network_contract.bp`. Launch it as a separate Blueprint;
its defaults target `192.168.178.111`, ports 32350–32352. It does not need to
stop Codex or reboot the rig. Optional arguments are `--host=NUMERIC_IP`,
`--port=BASE_PORT`, and `--runtime=current|multi|both` (default both). The HTTP
fixture on BASE_PORT+3 serves only `/network_contract.bp` for an existing local
loader; it does not install or execute the artifact on the rig.

For a reproducible Linux baseline with its own loopback fixture:

```sh
python3 probes/network_contract/run_host.py
```

The fixture reports handshake, frame opcode, byte count, Pong and FIN witnesses.
It never logs frame contents. The checked-in certificate and leaf private key
are public test fixtures for `network-probe.test`; the probe trusts only that
test CA for its WSS cases. Production trust stores are not changed. Native
certificate time validity and hostname checks still apply.

| Case | Independently checked expectation |
| --- | --- |
| socket.echo_300k | Interleaved nonblocking send/read, delayed 257-byte echo, exact 300 KiB beyond the native 256 KiB transmit window |
| socket.peek_preserves_bytes | Repeated peeks preserve data for the subsequent read |
| socket.shutdown_write_fin | All 300 KiB arrive intact before FIN; the peer then replies on the still-readable socket |
| socket.delayed_data_then_eof | Pending data is not EOF; actual peer closure becomes EOF |
| tcp.cancel_read_resume_eof | Timed-out read is dropped, a replacement read wakes and retains all bytes, then EOF |
| ws.text / wss.text | HTTP 101, strict TLS for WSS, bidirectional text |
| ws.binary_300k / wss.binary_300k | Large exact binary payload across frames, TLS records and TCP windows |
| wss.fragmented | Continuation-frame assembly, splitting a UTF-8 code point across frames |
| wss.ping_pong | Server observes the automatic matching Pong before returning a witness |
| wss.idle_pump_reuse | Four exchanges on one connection using Codex's select/mpsc/oneshot pump pattern, idle periods and canceled receives |
| wss.peer_close / wss.peer_reset | Idle read wakes for graceful Close and abrupt transport termination |
| wss.untrusted_rejected | Empty trust store rejects the fixture with a retained typed rustls certificate error |
| wss.parallel_four | Four simultaneous independently verified connections complete and join |

The four std/POSIX cases run once. The twelve async cases run on both a
current-thread and a two-worker runtime: **28 cases** total. Runtime teardown is
also logged. Compression negotiation, proxy tunnels, and every socket option
are outside this initial suite; a pass establishes only the cases above.

The Linux baseline passed all 28 cases in 6.7 seconds with the actual vendored
transport code. Independent fixture witnesses confirmed FIN=1, Pong=2, and four
300 KiB binary frames. The native Blueprint builds and passes the CABI pack guard.

The physical rig completed in 22.4 seconds on 2026-10-15: **26 passed, 2 failed**.
All 24 async cases passed across both runtime types. Peek consumed its witness,
so the following read reached EOF. Write half-close never delivered FIN, so the
fixture timed out and closed without sending its reply. The native fixture
observed both Pongs and all four 300 KiB binary frames, and zero FIN witnesses.
The probe returned the terminal, exited cleanly, and released its carrier with
zero native jobs remaining.
See [results and artifact hash](validation/rig-2026-10-15.json),
[filtered rig records](validation/rig-2026-10-15.log), and
[independent fixture records](validation/rig-fixture-2026-10-15.jsonl).

These failures match two source gaps: `std_abi_shim::shutdown` acknowledges the
Mio route without performing half-close, and native `recv` ignores MSG_PEEK.
The kernel repair now preserves peeked bytes, drains queued writes before FIN,
retains the readable half, implements Read/Write/Both shutdown on both socket
routes, and wakes blocked I/O after local shutdown. Loopback EOF is ordered
after queued bytes and preserves the peer's reply path. The repaired kernel
passes 61 focused regression tests and builds as `bld/trueos.iso` in the sibling
TRUEOS checkout. The published protocol-2 probe adds repeat peeks and verifies
the complete 300 KiB witness before FIN. The old rig records above remain the
baseline; physical verification requires booting the new kernel.

A rerun also exposed and repaired dropped final reply bytes when smoltcp
entered TIME-WAIT. The final repaired kernel passed **28/28** in QEMU in
8.225 seconds, with FIN after all 300 KiB, both Pongs, and all four binary
witnesses independently observed. The probe returned its terminal and completed
teardown with zero native jobs. The canonical boot image contains the same
runtime ELF that was tested. See [healed results](validation/healed-qemu-2026-10-15.json),
[guest records](validation/healed-qemu-2026-10-15.log), and
[fixture witnesses](validation/healed-qemu-fixture-2026-10-15.jsonl).

The passing async cases establish the exercised
transport contracts; they do not establish that Codex's whole turn or SQLx
remote connection pool progresses. Idle reuse here uses four exchanges with
350 ms idle intervals, rather than a multi-minute soak.

In the preceding Codex boot, warmup completed, but the real turn did not reach
WebSocket connection setup until roughly 13 minutes later. SQLx logged a
153.156-second acquisition during that interval. Its TRUEOS backend uses
persistent remote SQLite connections; acquisition can include permit wait,
Ping, connection opening, and initialization queries. Those records warrant a
separate remote-pool probe and do not identify which acquisition phase stalled.

The shared MaybeTlsStream wrapper also needed four obsolete `ref mut` bindings
removed to compile its plain-TCP path with Rust 2024.
