#!/usr/bin/env python3
"""W3Box: unprivileged PvPGN supervisor, loopback web UI and local CLI socket.

Not a public Internet web service. Access remotely using SSH port forwarding.
No endpoint accepts shell commands, arbitrary executable paths or uploaded code.
"""
from __future__ import annotations

import argparse
import collections
import fcntl
import http.cookies
import http.server
import ipaddress
import json
import logging
import os
import secrets
import signal
import socket
import socketserver
import struct
import subprocess
import tarfile
import threading
import time
from pathlib import Path
from urllib.parse import urlsplit

from common import (PIN, Layout, atomic_write, check_password, load_settings,
                    password_record, read_json, render_config, validate_settings, write_json)

LOG = logging.getLogger("w3box")
STATIC = Path(__file__).resolve().parent / "static"
MAX_REQUEST = 16_384


def tail(path: Path, lines: int = 160) -> str:
    try:
        with path.open("rb") as stream:
            stream.seek(0, 2)
            size = stream.tell()
            stream.seek(max(0, size - 96_000))
            return "\n".join(stream.read().decode("utf-8", errors="replace").splitlines()[-lines:])
    except FileNotFoundError:
        return "No log entries yet."


def listeners(pid: int | None) -> list[dict]:
    """Inspect this child process' own sockets; do not count somebody else's port."""
    if not pid: return []
    try:
        inodes = set()
        for fd in Path(f"/proc/{pid}/fd").iterdir():
            try:
                link = os.readlink(fd)
                if link.startswith("socket:["): inodes.add(link[8:-1])
            except FileNotFoundError:
                pass
        result = []
        for protocol in ("tcp", "udp"):
            for line in Path(f"/proc/{pid}/net/{protocol}").read_text().splitlines()[1:]:
                fields = line.split()
                if len(fields) < 10 or fields[9] not in inodes: continue
                if protocol == "tcp" and fields[3] != "0A": continue
                addr, port = fields[1].split(":")
                address = socket.inet_ntoa(struct.pack("<I", int(addr, 16)))
                result.append({"protocol": protocol, "address": address, "port": int(port, 16)})
        return result
    except (OSError, ValueError):
        return []


def gateway_script(settings: dict) -> str:
    template = (Path(__file__).resolve().parent / "add-gateway.ps1").read_text()
    # Settings validation excludes quotes, semicolons, backticks and newlines.
    return template.replace("__SERVER_ADDRESS__", settings["server_address"]).replace("__SERVER_NAME__", settings["server_name"])


