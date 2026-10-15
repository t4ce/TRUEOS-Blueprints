#!/usr/bin/env python3
"""Controlled TCP/WS/WSS peer. Test-only certificate/key; no model/API traffic."""
import argparse
import base64
import hashlib
import http.server
import json
from pathlib import Path
import socket
import socketserver
import ssl
import struct
import threading
import time

HERE = Path(__file__).resolve().parent
MAX_FRAME = 1024 * 1024
REPORT_LOCK = threading.Lock()


def report(event, **fields):
    with REPORT_LOCK:
        print(json.dumps(dict(event=event, **fields), sort_keys=True), flush=True)


def exact(stream, count):
    result = bytearray()
    while len(result) < count:
        part = stream.recv(count - len(result))
        if not part:
            raise EOFError(f"EOF at {len(result)}/{count}")
        result.extend(part)
    return bytes(result)


def send_frame(stream, opcode, payload=b"", final=True):
    head = bytes([(0x80 if final else 0) | opcode])
    if len(payload) < 126:
        head += bytes([len(payload)])
    elif len(payload) <= 65535:
        head += b"\x7e" + struct.pack("!H", len(payload))
    else:
        head += b"\x7f" + struct.pack("!Q", len(payload))
    # Chunk transport writes across frame/header and TLS-record boundaries.
    data = head + payload
    for start in range(0, len(data), 8191):
        stream.sendall(data[start:start + 8191])


def read_frame(stream):
    first, second = exact(stream, 2)
    length = second & 127
    if length == 126:
        length = struct.unpack("!H", exact(stream, 2))[0]
    elif length == 127:
        length = struct.unpack("!Q", exact(stream, 8))[0]
    if length > MAX_FRAME:
        raise ValueError("frame exceeds fixture budget")
    if not second & 128:
        raise ValueError("client frame was not masked")
    mask = exact(stream, 4)
    payload = exact(stream, length)
    payload = bytes(value ^ mask[i % 4] for i, value in enumerate(payload))
    return first & 15, payload


class ThreadServer(socketserver.ThreadingTCPServer):
    allow_reuse_address = True
    daemon_threads = True


