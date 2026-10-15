#!/usr/bin/env python3
"""Check synthetic fixture contract/cleanup evidence without printing payloads."""
import json
import sys


def check(path, https=False, client_log=None):
    with open(path) as source:
        rows = [json.loads(line) for line in source if line.strip()]
    for case in ("json", "recovery"):
        bodies = [row for row in rows if row.get("case") == case and row.get("stage") == "body_written"]
        assert bodies and all(row["bytes"] > 256 * 1024 for row in bodies), case
    events = [row for row in rows if row.get("case") == "sse" and row.get("stage") == "event_written"]
    assert events and all(row["bytes"] > 256 * 1024 and row["fragments"] > 10 for row in events)
    for case in ("sse", "headers_timeout", "body_timeout", "cancel"):
        ends = [row for row in rows if row.get("case") == case and row.get("stage") == "stream_end"]
        assert ends and all(row["peer_closed"] == 1 for row in ends), case
    if https:
        requests = [row for row in rows if row.get("stage") in ("initialize", "tools_list")]
        assert requests and all(row["protocol"] == 2 and row["alpn_h2"] == 1 for row in requests)
        rejected = [row for row in rows if row.get("stage") == "tls_rejected" and row.get("wrong_root") == 1]
        client_unknown_issuer = False
        if client_log:
            with open(client_log) as source:
                client_unknown_issuer = any("case=wrong_root phase=certificate_validation unknown_issuer=1 ok=1" in line for line in source)
        assert rejected and (any(row["unknown_ca"] == 1 for row in rejected) or client_unknown_issuer)
        assert not any(row.get("stage") == "unexpected_wrong_root_request" for row in rows)
    print("mcp_http_fixture: PASS large_json=1 fragmented_sse=1 peer_cleanup=4 protocol=" + ("h2 wrong_root_rejected=1" if https else "http1"))


if __name__ == "__main__":
    client_log = sys.argv[sys.argv.index("--client-log") + 1] if "--client-log" in sys.argv else None
    check(sys.argv[1], "--https" in sys.argv[2:], client_log)
