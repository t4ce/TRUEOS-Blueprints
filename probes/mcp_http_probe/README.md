# MCP HTTP contract probe

This standalone, probes-only package exercises Codex's actual `RmcpClient`,
`StreamableHttpClientAdapter`, `RouteAwareHttpClient` and HTTP transport against
a deterministic Python LAN fixture. It uses synthetic data and no credentials.
The adapter explicitly takes responsibility for authentication while sending no
authentication headers, bypassing local OAuth credential-store lookup.

Start the fixture on the Linux host reachable from the guest:

```sh
python3 probes/mcp_http_probe/fixture.py > /tmp/mcp-http-fixture.jsonl
```

The probe accepts the fixture base URL as its first argument. Its host default is
`http://192.168.178.111:18766`, the current LAN build host. A host baseline can use
`http://127.0.0.1:18766` instead. Build and publish with `build_publish.sh`; the
package is registered only in `probes.json`, excluded from the Blueprint parent
workspace, and never added to the production Apps catalog.

Cases:

- Initialize and initialized notification, then 106 tools in a 330447-byte JSON body.
- The same tools in fragmented chunked SSE, including a split event terminator,
  with no EOF until the client closes or eight seconds elapse. The list must
  complete in under four seconds.
- Actual HTTP 404 and 401 responses and a TCP reset.
- A 300 ms operation deadline before response headers and during an incomplete body.
- Caller cancellation after 150 ms, shutdown, and a subsequent healthy client.

The probe logs fixed case/phase names, elapsed times, status codes and tool counts.
The fixture additionally records response byte counts and whether stalled/SSE
connections closed. Validate those records with:

```sh
python3 probes/mcp_http_probe/check_fixture.py /tmp/mcp-http-fixture.jsonl
```

The host default and `--http1` mode test plain HTTP/1.1. This does not establish TLS, certificate, ALPN, HTTP/2,
production authentication, real endpoint, or full Codex scheduling correctness.
Operation deadlines are tested with a 250–2000 ms acceptance window, so guest
scheduler delays remain visible as failures rather than being hidden by larger
timeouts.

The HTTPS extension uses Node's built-in HTTP/2 server and a dedicated synthetic
test CA; no additional Node packages or TLS bypass are involved:

```sh
node probes/mcp_http_probe/fixture_https.mjs > /tmp/mcp-https-fixture.jsonl
mcp_http_probe --https
python3 probes/mcp_http_probe/check_fixture.py /tmp/mcp-https-fixture.jsonl --https
```

Native exact-name launch defaults to this HTTPS mode because Shell3 does not
forward probe argv. On the host, `--https` selects the LAN TLS fixture on port 18768, configures its embedded
public test CA through the existing `CODEX_CA_CERTIFICATE` path before client
construction, and adds a certificate-rejection test on port 18769. To test
loopback, pass `https://127.0.0.1:18768 --https --wrong-root https://127.0.0.1:18769`.
The fixture logs actual negotiated ALPN and HTTP version on every MCP request.
The checker requires HTTP/2, correct peer cleanup, and zero HTTP requests reaching
the untrusted listener. Certificate rejection needs either the server's unknown-CA
alert or an actual client transport error containing `UnknownIssuer`; provide the
allowlisted client log with `--client-log <log>`. The latter matters when the native
transport resets the socket before its TLS alert reaches the fixture. The probe
always requires the client-side classification using the normal route-aware pool,
which preserves the error chain before the exec adapter flattens it.

The host default/`--http1` invocation and Python HTTP/1.1 fixture retain the
original nine cases. Native runtimes explicitly enable both the I/O and timer drivers.

Before networking, the probe checks real C `setenv`/`getenv`/`unsetenv`, Rust
environment snapshots, overwrite behavior, invalid-name errors, and native
reserved-key rejection. A worker must observe a main-thread mutation and expose
its own mutation back to the main thread. The embedded CA must then appear in
both the getter and snapshot before constructing the normal Codex HTTP client.
A per-instance sentinel also detects an accidentally shared process environment.

The first native HTTPS run exposed an unresolved `setenv` import returning fake
success: the CA override was absent, and all trusted-root handshakes failed. Its
wrong-root failure alone proves no certificate validation. Validation requires
the repaired environment contract, a successful trusted endpoint, and the
fixture's independent unknown-CA and ALPN witnesses.

The final native kernel9 run of package `9f12bab4` passed all ten cases,
including actual client `UnknownIssuer`, 20 HTTP/2 requests with ALPN `h2`,
zero HTTP requests reaching the untrusted listener, and all cleanup checks.
Tools/list returned 106 tools in 238 ms as JSON and 209 ms as SSE.
See [the native evidence](validation/native-cert-h2-kernel9.json).
This controlled run does not measure production authentication or full Codex
contention on the physical rig. Fixture follow-up endpoints are recorded in
[the fixture metadata](validation/https-fixture-followup.json).