class Raw(socketserver.BaseRequestHandler):
    def handle(self):
        stream = self.request
        stream.settimeout(15)
        stream.setsockopt(socket.IPPROTO_TCP, socket.TCP_NODELAY, 1)
        mode = "?"
        total = 0
        try:
            mode = exact(stream, 1).decode("ascii")
            report("raw_begin", case=mode)
            if mode == "E":
                while True:
                    data = stream.recv(257)
                    if not data:
                        break
                    total += len(data)
                    time.sleep(0.001)
                    stream.sendall(data)
            elif mode == "P":
                time.sleep(0.1)
                stream.sendall(b"peek-witness")
            elif mode == "H":
                expected = bytes((i * 23 + i // 257) % 251 for i in range(300 * 1024))
                time.sleep(0.2)
                while True:
                    data = stream.recv(257)
                    if not data:
                        break
                    if data != expected[total:total + len(data)]:
                        raise ValueError("half-close payload differs")
                    total += len(data)
                if total != len(expected):
                    raise ValueError(f"FIN before complete payload: {total}/{len(expected)}")
                stream.sendall(b"half-close-ok")
                report("fin_observed", received_bytes=total)
            elif mode == "C":
                time.sleep(0.3)
                stream.sendall(b"delayed-witness")
            else:
                raise ValueError("unknown raw mode")
            report("raw_done", case=mode, received_bytes=total)
        except (OSError, EOFError, ValueError) as error:
            report("raw_error", case=mode, error=str(error), received_bytes=total)


class WebSocket(socketserver.BaseRequestHandler):
    def handle(self):
        stream = self.request
        stream.settimeout(15)
        stream.setsockopt(socket.IPPROTO_TCP, socket.TCP_NODELAY, 1)
        tls = bool(getattr(self.server, "tls", None))
        path = "?"
        try:
            if tls:
                stream = self.server.tls.wrap_socket(stream, server_side=True)
            header = bytearray()
            while not header.endswith(b"\r\n\r\n"):
                header.extend(exact(stream, 1))
                if len(header) > 16384:
                    raise ValueError("oversized HTTP upgrade")
            lines = header.decode("ascii").split("\r\n")
            method, path, _ = lines[0].split()
            headers = dict(line.split(":", 1) for line in lines[1:] if ":" in line)
            headers = {key.lower(): value.strip() for key, value in headers.items()}
            if method != "GET" or headers.get("sec-websocket-version") != "13":
                raise ValueError("invalid upgrade")
            accepted = base64.b64encode(hashlib.sha1((headers["sec-websocket-key"] + "258EAFA5-E914-47DA-95CA-C5AB0DC85B11").encode()).digest())
            stream.sendall(b"HTTP/1.1 101 Switching Protocols\r\nUpgrade: websocket\r\nConnection: Upgrade\r\nSec-WebSocket-Accept: " + accepted + b"\r\n\r\n")
            report("ws_upgraded", tls=tls, path=path)
            if path == "/fragment":
                payload = "fragmented-€-witness".encode()
                # Split the multibyte code point between continuation frames.
                split = payload.index(b"\xe2") + 1
                send_frame(stream, 1, payload[:split], final=False)
                time.sleep(0.1)
                send_frame(stream, 0, payload[split:])
            elif path == "/ping":
                send_frame(stream, 9, b"probe-ping")
                opcode, payload = read_frame(stream)
                if opcode != 10 or payload != b"probe-ping":
                    raise ValueError("missing matching Pong")
                report("pong_observed", tls=tls)
                send_frame(stream, 1, b"pong-observed")
            elif path == "/close":
                time.sleep(0.2)
                send_frame(stream, 8, struct.pack("!H", 1000))
            elif path == "/reset":
                time.sleep(0.2)
                stream.setsockopt(socket.SOL_SOCKET, socket.SO_LINGER, struct.pack("ii", 1, 0))
                return
            while True:
                opcode, payload = read_frame(stream)
                report("ws_frame", tls=tls, path=path, opcode=opcode, payload_bytes=len(payload))
                if opcode == 8:
                    if path != "/close":
                        send_frame(stream, 8, payload)
                    break
                if opcode == 9:
                    send_frame(stream, 10, payload)
                elif opcode in (1, 2):
                    send_frame(stream, opcode, payload)
        except (OSError, EOFError, ValueError, KeyError) as error:
            report("ws_end", tls=tls, path=path, error=str(error))
        finally:
            if stream is not self.request:
                stream.close()


class Artifact(http.server.BaseHTTPRequestHandler):
    def do_GET(self):
        if self.path != "/network_contract.bp":
            self.send_error(404)
            return
        path = self.server.artifact
        if not path.is_file():
            self.send_error(404)
            return
        self.send_response(200)
        self.send_header("Content-Length", str(path.stat().st_size))
        self.end_headers()
        with path.open("rb") as source:
            while chunk := source.read(65536):
                self.wfile.write(chunk)
    def log_message(self, *_):
        pass


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--bind", default="127.0.0.1")
    parser.add_argument("--port", type=int, default=32350)
    parser.add_argument("--seconds", type=int, default=600)
    parser.add_argument("--artifact", type=Path, default=HERE.parents[2] / "dist/network_contract.bp")
    args = parser.parse_args()
    context = ssl.SSLContext(ssl.PROTOCOL_TLS_SERVER)
    context.load_cert_chain(HERE / "cert.pem", HERE / "key.pem")
    servers = []
    for offset, handler in [(0, Raw), (1, WebSocket), (2, WebSocket), (3, Artifact)]:
        server = ThreadServer((args.bind, args.port + offset), handler)
        server.tls = context if offset == 2 else None
        server.artifact = args.artifact
        threading.Thread(target=server.serve_forever, daemon=True).start()
        servers.append(server)
    report("fixture_ready", bind=args.bind, raw=args.port, ws=args.port+1, wss=args.port+2, artifact=args.port+3)
    try:
        time.sleep(args.seconds)
    except KeyboardInterrupt:
        pass
    finally:
        for server in servers:
            server.shutdown()
            server.server_close()
        report("fixture_stopped")


if __name__ == "__main__":
    main()
