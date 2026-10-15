#!/usr/bin/env python3
"""Synthetic MCP HTTP/1.1 fixture. Logs fixed stages/counts only; no auth."""
import argparse
import json
import select
import socket
import struct
import threading
import time
from http.server import BaseHTTPRequestHandler, ThreadingHTTPServer

LOCK = threading.Lock()


def log(**fields):
    fields["monotonic_ns"] = time.monotonic_ns()
    fields["unix_ms"] = time.time_ns() // 1_000_000
    with LOCK:
        print(json.dumps(fields, sort_keys=True), flush=True)


TOOLS = [{"name": f"synthetic_tool_{i:03d}", "description": "synthetic description " + "x" * 3000,
          "inputSchema": {"type": "object", "properties": {}}} for i in range(106)]


class Handler(BaseHTTPRequestHandler):
    protocol_version = "HTTP/1.1"

    def log_message(self, *_):
        pass

    def reply(self, code, data=b"", content_type="application/json"):
        self.send_response(code)
        self.send_header("Content-Type", content_type)
        self.send_header("Content-Length", str(len(data)))
        self.end_headers()
        if data:
            self.wfile.write(data)
            self.wfile.flush()

    def do_GET(self):
        self.reply(405)

    def do_DELETE(self):
        self.reply(204)

    def wait_closed(self, case):
        start = time.monotonic()
        deadline = start + 8
        closed = False
        while time.monotonic() < deadline:
            ready, _, _ = select.select([self.connection], [], [], .05)
            if ready:
                try:
                    closed = not self.connection.recv(1, socket.MSG_PEEK)
                except OSError:
                    closed = True
                if closed:
                    break
        log(case=case, stage="stream_end", peer_closed=int(closed), duration_ms=int((time.monotonic() - start) * 1000))
        self.close_connection = True

    def do_POST(self):
        self.connection.setsockopt(socket.IPPROTO_TCP, socket.TCP_NODELAY, 1)
        case = self.path.strip("/")
        if case not in {"json", "sse", "404", "401", "reset", "headers_timeout", "body_timeout", "cancel", "recovery"}:
            self.reply(404)
            return
        try:
            message = json.loads(self.rfile.read(int(self.headers.get("Content-Length", "0"))))
            method = message.get("method")
            if method == "initialize":
                result = {"protocolVersion": "2025-06-18", "capabilities": {"tools": {}},
                          "serverInfo": {"name": "synthetic-fixture", "version": "1"}}
                data = json.dumps({"jsonrpc": "2.0", "id": message["id"], "result": result}).encode()
                self.reply(200, data)
                log(case=case, stage="initialize", bytes=len(data))
                return
            if method == "notifications/initialized":
                self.reply(202)
                log(case=case, stage="initialized")
                return
            if method != "tools/list":
                self.reply(400)
                return
            log(case=case, stage="tools_list")
            if case in {"404", "401"}:
                self.reply(int(case), b"{}")
                return
            if case == "reset":
                self.connection.setsockopt(socket.SOL_SOCKET, socket.SO_LINGER, struct.pack("ii", 1, 0))
                self.connection.close()
                self.close_connection = True
                return
            if case in {"headers_timeout", "cancel"}:
                self.wait_closed(case)
                return
            data = json.dumps({"jsonrpc": "2.0", "id": message["id"], "result": {"tools": TOOLS}}, separators=(",", ":")).encode()
            if case == "body_timeout":
                self.send_response(200)
                self.send_header("Content-Type", "application/json")
                self.send_header("Content-Length", str(len(data)))
                self.end_headers()
                self.wfile.write(data[:100])
                self.wfile.flush()
                self.wait_closed(case)
            elif case == "sse":
                self.send_response(200)
                self.send_header("Content-Type", "text/event-stream")
                self.send_header("Transfer-Encoding", "chunked")
                self.end_headers()
                event = b"event: message\r\ndata: " + data
                # Split field names, CRLF, UTF-8/JSON tokens and the event terminator.
                sizes = [1, 2, 7, 3, 4093, 16381]
                offset = 0
                part = 0
                while offset < len(event):
                    count = sizes[part % len(sizes)]
                    fragment = event[offset:offset + count]
                    self.wfile.write(f"{len(fragment):x}\r\n".encode() + fragment + b"\r\n")
                    self.wfile.flush()
                    offset += len(fragment)
                    part += 1
                    if part < 8:
                        time.sleep(.002)
                for fragment in [b"\r", b"\n\r", b"\n"]:
                    self.wfile.write(f"{len(fragment):x}\r\n".encode() + fragment + b"\r\n")
                    self.wfile.flush()
                    time.sleep(.005)
                log(case=case, stage="event_written", bytes=len(data), fragments=part)
                # No terminating zero chunk: reply must complete before EOF.
                self.wait_closed(case)
            else:
                self.reply(200, data)
                log(case=case, stage="body_written", bytes=len(data))
        except (BrokenPipeError, ConnectionResetError, OSError):
            log(case=case, stage="peer_reset")
            self.close_connection = True


def main():
    parser = argparse.ArgumentParser()
    parser.add_argument("--bind", default="0.0.0.0")
    parser.add_argument("--port", type=int, default=18766)
    args = parser.parse_args()
    server = ThreadingHTTPServer((args.bind, args.port), Handler)
    log(stage="ready", port=server.server_port, tools=len(TOOLS), protocol="http1")
    server.serve_forever()


if __name__ == "__main__":
    main()