class Manager:
    def __init__(self, layout: Layout):
        self.layout = layout
        self.lock = threading.RLock()
        self.settings = load_settings(layout)
        self.desired = bool(read_json(layout.root / "state.json", {"desired_running": True})["desired_running"])
        self.process: subprocess.Popen | None = None
        self.output = None
        self.started = None
        self.last_error = ""
        self.last_exit = None
        self.failures = collections.deque()
        self.next_start = 0.0
        self.done = threading.Event()
        self.thread = None

    def save_state(self):
        write_json(self.layout.root / "state.json", {"desired_running": self.desired})

    def _spawn(self):
        if self.process is not None and self.process.poll() is None: return
        if self.output:
            self.output.close()
            self.output = None
        render_config(self.layout, self.settings)
        path = self.layout.var / "process.log"
        if path.exists() and path.stat().st_size > 5_000_000:
            path.replace(path.with_suffix(".log.1"))
        self.output = path.open("ab", buffering=0)
        self.process = subprocess.Popen(
            [str(self.layout.binary), "-f", "-c", str(self.layout.conf / "bnetd.conf")],
            cwd=self.layout.prefix / "pvpgn", stdin=subprocess.DEVNULL,
            stdout=self.output, stderr=subprocess.STDOUT, start_new_session=True,
            close_fds=True,
        )
        self.started = time.monotonic()
        LOG.info("Started PvPGN pid=%s", self.process.pid)

    def _stop_process(self) -> bool:
        graceful = True
        if self.process is not None:
            proc = self.process
            if proc.poll() is None:
                try:
                    os.killpg(proc.pid, signal.SIGTERM)
                    proc.wait(timeout=12)
                except ProcessLookupError:
                    pass
                except subprocess.TimeoutExpired:
                    graceful = False
                    LOG.warning("PvPGN did not stop gracefully; killing its process group")
                    os.killpg(proc.pid, signal.SIGKILL)
                    proc.wait(timeout=5)
            self.last_exit = proc.returncode
            self.process = None
        if self.output:
            self.output.close()
            self.output = None
        return graceful

    def start(self):
        with self.lock:
            self.desired = True
            self.save_state()
            self.failures.clear()
            self.last_error = ""
            try:
                self._spawn()
            except (OSError, ValueError) as exc:
                self.last_error = str(exc)
                self.desired = False
                self.save_state()
                raise
            return self.status()

    def stop(self):
        with self.lock:
            self.desired = False
            self.save_state()
            graceful = self._stop_process()
            result = self.status()
            result["graceful_stop"] = graceful
            return result

    def restart(self):
        with self.lock:
            self._stop_process()
            return self.start()

    def status(self):
        with self.lock:
            running = self.process is not None and self.process.poll() is None
            pid = self.process.pid if running else None
            ports = listeners(pid)
            ready = running and any(p["protocol"] == "tcp" and p["port"] == 6112 for p in ports)
            try:
                accounts = sum(p.is_file() and not p.name.startswith(".") for p in (self.layout.var / "users").iterdir())
            except FileNotFoundError:
                accounts = 0
            return {
                "server_name": self.settings["server_name"],
                "server_address": self.settings["server_address"],
                "process_running": running, "ready": ready,
                "desired_running": self.desired, "pid": pid,
                "uptime_seconds": int(time.monotonic() - self.started) if running and self.started else 0,
                "account_files": accounts, "listeners": ports,
                "last_exit": self.last_exit, "last_error": self.last_error,
                "target_client": "Windows RoC 1.21b / WAR3",
                "source_commit": PIN,
                "strict_version": self.settings["strict_version"],
            }

    def logs(self):
        return {"server": tail(self.layout.var / "bnetd.log"), "startup": tail(self.layout.var / "process.log", 80)}

    def doctor(self):
        with self.lock:
            status = self.status()
            checks = []
            def add(name, okay, detail):
                checks.append({"name": name, "ok": bool(okay), "detail": detail})
            add("Server executable", self.layout.binary.is_file() and os.access(self.layout.binary, os.X_OK), str(self.layout.binary))
            add("PvPGN process", status["process_running"], f"PID {status['pid']}" if status["pid"] else "Stopped or failed; read startup and server logs.")
            add("Login listener", status["ready"], "PvPGN must own TCP port 6112.")
            for name in ("IX86ver1.mpq", "icons-WAR3.bni"):
                path = self.layout.var / "files" / name
                add("Support file: " + name, path.is_file() and path.stat().st_size > 0, str(path))
            try:
                versions = read_json(self.layout.conf / "versioncheck.json", {})
                found = any(e.get("versionTag") == "WAR3_121B" for v in versions["WAR3"]["IX86"].values() for e in v["entries"])
            except (KeyError, TypeError):
                found = False
            add("RoC 1.21b version entry", found, "Exact upstream WAR3_121B CheckRevision entry; other versions are not enabled.")
            add("Account storage writable", os.access(self.layout.var / "users", os.W_OK), str(self.layout.var / "users"))
            return {"checks": checks, "notes": [
                "Local checks only: this does not prove external firewall, router/NAT, Windows loader or in-game connectivity.",
                "The Windows game needs a loader compatible with versions BEFORE 1.22a. Modern w3lh/w3l explicitly excludes those versions.",
                "Custom games are hosted by a player's game client. This realm is not a dedicated simulation server or NAT relay.",
                "An account-file count is not an online-player count.",
            ]}

    def update_settings(self, value):
        value = validate_settings(value)
        with self.lock:
            old = self.settings.copy()
            was_desired = self.desired
            self._stop_process()
            try:
                render_config(self.layout, value)
                write_json(self.layout.root / "settings.json", value)
                self.settings = value
            except Exception:
                self.settings = old
                render_config(self.layout, old)
                if was_desired: self._spawn()
                raise
            if was_desired: self._spawn()
            return self.settings.copy()

    def backup(self):
        with self.lock:
            was_running = self.process is not None and self.process.poll() is None
            graceful = self._stop_process()
            name = time.strftime("w3box-%Y%m%d-%H%M%S-") + secrets.token_hex(3) + ".tar.gz"
            path = self.layout.root / "backups" / name
            path.parent.mkdir(parents=True, exist_ok=True)
            try:
                def include(info):
                    # Logs are diagnostic data, not account state; keep archives small.
                    if "/backups/" in info.name or info.name.endswith((".log", ".log.1")): return None
                    return info
                with tarfile.open(path, "w:gz", dereference=False) as archive:
                    for item in ("conf", "var", "settings.json", "state.json", "auth.json"):
                        src = self.layout.root / item
                        if src.exists(): archive.add(src, arcname=item, filter=include)
                path.chmod(0o600)
            except Exception:
                path.unlink(missing_ok=True)
                raise
            finally:
                if was_running: self._spawn()
            return {"filename": name, "path": str(path), "bytes": path.stat().st_size,
                    "graceful_stop": graceful,
                    "message": "Backup includes account data and panel password hash. Keep it private. The realm was briefly stopped to flush state."}

    def latest_backup(self) -> Path:
        paths = list((self.layout.root / "backups").glob("w3box-*.tar.gz"))
        if not paths: raise FileNotFoundError("No backup exists yet.")
        return max(paths, key=lambda p: p.stat().st_mtime)

    def run_monitor(self):
        while not self.done.wait(1):
            with self.lock:
                now = time.monotonic()
                if self.process is not None and self.process.poll() is not None:
                    self.last_exit = self.process.returncode
                    self.last_error = f"PvPGN exited with code {self.last_exit}. Inspect logs."
                    LOG.warning(self.last_error)
                    self.process = None
                    if self.output:
                        self.output.close()
                        self.output = None
                    self.failures.append(now)
                    self.next_start = now + 4
                while self.failures and self.failures[0] < now - 300: self.failures.popleft()
                if self.desired and len(self.failures) >= 5:
                    self.desired = False
                    self.save_state()
                    self.last_error += " Automatic restart stopped after five failures in five minutes."
                if self.desired and self.process is None and now >= self.next_start:
                    try:
                        self._spawn()
                    except Exception as exc:
                        self.last_error = str(exc)
                        self.failures.append(now)
                        self.next_start = now + 5
                        LOG.error("Start failed: %s", exc)

    def launch(self):
        self.thread = threading.Thread(target=self.run_monitor, daemon=True)
        self.thread.start()

    def close(self):
        self.done.set()
        if self.thread: self.thread.join(timeout=2)
        with self.lock: self._stop_process()


