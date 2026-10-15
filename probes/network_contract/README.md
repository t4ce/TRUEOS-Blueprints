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

Build without publishing:

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
| socket.peek_preserves_bytes | Peek returns data without consuming it |
| socket.shutdown_write_fin | Write half-close delivers FIN; the peer replies on the still-readable socket |
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

The Linux baseline passed all28 cases in approximately7 seconds with the actual
vendored transport code. The native Blueprint builds and passes the CABI pack
guard. Physical-rig execution is still pending; host success does not prove
native ABI behavior.

Source audit identified likely native failures that this probe deliberately
checks: `std_abi_shim::shutdown` acknowledges the Mio route without performing
half-close, and native `recv` ignores MSG_PEEK. Error/close readiness also needs
the runtime witnesses. These findings are not treated as probe passes or fixed
by this addition. The shared MaybeTlsStream wrapper also needed four obsolete
`ref mut` bindings removed to compile its plain-TCP path with Rust2024.
