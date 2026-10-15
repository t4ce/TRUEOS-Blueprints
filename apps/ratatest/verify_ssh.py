#!/usr/bin/env python3
"""Verify Ratatest's deployed SSH transport, input and terminal lifecycle.

Launches the published app through Shell3's ``§§ratatest`` path. Captures the
returned terminal bytes and checks every case, composer submission, PTY resize,
raw probe entry/return, and clean quit. This is a transport check: SSH forwards
app ANSI bytes without using TRUEOS's native terminal parser or UI4 renderer.
The capture answers CPR queries with a synthetic origin (1,1), which Ratatui
requires when clearing its viewport. Those replies are logged separately;
they do not establish TRUEOS cursor-report or other capability support.

Example:
  python3 apps/ratatest/verify_ssh.py --output /tmp/ratatest-ssh-run1

The host key must already be trusted in --known-hosts. The output directory
must be new. Only Python's standard library and OpenSSH are required.
"""

import argparse
import fcntl
import json
import os
from pathlib import Path
import pty
import re
import select
import signal
import struct
import subprocess
import termios
import time
import tty
import uuid


APP = Path(__file__).resolve().parent
RAW_MARKER = "RATATEST RAW ANSI PROBE"
ANSI = re.compile(
    r"\x1b\][^\x07\x1b]*(?:\x07|\x1b\\)|"
    r"\x1b\[[0-?]*[ -/]*[@-~]|\x1b[ -/]*[@-Z\\-_]"
)


def plain(data):
    """Remove terminal commands without interpreting or validating their cells."""
    return ANSI.sub("", bytes(data).decode("utf-8", errors="replace"))


def printed_text(text):
    # CrosstermBackend may move past unchanged blank cells instead of printing
    # spaces. Match the exact printed characters while allowing those gaps.
    return r"\s*".join(re.escape(part) for part in text.split())


def source_cases():
    """Read the app's source inventory instead of maintaining a second count."""
    ui = (APP / "src/ui.rs").read_text()
    gallery = (APP / "src/gallery.rs").read_text()
    arena = (APP / "src/arena.rs").read_text()
    explicit = re.findall(r'\(\s*"([a-z][a-z0-9-]*)"\s*,\s*"([^"\n]*)"\s*,', ui)
    pages = re.search(r"pub const PAGES\b.*?=\s*&\[(.*?)\n\];", gallery, re.S)
    if not pages:
        raise ValueError("Could not read gallery::PAGES from src/gallery.rs")
    titles = re.findall(r'\(\s*"([^"\n]*)"\s*,', pages.group(1))
    # Scenario tuples precede the gallery loop; Codex and ANSI tuples follow it.
    before_gallery = ui.split("gallery::PAGES", 1)[0]
    scenarios = re.findall(r'\(\s*"([a-z][a-z0-9-]*)"\s*,\s*"([^"\n]*)"\s*,', before_gallery)
    cases = scenarios + [(f"widgets-{i}", title) for i, title in enumerate(titles)]
    cases += explicit[len(scenarios):]
    arena_pages = re.search(r"pub const PAGES\b.*?=\s*&\[(.*?)\n\];", arena, re.S)
    if not arena_pages:
        raise ValueError("Could not read arena::PAGES from src/arena.rs")
    cases += re.findall(r'\(\s*"([a-z][a-z0-9-]*)"\s*,\s*"([^"\n]*)"\s*,', arena_pages.group(1))
    if not cases or len({case_id for case_id, _ in cases}) != len(cases):
        raise ValueError("Source inventory is empty or contains duplicate case ids")
    required = {"session", "composer", "ansi-cursor", "ansi-history", "ansi-probes", "ansi-modes", "input"}
    if not required.issubset({case_id for case_id, _ in cases}):
        raise ValueError("Source inventory is missing a required verification case")
    return cases