class Application:
    def __init__(self, manager: Manager, port: int):
        self.manager = manager
        self.port = port
        self.auth_lock = threading.RLock()
        self.sessions = {}
        self.attempts = collections.deque()
        self.auth = read_json(manager.layout.root / "auth.json")
        if not self.auth: raise ValueError("Panel password missing. Run installer initialization first.")

    def login(self, password: str):
        with self.auth_lock:
            now = time.monotonic()
            while self.attempts and self.attempts[0] < now - 60: self.attempts.popleft()
            if len(self.attempts) >= 8: raise PermissionError("Too many login attempts; retry in a minute.")
            self.attempts.append(now)
            if not check_password(password, self.auth): raise PermissionError("Incorrect password.")
            self.sessions = {k: v for k, v in self.sessions.items() if v["expires"] > now}
            if len(self.sessions) >= 32: self.sessions.pop(next(iter(self.sessions)))
            token, csrf = secrets.token_urlsafe(32), secrets.token_urlsafe(32)
            self.sessions[token] = {"expires": now + 28_800, "csrf": csrf}
            return token, csrf

    def session(self, cookie):
        try:
            values = http.cookies.SimpleCookie(cookie or "")
            token = values["w3box_session"].value
        except (KeyError, http.cookies.CookieError):
            return None, None
        with self.auth_lock:
            data = self.sessions.get(token)
            if not data or data["expires"] <= time.monotonic(): return None, None
            return token, data

    def dispatch(self, command: str, data: dict | None = None, local: bool = False):
        m = self.manager
        data = data or {}
        if command == "status": return m.status()
        if command == "logs": return m.logs()
        if command == "doctor": return m.doctor()
        if command == "settings":
            if data: return m.update_settings(data)
            with m.lock: return m.settings.copy()
        if command == "start": return m.start()
        if command == "stop": return m.stop()
        if command == "restart": return m.restart()
        if command == "backup": return m.backup()
        if command == "gateway": return {"script": gateway_script(m.settings)}
        if command == "password" and local:
            record = password_record(data.get("password", ""))
            with self.auth_lock:
                write_json(m.layout.root / "auth.json", record)
                self.auth = record
                self.sessions.clear()
                self.attempts.clear()
            return {"message": "Panel password changed; all web sessions logged out."}
        raise ValueError("Unknown or unavailable command.")


