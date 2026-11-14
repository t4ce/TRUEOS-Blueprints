#!/usr/bin/env python3
"""Exercise the real host VNNI service over loopback; no live Spirit side effects.

Build the host binary first, then pass --binary if it is outside the ordinary
Blueprint repository target directory. Only the final native Spirit submission
is replaced by --dry-run; HTTP, queueing, cadence and inference are production.
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
LABELS = {"neutral", "wave", "happy", "sad", "affection", "disapprove", "agree", "confused", "idea", "angry"}


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
        assert health["protocol"] == "lily2vec-v1", health
        assert health["backend"] == "avx-vnni" and health["ready"] is True, health
        assert health["received"] == 0, health
        print("PASS: HTTP health reports a ready AVX-VNNI backend")

        source = "integration-response"
        received = 0
        def post(sequence, text=None, clear=False):
            nonlocal received
            data = {"source": source, "sequence": sequence}
            if text is not None:
                data["text"] = text
            if clear:
                data["clear"] = True
            status, reply = request(base, "/v1/snapshots", data)
            assert status == 202, (status, reply)
            received += 1
            state = "stale" if not reply["accepted"] else ("clear" if clear else "queued")
            notifications.wait(received, f"recv seq={sequence} chars={len(text or '')} {state}")
            return reply

        assert post(1, "hi")["accepted"] is True
        assert events.wait(source, 1)["label"] == "wave"
        notifications.wait(received, "result=wave spirit=dry-run")
        print("PASS: a queued snapshot reaches the real VNNI classifier")

        # A fresh cue starts the global cadence window. All three network
        # submissions fit inside it, so only the newest pending text wins.
        for sequence, text in [(2, "i love you"), (3, "NO"), (4, "i dont feel well")]:
            assert post(sequence, text)["accepted"] is True
        assert events.wait(source, 4)["label"] == "sad"
        notifications.wait(received, "result=sad spirit=dry-run")
        events.assert_absent(source, {2, 3}, 0)
        print("PASS: the bounded mailbox coalesces rapid snapshots to the latest")

        # A late retry cannot replace a newer source sequence.
        assert post(3, "i am furious")["accepted"] is False
        assert post(5, "i love you")["accepted"] is True
        assert post(6, clear=True)["accepted"] is True
        events.assert_absent(source, {2, 3, 5, 6}, 2.8)
        assert post(7, "yes that will do")["accepted"] is True
        assert events.wait(source, 7)["label"] == "agree"
        notifications.wait(received, "result=agree spirit=dry-run")
        print("PASS: stale sequences are ignored and cancellation clears pending work")

        invalid = [
            {"source": "", "sequence": 1, "text": "hi"},
            {"source": "x" * 129, "sequence": 1, "text": "hi"},
            {"source": "bad source", "sequence": 1, "text": "hi"},
            {"source": source, "sequence": 0, "text": "hi"},
            {"source": source, "sequence": 8, "text": ""},
            {"source": source, "sequence": 8, "text": "x" * 769},
            {"source": source, "sequence": 8, "text": "🌼" * 769},
            {"source": source, "sequence": 8, "text": "x" * 9000},
        ]
        for data in invalid:
            status, reply = request(base, "/v1/snapshots", data)
            if len(data["text"]) == 9000:
                assert status == 413, (status, reply)  # HTTP body extraction rejects this first.
            else:
                assert status == 400, (status, reply)
                received += 1
                notifications.wait(received, f"recv seq={data['sequence']} chars={len(data['text'])} rejected")
        status, _ = request(base, "/v1/snapshots", raw=b"{not json")
        assert 400 <= status < 500, status
        events.assert_absent(source, {8}, 0.1)
        assert post(8, "i love you")["accepted"] is True
        assert events.wait(source, 8)["label"] == "affection"
        notifications.wait(received, "result=affection spirit=dry-run")
        print("PASS: malformed/oversized requests neither queue work nor consume a valid sequence")

        # Repeat suppression affects Spirit, not observability of classification.
        assert post(9, "i love you")["accepted"] is True
        notifications.wait(received, "result=affection spirit=held-repeat")
        events.assert_absent(source, {9}, 0)
        status, health = request(base, "/health")
        assert status == 200 and health["received"] == received, (status, health, received)
        assert health["classified"] == 5 and health["signaled"] == 4, health
        assert not notifications.errors, notifications.errors
        print("PASS: timestamped receipts correlate with single-label results, including held repeats")
        print("6 HTTP integration checks passed; Spirit signaling was dry-run only")
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
