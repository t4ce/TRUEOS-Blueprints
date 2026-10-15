#!/usr/bin/env python3
"""Exercise the published Arena under live SSH load for a bounded 26 seconds.

Uses verify_ssh.Session's trusted OpenSSH connection and synthetic CPR replies.
Captures a complete four-scene automatic tour without resetting its tick, edits
the Mission composer, toggles overlays and selection, and resizes the PTY while
animation runs. Separate paused redraws verify each Arena scene deterministically.
Captured bytes and elapsed time are transport measurements; the configured
40 frames/s target does not establish rendered or presented frame rate.

Example:
  python3 apps/ratatest/verify_stress_ssh.py --output /tmp/ratatest-stress-ssh

Only Python's standard library and OpenSSH are required. The output directory
must be new; identity and trusted known-hosts files must already exist.
"""

import argparse
import codecs
import json
import math
from pathlib import Path
import re
import subprocess
import time
import uuid

from verify_ssh import Session, printed_text, source_cases


TOUR_SECONDS = 26


class HeaderObserver:
    """Reconstruct only ASCII row 1, including incremental header substitutions.

    This small transport observer recognizes cursor addressing and erasure used
    by CrosstermBackend. It does not validate terminal rendering or Unicode width.
    Keeping the header avoids requiring a redraw that would reset the live tick.
    """

    def __init__(self, case_ids, epoch, columns=120):
        self.case_ids = set(case_ids)
        self.epoch = epoch
        self.offset = 0
        self.decoder = codecs.getincrementaldecoder("utf-8")("replace")
        self.state = "text"
        self.parameters = ""
        self.saved = (0, 0)
        self.col = self.row = 0
        self.columns = columns
        self.header = [" "] * columns
        self.transitions = []

    def resize(self, columns):
        self.columns = columns
        self.header = [" "] * columns

    def capture(self, session):
        data = bytes(session.data[self.offset:])
        self.offset = len(session.data)
        self.feed(data)

    def feed(self, data):
        for ch in self.decoder.decode(data):
            if self.state == "osc":
                if ch == "\x07":
                    self.state = "text"
                elif ch == "\x1b":
                    self.state = "osc-escape"
            elif self.state == "osc-escape":
                self.state = "text" if ch == "\\" else "osc"
            elif self.state == "escape":
                self.state = "text"
                if ch == "[":
                    self.state, self.parameters = "csi", ""
                elif ch == "]":
                    self.state = "osc"
                elif ch == "7":
                    self.saved = self.col, self.row
                elif ch == "8":
                    self.col, self.row = self.saved
            elif self.state == "csi":
                if "@" <= ch <= "~":
                    self.command(ch)
                    self.state = "text"
                elif ch == "\x1b":
                    self.state = "escape"
                elif len(self.parameters) < 128:
                    self.parameters += ch
            elif ch == "\x1b":
                self.state = "escape"
            elif ch == "\r":
                self.col = 0
            elif ch in "\n\v\f":
                self.row += 1
            elif ch == "\b":
                self.col = max(0, self.col - 1)
            elif ch == "\t":
                self.col = min(self.columns - 1, (self.col // 8 + 1) * 8)
            elif ch >= " " and ch != "\x7f":
                if self.col >= self.columns:
                    self.col, self.row = 0, self.row + 1
                if self.row == 0:
                    self.header[self.col] = ch
                    self.record()
                self.col += 1

    def command(self, final):
        if not re.fullmatch(r"[0-9;]*", self.parameters):
            return
        params = [int(part or "0") for part in self.parameters.split(";")]
        count = max(1, params[0])
        if final in "Hf":
            self.row = max(1, params[0]) - 1
            self.col = max(1, params[1] if len(params) > 1 else 1) - 1
        elif final == "G":
            self.col = count - 1
        elif final == "d":
            self.row = count - 1
        elif final == "A":
            self.row = max(0, self.row - count)
        elif final == "B":
            self.row += count
        elif final == "C":
            self.col = min(self.columns - 1, self.col + count)
        elif final == "D":
            self.col = max(0, self.col - count)
        elif final in "EF":
            self.row = max(0, self.row + (count if final == "E" else -count))
            self.col = 0
        elif final == "s":
            self.saved = self.col, self.row
        elif final == "u":
            self.col, self.row = self.saved
        elif final in "JK":
            mode = params[0]
            if mode in (2, 3) and (final == "J" or self.row == 0):
                self.header = [" "] * self.columns
            elif self.row == 0 and mode in (0, 1):
                first, last = (self.col, self.columns) if mode == 0 else (0, self.col + 1)
                self.header[first:last] = [" "] * len(self.header[first:last])

    def record(self):
        header = "".join(self.header)
        match = re.search(r"Ratatui\s*0\.30\.2\s*([a-z0-9-]+)\s+STRESS\b", header)
        if not match or match[1] not in self.case_ids:
            return
        if self.transitions and self.transitions[-1]["id"] == match[1]:
            return
        self.transitions.append({
            "id": match[1], "elapsed_seconds": round(time.monotonic() - self.epoch, 3),
            "capture_end_offset": self.offset, "header": header.rstrip(),
        })


def measured_check(session, name, passed, **details):
    session.checks.append({"name": name, "passed": passed, **details})
    print(f"{'PASS' if passed else 'FAIL'} {name}", flush=True)
    if not passed:
        raise RuntimeError(name)


def verify(session, cases, result):
    ids = [case_id for case_id, _ in cases]
    session.expect("Shell3 SSH ready", r"TrueOS\s+§", 0, session.args.startup_timeout)
    start = session.send("§§ratatest\r")
    session.expect("published app launch", r"RATATEST", start, session.args.startup_timeout)
    # This initial menu redraw precedes the live measurement and stress start.
    start = session.send("r")
    menu = printed_text("Choose a showcase") + ".*" + ".*".join(printed_text(title) for _, title in cases)
    session.expect("default Arena menu", menu, start)

    epoch = time.monotonic()
    observer = HeaderObserver(ids, epoch)
    observer.capture(session)
    tour_offset = session.send("s+i")
    result["live_actions"] = [{"elapsed_seconds": 0, "action": "start stress, raise load to 3, edit Mission composer"}]
    result["automatic_tour"] = {"requested_seconds": TOUR_SECONDS, "transitions": observer.transitions}
    session.expect("live load 3 ribbon", r"\bL3\b", tour_offset)
    session.expect("live composer editing", printed_text("COMPOSER / EDITING"), tour_offset)
    marker = "stress-" + uuid.uuid4().hex[:12]
    start = session.send(b"\x1b[200~" + marker.encode() + b"\x1b[201~")
    session.expect("live composer accepts bracketed paste", re.escape(marker), start, marker=marker)
    start = session.send("\r")
    session.expect("Enter finishes live composer editing", printed_text("i to edit"), start)
    measured_check(session, "composer completed in the first Mission interval", time.monotonic() - epoch < 6)
    session.send("\x1b[B\x1b[A")
    result["live_actions"].append({"elapsed_seconds": round(time.monotonic() - epoch, 3), "action": "selection Down then Up"})
    observer.capture(session)

    # Toggles and PTY resizes occur during the automatic tour. No live 'r' key
    # is sent, and no pause can postpone the six-second scene transitions.
    actions = [(2, "overlay", None), (3, "overlay", None),
               (4, "resize", (80, 24)), (11, "resize", (100, 32)),
               (18, "resize", (120, 40))]
    next_action = 0
    while time.monotonic() - epoch < TOUR_SECONDS:
        elapsed = time.monotonic() - epoch
        while next_action < len(actions) and elapsed >= actions[next_action][0]:
            _, action, geometry = actions[next_action]
            if geometry:
                session.geometry(*geometry)
                observer.resize(geometry[0])
            else:
                session.send("o")
            result["live_actions"].append({
                "elapsed_seconds": round(elapsed, 3), "action": action,
                **({"columns": geometry[0], "rows": geometry[1]} if geometry else {}),
            })
            next_action += 1
        session.receive(min(.1, max(0, TOUR_SECONDS - (time.monotonic() - epoch))))
        observer.capture(session)
        if session.process.poll() is not None:
            raise RuntimeError("SSH exited during the live automatic tour")
    elapsed = time.monotonic() - epoch
    captured = len(session.data) - tour_offset
    observed = [transition["id"] for transition in observer.transitions]
    result["automatic_tour"].update({
        "elapsed_seconds": round(elapsed, 3), "captured_bytes": captured,
        "captured_bytes_per_second": round(captured / elapsed, 3),
        "start_offset": tour_offset, "end_offset": len(session.data),
        "tick_reset_during_live_interval": False,
        "geometry_path": [[120, 40], [80, 24], [100, 32], [120, 40]],
    })
    measured_check(session, "automatic tour visits all four Arena scenes in order",
                   observed[:len(ids)] == ids, observed_ids=observed, expected_ids=ids)
    measured_check(session, "live tour delivers terminal bytes", captured > 0,
                   captured_bytes=captured, elapsed_seconds=round(elapsed, 3))

    start = session.send(" r")
    session.expect("live tour pauses responsively", rf"Ratatui\s*0\.30\.2\s*(?:{'|'.join(map(re.escape, ids))})\s*PAUSED", start)
    start = session.send("5\rr")
    session.header("manual Arena opens paused", ids[0], start)
    result["manual_windows"] = []
    for index, (case_id, title) in enumerate(cases):
        if index:
            start = session.send("nr")
            session.header(f"manual scene {case_id}", case_id, start, title=title)
        if index == 0:
            start = session.send("or")
            session.expect("paused Mission overlay opens", printed_text("APPROVAL / o closes"), start)
            start = session.send("or")
            session.expect("paused Mission base redraws after overlay closes", printed_text("LIVE TRANSCRIPT"), start)
        started = time.monotonic()
        offset = session.send(" ")
        session.drain(.8)
        live_bytes = len(session.data) - offset
        live_elapsed = time.monotonic() - started
        start = session.send(" r")
        session.header(f"manual scene runs and pauses {case_id}", case_id, start)
        window = {"id": case_id, "captured_bytes": live_bytes, "elapsed_seconds": round(live_elapsed, 3)}
        result["manual_windows"].append(window)
        measured_check(session, f"manual scene emits live output {case_id}", live_bytes > 0, **window)

    # Cross the automatic six-second scene boundary while editing. The
    # composer must stay visible until the user submits or leaves the field.
    hold_epoch = time.monotonic()
    held = HeaderObserver(ids, hold_epoch)
    held.capture(session)
    start = session.send("si")
    session.expect("composer holds the automatic tour", printed_text("COMPOSER / EDITING"), start)
    while time.monotonic() - hold_epoch < 6.4:
        session.receive(.1)
        held.capture(session)
    held_ids = [transition["id"] for transition in held.transitions]
    # Prior manual headers can be reconstructed before the new live interval;
    # inspect only transitions at/after the new full Mission header.
    last_mission = max(index for index, case_id in enumerate(held_ids) if case_id == ids[0])
    measured_check(session, "editing stays in Mission beyond the automatic scene boundary",
                   held_ids[last_mission:] == [ids[0]],
                   held_seconds=round(time.monotonic() - hold_epoch, 3), observed_ids=held_ids)
    session.send("\r")
    start = session.send("q")
    session.expect("quit returns to Shell3", r"TrueOS\s+§", start)
    start = session.send("exit\r")
    session.expect("SSH shell closed cleanly", r"Bye\.", start)
    session.process.wait(timeout=5)
    measured_check(session, "SSH exit status is zero", session.process.returncode == 0,
                   returncode=session.process.returncode)


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
    if not 1 <= args.port <= 65535 or any(not math.isfinite(value) or value <= 0 for value in (args.timeout, args.startup_timeout)):
        parser.error("port must be 1..65535 and timeouts must be finite and positive")
    if not args.host or args.host.startswith("-") or any(ch.isspace() for ch in args.host):
        parser.error("host must be a nonempty hostname/address, optionally prefixed with user@")
    args.identity = args.identity.expanduser().resolve()
    args.known_hosts = args.known_hosts.expanduser().resolve()
    if not args.identity.is_file() or not args.known_hosts.is_file():
        parser.error("identity and trusted known-hosts files must exist")
    inventory = source_cases()
    cases = inventory[next(index for index, (case_id, _) in enumerate(inventory) if case_id == "arena"):]
    if len(cases) != 4:
        parser.error("this bounded tour expects exactly four source-declared Arena scenes")
    args.output = args.output.expanduser().resolve()
    args.output.mkdir(parents=True, exist_ok=False)
    session = None
    result = {
        "host": args.host, "port": args.port, "passed": False,
        "scope": "SSH live process/input/PTY resize/ANSI transport; native ANSI parser and UI4 rendering are not covered",
        "arena_inventory": [{"id": case_id, "title": title} for case_id, title in cases],
        "configured_load": 3, "configured_target_fps": 40,
        "frame_rate_scope": "Configured target only; captured bytes and elapsed time do not measure presented frame rate",
        "capability_responses": {
            "emulated": True, "CPR": "synthetic origin (1,1); logged in query-replies.jsonl",
            "other_queries": "unanswered; terminal capabilities are not verified",
        },
    }
    try:
        session = Session(args, args.output)
        result["ssh_command"] = session.command
        verify(session, cases, result)
        result["passed"] = True
    except (OSError, RuntimeError, subprocess.TimeoutExpired) as error:
        result["error"] = str(error)
        if session and session.process.poll() is None:
            try:
                start = session.send(b"\x11")
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
    print(json.dumps({key: result[key] for key in ("passed", "scope", "captured_bytes") if key in result}, indent=2))
    if not result["passed"]:
        raise SystemExit(result.get("error", "SSH stress verification failed"))


if __name__ == "__main__":
    main()