class Handler(http.server.BaseHTTPRequestHandler):
    server_version = "W3Box/1.0"
    sys_version = ""

    @property
    def app(self): return self.server.app

    def setup(self):
        super().setup()
        self.connection.settimeout(10)

    def log_message(self, fmt, *args):
        # Do not record cookies, passwords or request bodies.
        LOG.debug("HTTP %s", fmt % args)

    def send(self, status, body, kind="application/json; charset=utf-8", headers=None):
        if isinstance(body, (dict, list)): body = json.dumps(body).encode()
        elif isinstance(body, str): body = body.encode()
        self.send_response(status)
        self.send_header("Content-Type", kind)
        self.send_header("Content-Length", str(len(body)))
        self.send_header("Cache-Control", "no-store")
        self.send_header("X-Content-Type-Options", "nosniff")
        self.send_header("Referrer-Policy", "no-referrer")
        self.send_header("X-Frame-Options", "DENY")
        self.send_header("Content-Security-Policy", "default-src 'self'; script-src 'self'; style-src 'self'; img-src 'self' data:; connect-src 'self'; frame-ancestors 'none'; base-uri 'none'; form-action 'self'")
        for key, value in (headers or {}).items(): self.send_header(key, value)
        self.end_headers()
        self.wfile.write(body)

    def valid_host(self):
        host = self.headers.get("Host", "").lower()
        return host in {f"127.0.0.1:{self.app.port}", f"localhost:{self.app.port}"}

    def valid_origin(self):
        origin = self.headers.get("Origin")
        return origin is None or origin == "http://" + self.headers.get("Host", "")

    def error(self, code, message): self.send(code, {"error": message})

    def do_GET(self):
        try:
            if not self.valid_host(): return self.error(403, "Use localhost or 127.0.0.1, directly or through an SSH tunnel.")
            path = urlsplit(self.path).path
            assets = {"/": ("index.html", "text/html; charset=utf-8"),
                      "/static/app.css": ("app.css", "text/css; charset=utf-8"),
                      "/static/app.js": ("app.js", "text/javascript; charset=utf-8")}
            if path in assets:
                name, kind = assets[path]
                return self.send(200, (STATIC / name).read_bytes(), kind)
            token, session = self.app.session(self.headers.get("Cookie"))
            if not session: return self.error(401, "Sign in first.")
            if path == "/api/session": return self.send(200, {"csrf": session["csrf"]})
            if path in ("/api/status", "/api/logs", "/api/doctor", "/api/settings"):
                return self.send(200, self.app.dispatch(path.rsplit("/", 1)[1]))
            if path == "/client/add-gateway.ps1":
                return self.send(200, gateway_script(self.app.manager.settings), "text/plain; charset=utf-8",
                                 {"Content-Disposition": 'attachment; filename="add-gateway.ps1"'})
            if path == "/api/backup/latest":
                with self.app.manager.lock:
                    file = self.app.manager.latest_backup()
                    self.send_response(200)
                    self.send_header("Content-Type", "application/gzip")
                    self.send_header("Content-Length", str(file.stat().st_size))
                    self.send_header("Content-Disposition", f'attachment; filename="{file.name}"')
                    self.send_header("Cache-Control", "no-store")
                    self.send_header("X-Content-Type-Options", "nosniff")
                    self.end_headers()
                    with file.open("rb") as data:
                        while True:
                            block = data.read(65_536)
                            if not block: break
                            self.wfile.write(block)
                return
            self.error(404, "Not found.")
        except FileNotFoundError as exc:
            self.error(404, str(exc))
        except (BrokenPipeError, ConnectionResetError, TimeoutError):
            pass
        except Exception:
            LOG.exception("GET failed")
            self.error(500, "Request failed. Check: journalctl -u w3box")

    def do_POST(self):
        try:
            if not self.valid_host() or not self.valid_origin(): return self.error(403, "Origin rejected.")
            if self.headers.get_content_type() != "application/json": return self.error(415, "Use application/json.")
            if self.headers.get("Transfer-Encoding"): return self.error(400, "Chunked requests are not supported.")
            try: length = int(self.headers.get("Content-Length", "0"))
            except ValueError: return self.error(400, "Invalid content length.")
            if not 0 < length <= MAX_REQUEST: return self.error(413, "Request body must be 1–16384 bytes.")
            data = json.loads(self.rfile.read(length))
            if not isinstance(data, dict): return self.error(400, "JSON object required.")
            path = urlsplit(self.path).path
            if path == "/api/login":
                token, csrf = self.app.login(data.get("password", ""))
                return self.send(200, {"csrf": csrf}, headers={"Set-Cookie": f"w3box_session={token}; HttpOnly; SameSite=Strict; Path=/; Max-Age=28800"})
            token, session = self.app.session(self.headers.get("Cookie"))
            if not session: return self.error(401, "Sign in first.")
            if not secrets.compare_digest(self.headers.get("X-CSRF-Token", ""), session["csrf"]):
                return self.error(403, "CSRF token missing or invalid.")
            if path == "/api/logout":
                with self.app.auth_lock: self.app.sessions.pop(token, None)
                return self.send(200, {"ok": True}, headers={"Set-Cookie": "w3box_session=; HttpOnly; SameSite=Strict; Path=/; Max-Age=0"})
            if path in ("/api/start", "/api/stop", "/api/restart", "/api/backup", "/api/settings"):
                return self.send(200, self.app.dispatch(path.rsplit("/", 1)[1], data))
            self.error(404, "Not found.")
        except PermissionError as exc:
            self.error(403, str(exc))
        except (ValueError, TypeError) as exc:
            self.error(400, str(exc))
        except (BrokenPipeError, ConnectionResetError, TimeoutError):
            pass
        except Exception:
            LOG.exception("POST failed")
            self.error(500, "Operation failed. Check the logs before retrying.")