class Session:
    def __init__(self, args, output):
        self.args = args
        self.started = time.monotonic()
        self.data = bytearray()
        self.checks = []
        self.terminal = (output / "terminal.bin").open("wb")
        self.chunks = (output / "chunks.jsonl").open("w")
        self.query_replies = (output / "query-replies.jsonl").open("w")
        self.cpr_tail = b""
        self.cpr_replies = 0
        self.master, self.slave = pty.openpty()
        self.geometry(120, 40)
        # Raw local input also disables IXON, preserving Ctrl-Q for app cleanup.
        tty.setraw(self.slave)
        host = args.host if "@" in args.host else f"t4ce@{args.host}"
        self.command = [
            "ssh", "-F", "/dev/null", "-tt", "-p", str(args.port),
            "-o", "BatchMode=yes", "-o", "ConnectTimeout=5",
            "-o", "StrictHostKeyChecking=yes", "-o", "LogLevel=ERROR",
            "-o", f"UserKnownHostsFile={args.known_hosts}",
            "-o", "IdentitiesOnly=yes", "-i", str(args.identity), host,
        ]
        self.process = subprocess.Popen(
            self.command, stdin=self.slave, stdout=self.slave, stderr=self.slave,
            env=dict(os.environ, TERM="xterm-256color"),
        )

    def geometry(self, columns, rows):
        fcntl.ioctl(self.slave, termios.TIOCSWINSZ, struct.pack("HHHH", rows, columns, 0, 0))
        if hasattr(self, "process"):
            self.process.send_signal(signal.SIGWINCH)

    def receive(self, timeout):
        if not select.select([self.master], [], [], max(0, timeout))[0]:
            return b""
        try:
            data = os.read(self.master, 65536)
        except OSError:
            return b""
        if data:
            offset = len(self.data)
            self.data.extend(data)
            self.terminal.write(data)
            self.terminal.flush()
            self.chunks.write(json.dumps({
                "elapsed_ms": round((time.monotonic() - self.started) * 1000, 3),
                "offset": offset, "bytes": len(data),
            }) + "\n")
            self.chunks.flush()
            self.answer_queries(data, offset)
        return data

    def answer_queries(self, data, offset):
        """Answer split CPR requests without mixing replies with test input.

        A fixed origin is sufficient for Ratatui's clear/save/restore lifecycle:
        this capture checks fresh absolute-positioned frames, not terminal cells.
        Other capability queries remain unanswered rather than advertising
        features this small responder cannot implement.
        """
        combined = self.cpr_tail + data
        for match in re.finditer(b"\x1b\\[6n", combined):
            response = b"\x1b[1;1R"
            remaining = memoryview(response)
            while remaining:
                remaining = remaining[os.write(self.master, remaining):]
            self.cpr_replies += 1
            self.query_replies.write(json.dumps({
                "elapsed_ms": round((time.monotonic() - self.started) * 1000, 3),
                "query_offset": offset - len(self.cpr_tail) + match.start(),
                "query": "CSI 6 n", "response": response.decode("ascii"),
                "emulated": True, "policy": "synthetic cursor origin, row 1 column 1",
            }) + "\n")
            self.query_replies.flush()
        self.cpr_tail = combined[-3:]

    def drain(self, seconds=.12):
        deadline = time.monotonic() + seconds
        while time.monotonic() < deadline:
            self.receive(min(.05, deadline - time.monotonic()))

    def send(self, payload):
        while self.receive(0):
            pass
        start = len(self.data)
        remaining = memoryview(payload.encode() if isinstance(payload, str) else payload)
        while remaining:
            remaining = remaining[os.write(self.master, remaining):]
        return start

    def expect(self, name, pattern, start, timeout=None, **details):
        timeout = self.args.timeout if timeout is None else timeout
        deadline = time.monotonic() + timeout
        while True:
            if re.search(pattern, plain(self.data[start:]), re.S):
                check = {"name": name, "passed": True, "start_offset": start,
                         "end_offset": len(self.data), **details}
                self.checks.append(check)
                print(f"PASS {name}", flush=True)
                return bytes(self.data[start:])
            if time.monotonic() >= deadline or self.process.poll() is not None:
                self.checks.append({"name": name, "passed": False,
                                    "start_offset": start, "end_offset": len(self.data),
                                    "expected_pattern": pattern, **details})
                raise RuntimeError(f"Missing {name}; inspect terminal.bin at offset {start}")
            self.receive(min(.1, deadline - time.monotonic()))

    def header(self, name, case_id, start, **details):
        return self.expect(name, rf"Ratatui\s*0\.30\.2\s*{re.escape(case_id)}\s*PAUSED", start, **details)

    def close(self):
        if self.process.poll() is None:
            self.process.terminate()
            try:
                self.process.wait(timeout=3)
            except subprocess.TimeoutExpired:
                self.process.kill()
                self.process.wait()
        self.receive(0)
        os.close(self.slave)
        os.close(self.master)
        self.terminal.close()
        self.chunks.close()
        self.query_replies.close()


