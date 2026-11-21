#!/usr/bin/env python3
"""Exercise the real host VNNI service over loopback; no live Spirit side effects.

Build the host binary first, then pass --binary if it is outside the ordinary
Blueprint repository target directory. Only the final native Spirit submission
is replaced by --dry-run; HTTP, deduplication and inference are production.
"""
from __future__ import annotations

import argparse
import json
import re
import socket
import subprocess
import threading
import time
import urllib.error
import urllib.request
from pathlib import Path

APP = Path(__file__).resolve().parents[1]
REPO = APP.parents[1]
LABELS = {
    "Angry", "Blush", "Confused", "Cry", "Disapprove", "Drink", "Eat", "Flirt",
    "Wave", "Tired", "Grimace", "Silly", "Scared", "Nod", "Idea", "Happy", "Static",
}


class Events:
    def __init__(self, output):
        self.items = []
        self.errors = []
        self.condition = threading.Condition()
        self.thread = threading.Thread(target=self.read, args=(output,), daemon=True)
        self.thread.start()

    @staticmethod
    def decode(line):
        item = json.loads(line)
        assert set(item) == {"source", "sequence", "label", "accepted"}, item
        assert item["label"] in LABELS and item["accepted"] is True, item
        return item

    def read(self, output):
        for line in output:
            try:
                item = self.decode(line)
                if item is None:
                    continue
                with self.condition:
                    self.items.append(item)
                    self.condition.notify_all()
            except Exception as error:
                with self.condition:
                    self.errors.append(f"invalid output event {line!r}: {error}")
                    self.condition.notify_all()

    def wait_for(self, predicate, description, timeout=5.0):
        deadline = time.monotonic() + timeout
        with self.condition:
            while True:
                if self.errors:
                    raise AssertionError(self.errors)
                for item in self.items:
                    if predicate(item):
                        return item
                remaining = deadline - time.monotonic()
                if remaining <= 0:
                    raise AssertionError(f"missing {description}; got {self.items}")
                self.condition.wait(remaining)

    def wait(self, source, sequence, timeout=5.0):
        return self.wait_for(lambda item: item["source"] == source and item["sequence"] == sequence,
                             f"event {source}:{sequence}", timeout)

    def assert_absent(self, source, sequences, duration):
        deadline = time.monotonic() + duration
        with self.condition:
            while True:
                assert not self.errors, self.errors
                assert not [item for item in self.items if item["source"] == source and item["sequence"] in sequences], self.items
                remaining = deadline - time.monotonic()
                if remaining <= 0:
                    return
                self.condition.wait(remaining)


class Notifications(Events):
    pattern = re.compile(r"lily2vec #([1-9][0-9]*) ((?:[01][0-9]|2[0-3]):[0-5][0-9]:[0-5][0-9]\.[0-9]{3}Z) (.+)\n?")

    def __init__(self, output):
        self.lines = []
        super().__init__(output)

    def decode(self, line):
        self.lines.append(line)
        if not line.startswith("lily2vec #"):
            return None  # READY and other startup diagnostics are not notifications.
        match = self.pattern.fullmatch(line)
        assert match, line
        return {"request": int(match[1]), "timestamp": match[2], "event": match[3]}

    def wait(self, number, event, timeout=5.0):
        return self.wait_for(lambda item: item["request"] == number and item["event"] == event,
                             f"notification #{number} {event}", timeout)


