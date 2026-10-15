#!/usr/bin/env python3
"""Check synthetic fixture contract/cleanup evidence without printing payloads."""
import json
import sys


def check(path):
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
    print("mcp_http_fixture: PASS large_json=1 fragmented_sse=1 peer_cleanup=4 protocol=http1")


if __name__ == "__main__":
    check(sys.argv[1])
