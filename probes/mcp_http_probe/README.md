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

The probe accepts the fixture base URL as its first argument. Its default is
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

This is plain HTTP/1.1. It does not establish TLS, certificate, ALPN, HTTP/2,
production authentication, real endpoint, or full Codex scheduling correctness.
Operation deadlines are tested with a 250–2000 ms acceptance window, so guest
scheduler delays remain visible as failures rather than being hidden by larger
timeouts.