class WebServer(http.server.ThreadingHTTPServer):
    daemon_threads = True
    allow_reuse_address = True

    def __init__(self, address, app):
        self.app = app
        self.slots = threading.BoundedSemaphore(24)
        super().__init__(address, Handler)

    def process_request(self, request, client_address):
        if not self.slots.acquire(blocking=False):
            self.shutdown_request(request)
            return
        try: super().process_request(request, client_address)
        except Exception:
            self.slots.release()
            raise

    def process_request_thread(self, request, client_address):
        try: super().process_request_thread(request, client_address)
        finally: self.slots.release()


class LocalHandler(socketserver.StreamRequestHandler):
    def handle(self):
        self.request.settimeout(65)
        try:
            _, uid, _ = struct.unpack("3i", self.request.getsockopt(socket.SOL_SOCKET, socket.SO_PEERCRED, struct.calcsize("3i")))
            if uid not in (0, os.getuid()): raise PermissionError("Root or service user required.")
            raw = self.rfile.readline(MAX_REQUEST + 1)
            if len(raw) > MAX_REQUEST or not raw.endswith(b"\n"): raise ValueError("Invalid request size.")
            message = json.loads(raw)
            if not isinstance(message, dict): raise ValueError("JSON object required.")
            result = self.server.app.dispatch(message.get("command", ""), message.get("data", {}), local=True)
            reply = {"ok": True, "result": result}
        except Exception as exc:
            reply = {"ok": False, "error": str(exc)}
        try: self.wfile.write(json.dumps(reply).encode() + b"\n")
        except (BrokenPipeError, ConnectionResetError): pass


class LocalServer(socketserver.ThreadingMixIn, socketserver.UnixStreamServer):
    daemon_threads = True


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--root", type=Path, default=Path("/var/lib/w3box"))
    parser.add_argument("--prefix", type=Path, default=Path("/opt/w3box"))
    parser.add_argument("--runtime", type=Path, default=Path("/run/w3box"))
    parser.add_argument("--port", type=int, default=8787)
    args = parser.parse_args()
    if not 1024 <= args.port <= 65535: parser.error("Port must be 1024–65535.")
    logging.basicConfig(level=logging.INFO, format="%(asctime)s %(levelname)s %(message)s")
    os.umask(0o027)
    layout = Layout(args.root, args.prefix, args.runtime)
    layout.runtime.mkdir(parents=True, exist_ok=True, mode=0o700)
    lockfile = (layout.runtime / "supervisor.lock").open("w")
    try: fcntl.flock(lockfile, fcntl.LOCK_EX | fcntl.LOCK_NB)
    except BlockingIOError: raise SystemExit("Another W3Box supervisor is already running.")
    manager = Manager(layout)
    app = Application(manager, args.port)
    web = WebServer(("127.0.0.1", args.port), app)
    layout.socket.unlink(missing_ok=True)
    local = LocalServer(str(layout.socket), LocalHandler)
    local.app = app
    layout.socket.chmod(0o600)
    shutdown = threading.Event()
    for sig in (signal.SIGTERM, signal.SIGINT): signal.signal(sig, lambda *_: shutdown.set())
    try:
        for server in (web, local): threading.Thread(target=server.serve_forever, daemon=True).start()
        manager.launch()
        LOG.info("Panel listening on http://127.0.0.1:%s ; CLI socket %s", args.port, layout.socket)
        shutdown.wait()
    finally:
        web.shutdown()
        local.shutdown()
        web.server_close()
        local.server_close()
        manager.close()
        layout.socket.unlink(missing_ok=True)
        lockfile.close()

if __name__ == "__main__": main()