def verify(session, cases):
    session.expect("Shell3 SSH ready", r"TrueOS\s+§", 0, session.args.startup_timeout)
    start = session.send("§§ratatest\r")
    session.expect("published app launch", r"RATATEST", start, session.args.startup_timeout)
    # All section menus must expose the source inventory, not just a header.
    boundaries = [0, next(i for i, case in enumerate(cases) if case[0].startswith("widgets-")),
                  next(i for i, case in enumerate(cases) if case[0] == "prose"),
                  next(i for i, case in enumerate(cases) if case[0] == "ansi-cursor"),
                  next(i for i, case in enumerate(cases) if case[0] == "arena"), len(cases)]
    for group, (first, last) in enumerate(zip(boundaries, boundaries[1:]), 1):
        start = session.send(f"{group}r")
        pattern = printed_text("Choose a showcase") + ".*" + ".*".join(printed_text(title) for _, title in cases[first:last])
        session.expect(f"section {group} menu titles", pattern, start)
    start = session.send("1\r r")
    session.header("opened and paused", cases[0][0], start)

    for index, (case_id, title) in enumerate(cases):
        # 'n' is the app's next-case shortcut. Shell3 reserves Tab for a VMX
        # app's return to the shell, so literal Tab would not verify traversal.
        # 'r' clears and redraws, giving each check a complete fresh header.
        start = session.send("r" if index == 0 else "nr")
        session.header(f"case {index + 1}/{len(cases)} {case_id}", case_id, start, id=case_id, title=title)
        if case_id == "composer":
            marker = "ratatest-ssh-" + uuid.uuid4().hex[:10]
            start = session.send("i")
            # Unchanged characters inside the footer can be omitted by the
            # differential backend; the newly drawn EDITING prefix is enough.
            session.expect("composer editing", r"EDITING:", start)
            start = session.send(marker + "\rr")
            session.expect("composer submitted input", r"You:\s*" + re.escape(marker), start, marker=marker)
            for columns, rows in [(80, 24), (120, 40)]:
                start = len(session.data)
                session.geometry(columns, rows)
                session.drain(.25)
                session.send("r")
                session.header(f"PTY resize {columns}x{rows}", case_id, start, columns=columns, rows=rows)
        if case_id in {"ansi-cursor", "ansi-history", "ansi-probes", "ansi-modes", "input"}:
            start = session.send("a")
            session.expect(f"raw probe {case_id}", re.escape(RAW_MARKER), start)
            session.drain(.3)
            start = session.send(b"\x1b")
            returned = session.header(f"raw probe returned {case_id}", case_id, start)
            # Check the app's restoration bytes independently of the rendered text.
            restored = b"\x1b[r" in returned and b"\x1b[?2026l" in returned and b"\x1b[?2004h" in returned
            session.checks.append({"name": f"raw mode restoration {case_id}", "passed": restored})
            if not restored:
                raise RuntimeError(f"Raw probe {case_id} did not emit its mode restoration")

    start = session.send("q")
    session.expect("quit returns to Shell3", r"TrueOS\s+§", start)
    start = session.send("exit\r")
    session.expect("SSH shell closed cleanly", r"Bye\.", start)
    session.process.wait(timeout=5)
    if session.process.returncode != 0:
        raise RuntimeError(f"SSH exited with status {session.process.returncode}")


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--host", default="192.168.178.94")
    parser.add_argument("--port", type=int, default=22)
    parser.add_argument("--identity", type=Path, default=Path("~/.ssh/id_ed25519_trueos"))
    parser.add_argument("--known-hosts", type=Path, default=Path("~/.ssh/known_hosts"))
    parser.add_argument("--output", type=Path, required=True, help="New evidence directory")
    parser.add_argument("--timeout", type=float, default=5, help="Seconds per check")
    parser.add_argument("--startup-timeout", type=float, default=30, help="Seconds for SSH and Appstore startup")
    args = parser.parse_args()
    if not 1 <= args.port <= 65535 or args.timeout <= 0 or args.startup_timeout <= 0:
        parser.error("port must be 1..65535 and timeouts must be positive")
    args.identity = args.identity.expanduser().resolve()
    args.known_hosts = args.known_hosts.expanduser().resolve()
    if not args.identity.is_file() or not args.known_hosts.is_file():
        parser.error("identity and trusted known-hosts files must exist")
    cases = source_cases()
    args.output = args.output.expanduser().resolve()
    args.output.mkdir(parents=True, exist_ok=False)
    session = None
    result = {
        "host": args.host, "port": args.port, "passed": False,
        "scope": "SSH process/input/resize/ANSI transport; native ANSI parser and UI4 rendering are not covered",
        "case_inventory": [{"id": case_id, "title": title} for case_id, title in cases],
        "case_count": len(cases), "next_case_key": "n",
        "park_reentry": {"tested": False, "reason": "This run checks clean quit; retained-slot reentry is not exercised."},
        "capability_responses": {
            "emulated": True, "CPR": "synthetic origin (1,1) for Ratatui clear/save/restore; logged in query-replies.jsonl",
            "other_queries": "unanswered; terminal capabilities are not verified",
        },
    }
    try:
        session = Session(args, args.output)
        result["ssh_command"] = session.command
        verify(session, cases)
        result["passed"] = True
    except (OSError, RuntimeError, subprocess.TimeoutExpired) as error:
        result["error"] = str(error)
        if session and session.process.poll() is None:
            # Ctrl-Q quits Ratatest even when editing. Only issue 'exit' after
            # observing Shell3 again, so cleanup cannot become composer input.
            try:
                start = session.send(b"\x1b")
                session.drain(.2)
                session.send(b"\x11")
                session.expect("failure cleanup returns to Shell3", r"TrueOS\s+§", start, 2)
                session.send("exit\r")
                session.drain(.2)
            except (OSError, RuntimeError):
                pass
    finally:
        if session:
            result["checks"] = session.checks
            result["elapsed_seconds"] = round(time.monotonic() - session.started, 3)
            result["captured_bytes"] = len(session.data)
            result["capability_responses"]["CPR_reply_count"] = session.cpr_replies
            session.close()
        (args.output / "result.json").write_text(json.dumps(result, indent=2) + "\n")
    print(json.dumps({key: result[key] for key in ("passed", "case_count", "scope")}, indent=2))
    if not result["passed"]:
        raise SystemExit(result.get("error", "SSH verification failed"))


if __name__ == "__main__":
    main()