def request(base, path, data=None, raw=None):
    body = raw if raw is not None else (None if data is None else json.dumps(data, ensure_ascii=False).encode())
    headers = {} if body is None else {"Content-Type": "application/json"}
    req = urllib.request.Request(base + path, data=body, headers=headers)
    try:
        with urllib.request.urlopen(req, timeout=2) as response:
            payload = response.read()
            return response.status, json.loads(payload) if payload else None
    except urllib.error.HTTPError as error:
        payload = error.read()
        try:
            value = json.loads(payload)
        except (ValueError, UnicodeError):
            value = payload.decode(errors="replace")
        return error.code, value


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--binary", type=Path, default=REPO / "target/x86_64-unknown-linux-gnu/release/lily2vec")
    args = parser.parse_args()
    binary = args.binary.resolve()
    if not binary.is_file():
        parser.error(f"binary missing: {binary}; build the host release binary first")
    with socket.socket() as reserved:
        reserved.bind(("127.0.0.1", 0))
        port = reserved.getsockname()[1]
    base = f"http://127.0.0.1:{port}"
    process = subprocess.Popen([str(binary), "--serve", "--dry-run", "--port", str(port)],
                               stdin=subprocess.DEVNULL, stdout=subprocess.PIPE,
                               stderr=subprocess.PIPE, text=True, bufsize=1)
    events = Events(process.stdout)
    notifications = Notifications(process.stderr)
    try:
        deadline = time.monotonic() + 10
        while True:
            if process.poll() is not None:
                raise AssertionError(f"service exited with {process.returncode}")
            try:
                status, health = request(base, "/health")
                if status == 200:
                    break
            except (OSError, urllib.error.URLError):
                pass
            if time.monotonic() >= deadline:
                raise AssertionError("service did not become ready")
            time.sleep(0.025)
        assert health["protocol"] == "lily2vec-v2", health
        assert health["backend"] == "avx-vnni" and health["ready"] is True, health
        assert health["received"] == 0, health
        print("PASS: HTTP health reports a ready AVX-VNNI backend")

        for text, label in [("hi", "Waving"), ("x " * 4500 + "i dont feel well", "Cry")]:
            assert request(base, "/v1/classify", {"text": text}) == (200, {"label": label})
        for text in [" ", "🙂" * 4097]:
            assert request(base, "/v1/classify", {"text": text})[0] == 400
        assert request(base, "/v1/classify", {"text": "a" * 100_000})[0] == 413
        assert request(base, "/health") == (200, health)
        assert not events.items and not notifications.items
        print("PASS: classification returns a per-request label without touching Spirit state")

        source = "integration-response"
        def turn(sequence, text):
            return {"source": source, "sequence": sequence, "text": text}

        # A legacy snapshot is accepted by the transport, logged, and ignored.
        assert request(base, "/v1/snapshots", turn(1, "hi")) == (
            202, {"accepted": False, "deprecated": True}
        )
        assert request(base, "/health") == (200, health)
        events.assert_absent(source, {1}, 0.1)
        assert any("WARN lily2vec: deprecated snapshot signal ignored" in line for line in notifications.lines)

        examples = [
            (1, "hi", "Wave"),
            (2, "i dont feel well", "Cry"),
            (3, "drink", "Drink"),
            (4, "i love you", "Flirt"),
            (5, "i love you", "Flirt"),
            (6, "unrecognizablexyz", "Static"),
        ]
        for sequence, text, label in examples:
            assert request(base, "/v1/turns", turn(sequence, text)) == (202, {"accepted": True, "label": label})
            assert events.wait(source, sequence)["label"] == label
            state = "idle" if label == "Static" else "dry-run"
            notifications.wait(sequence, f"turn seq={sequence} result={label} spirit={state}")

        assert request(base, "/v1/turns", turn(6, "hi")) == (202, {"accepted": False, "stale": True})
        assert request(base, "/v1/turns", turn(3, "hi")) == (202, {"accepted": False, "stale": True})
        assert len(events.items) == len(examples), events.items

        invalid = [
            ({**turn(7, "hi"), "source": ""}, 400),
            ({**turn(7, "hi"), "source": "x" * 129}, 400),
            ({**turn(7, "hi"), "source": "bad source"}, 400),
            (turn(0, "hi"), 400),
            (turn(7, ""), 400),
            (turn(7, "x" * 769), 400),
            (turn(7, "🌼" * 769), 400),
            (turn(7, "x" * 9000), 413),
        ]
        for data, expected in invalid:
            assert request(base, "/v1/turns", data)[0] == expected, data
        status, _ = request(base, "/v1/turns", raw=b"{not json")
        assert 400 <= status < 500, status
        assert request(base, "/v1/turns", turn(7, "grimace")) == (202, {"accepted": True, "label": "Grimace"})
        assert events.wait(source, 7)["label"] == "Grimace"

        status, health = request(base, "/health")
        assert status == 200 and health["classified"] == 7 and health["signaled"] == 6, health
        assert not notifications.errors, notifications.errors
        print("PASS: completed turns each classify once, repeated labels can play, and retries and legacy snapshots add no action")
    except BaseException:
        print("Service diagnostics:\n" + "".join(notifications.lines)[-6000:])
        raise
    finally:
        process.terminate()
        try:
            process.wait(timeout=5)
        except subprocess.TimeoutExpired:
            process.kill()
            process.wait(timeout=5)
        events.thread.join(timeout=1)
        notifications.thread.join(timeout=1)

if __name__ == "__main__":
    main()
