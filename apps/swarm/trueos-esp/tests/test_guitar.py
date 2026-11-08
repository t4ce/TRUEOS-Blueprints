"""Host checks of the actual sketch with mocked GPIO/PWM and wrapping ticks."""
import importlib.util
from pathlib import Path
import sys
import types
import unittest
from unittest.mock import patch


class Pin:
    OUT = 1

    def __init__(self, gpio, mode=None, value=0):
        self.gpio = gpio
        self.level = value

    def value(self, level):
        self.level = level


class PWM:
    def __init__(self, pin, freq):
        self.pulse = None

    def duty_ns(self, pulse):
        self.pulse = pulse

    def deinit(self):
        pass


spec = importlib.util.spec_from_file_location(
    "guitar", Path(__file__).resolve().parents[1] / "iot/guitar.py")
guitar = importlib.util.module_from_spec(spec)
with patch.dict(sys.modules, {"machine": types.SimpleNamespace(Pin=Pin, PWM=PWM)}):
    spec.loader.exec_module(guitar)

WRAP = 1 << 30


class GuitarTest(unittest.TestCase):
    def setUp(self):
        self.clock = patch.object(guitar, "time", types.SimpleNamespace(
            ticks_ms=lambda: 0,
            ticks_add=lambda now, delta: (now + delta) % WRAP,
            ticks_diff=lambda a, b: ((a - b + WRAP // 2) % WRAP) - WRAP // 2,
        ))
        self.clock.start()
        self.addCleanup(self.clock.stop)
        self.instrument = guitar.Guitar()
        self.addCleanup(self.instrument.close)

    def levels(self):
        return [[pin.level for pin in row] for row in self.instrument.rows]

    def test_frets_release_other_positions_and_stop(self):
        self.instrument.frets((1, 2, 4))
        self.assertEqual(self.levels(), [[1, 0, 0, 0], [0, 1, 0, 0], [0, 0, 0, 1]])
        self.instrument.frets((0, 3, 0))
        self.assertEqual(self.levels(), [[0, 0, 0, 0], [0, 0, 1, 0], [0, 0, 0, 0]])
        self.instrument.stop()
        self.assertEqual(self.levels(), [[0] * 4] * 3)

    def test_active_low_relays(self):
        with patch.object(guitar, "RELAY_ACTIVE", 0):
            self.instrument.frets((1, 0, 0))
            self.assertEqual(self.levels()[0], [0, 1, 1, 1])
            self.instrument.stop()
            self.assertEqual(self.levels(), [[1] * 4] * 3)

    def test_bar_sweeps_only_non_rest_beats_and_completes_across_wrap(self):
        self.instrument.load("BAR 60 000 100 300 ---", WRAP - 100)
        sweeps = 0
        while self.instrument.events:
            deadline, action, value = self.instrument.events[0]
            if action == "sweep":
                sweeps += 1
            reply = self.instrument.tick(deadline)
        self.assertEqual(sweeps, 3)
        self.assertEqual(reply, "DONE\n")
        self.assertEqual(self.levels(), [[0] * 4] * 3)

    def test_late_event_stops_instead_of_catching_up(self):
        self.instrument.load("BAR 60 444 444 444 444", 0)
        deadline = self.instrument.events[0][0]
        self.instrument.tick(deadline)
        deadline = self.instrument.events[0][0]
        self.assertIn("ERR late", self.instrument.tick(deadline + guitar.MAX_LATE_MS + 1))
        self.assertFalse(self.instrument.events)
        self.assertEqual(self.levels(), [[0] * 4] * 3)

    def test_invalid_frames_do_not_schedule_any_outputs(self):
        for command in ("BAR 0 000 000 000 000", "BAR 121 000 000 000 000",
                        "BAR 60 500 000 000 000", "BAR 60 00 000 000 000",
                        "BAR 60 0-- 000 000 000", "BAR 60 000", "garbage"):
            with self.subTest(command=command), self.assertRaises(ValueError):
                self.instrument.load(command, 0)
            self.assertFalse(self.instrument.events)

    def test_busy_bar_cannot_replace_active_schedule(self):
        self.instrument.load("BAR 60 000 000 000 000", 0)
        original = list(self.instrument.events)
        with self.assertRaisesRegex(ValueError, "busy"):
            self.instrument.load("BAR 60 444 444 444 444", 1)
        self.assertEqual(self.instrument.events, original)

    def test_tempo_rejects_insufficient_mechanical_budget(self):
        with patch.object(guitar, "SWEEP_MS", 1000):
            with self.assertRaisesRegex(ValueError, "beat too short"):
                guitar.parse_bar("BAR 120 000 000 000 000")

    def test_tcp_fragmentation_status_stop_and_disconnect(self):
        class Client:
            packets = iter((b"BAR 60 000 100", b" 300 ---\nSTATUS\n", b"STOP\n", b""))
            replies = []

            def settimeout(self, timeout):
                pass

            def recv(self, size):
                return next(self.packets)

            def sendall(self, message):
                self.replies.append(message)

        client = Client()
        guitar.serve_client(client, self.instrument)
        self.assertEqual(client.replies,
                         [b"READY GUITAR 1\n", b"OK BAR\n", b"BUSY\n", b"OK STOP\n"])
        self.assertFalse(self.instrument.events)


if __name__ == "__main__":
    unittest.main()
