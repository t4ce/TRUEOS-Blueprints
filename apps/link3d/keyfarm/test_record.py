"""Mapping and raw-event regression checks; never opens a physical keyboard."""
import contextlib
import io
import json
import os
from pathlib import Path
import tempfile
import unittest
from unittest.mock import patch

import record


class KeyfarmTests(unittest.TestCase):
    def setUp(self):
        self.directory = tempfile.TemporaryDirectory()
        self.addCleanup(self.directory.cleanup)
        self.path = Path(self.directory.name) / "mapping.json"
        self.model_hash, self.slots = record.model_slots()
        self.data = record.load(self.path, self.model_hash, self.slots, "de", "")
        self.xkb = record.Xkb("de", "")
        self.addCleanup(self.xkb.close)
        self.quiet = contextlib.redirect_stdout(io.StringIO())
        self.quiet.__enter__()
        self.addCleanup(self.quiet.__exit__, None, None, None)

    def press(self, code, scan=None):
        record.record_press(self.data, code, scan, self.xkb, {})

    def test_real_model_rows_and_special_positions(self):
        self.assertEqual([sum(slot["row"] == row for slot in self.slots)
                          for row in range(1, 6)], [14, 14, 13, 13, 8])
        self.assertEqual({slot["mesh"] for slot in self.slots}, set(range(63)) - {29})
        self.assertEqual(self.slots[27]["mesh"], 28)  # ISO Enter at end of Q row
        self.assertEqual(self.slots[57]["mesh"], 23)  # real spacebar
        for row in range(1, 6):
            xs = [slot["origin"][0] for slot in self.slots if slot["row"] == row]
            self.assertEqual(xs, sorted(xs))

    def test_space_joker_preserves_position(self):
        self.press(57)
        self.press(30, 0x70004)
        self.assertEqual(self.slots[0]["press"]["status"], "skipped")
        self.assertNotIn("linux_code", self.slots[0]["press"])
        self.assertEqual(self.slots[1]["press"]["label"], "a")
        self.assertEqual(self.slots[1]["press"]["scan"], 0x70004)
        self.assertEqual(record.next_slot(self.data)["slot"], 3)

    def test_real_spacebar_is_mapped(self):
        for _ in range(57):
            self.press(57)
        self.press(57)
        self.assertEqual(self.slots[57]["press"]["status"], "mapped")
        self.assertEqual(self.slots[57]["press"]["label"], "Space")

    def test_layout_legends_and_modifiers(self):
        self.press(42)
        self.press(21)  # German Z, not US Y; Shift tap must not latch.
        self.assertEqual(self.slots[0]["press"]["label"], "Shift")
        self.assertEqual(self.slots[1]["press"]["label"], "z")
        self.assertEqual([item["text"] for item in self.slots[1]["press"]["legends"]][:2],
                         ["z", "Z"])

    def test_save_resume_and_explicit_holes(self):
        self.press(1)
        self.press(57)
        record.save(self.path, self.data)
        resumed = record.load(self.path, self.model_hash, record.model_slots()[1], "de", "")
        self.assertEqual(record.next_slot(resumed)["slot"], 3)
        self.assertEqual(resumed["keys"][1]["press"]["status"], "skipped")
        self.assertIn("skipped", self.path.with_suffix(".txt").read_text())
        self.assertFalse(self.path.with_name("mapping.json.tmp").exists())
        with self.assertRaises(ValueError):
            record.load(self.path, "wrong-model", self.slots, "de", "")
        with self.assertRaises(ValueError):
            record.load(self.path, self.model_hash, self.slots, "us", "")

    @staticmethod
    def event(kind, code, value):
        return record.EVENT.pack(0, 0, kind, code, value)

    def capture_events(self, events):
        read_fd, write_fd = os.pipe()
        try:
            os.write(write_fd, b"".join(self.event(*event) for event in events))
            os.close(write_fd)
            write_fd = None
            record.capture(read_fd, self.data, self.path, self.xkb)
        finally:
            os.close(read_fd)
            if write_fd is not None:
                os.close(write_fd)

    def test_raw_events_repeat_duplicate_down_and_scan(self):
        for _ in range(60):
            self.press(57)
        self.capture_events([
            (4, 4, 0x70004), (1, 30, 1), (1, 30, 2), (1, 30, 1),
            (1, 30, 0), (0, 0, 0),
            (4, 4, 0x70005), (1, 48, 1), (1, 48, 0), (0, 0, 0),
        ])
        self.assertIsNone(record.next_slot(self.data))
        self.assertEqual([slot["press"]["label"] for slot in self.slots[-2:]], ["a", "b"])
        self.assertEqual(self.slots[-2]["press"]["scan"], 0x70004)
        self.assertEqual(self.slots[-1]["press"]["scan"], 0x70005)

    def test_escape_tap_maps_but_hold_pauses_without_advancing(self):
        for _ in range(61):
            self.press(57)
        self.capture_events([(1, 1, 1), (1, 1, 0), (0, 0, 0)])
        self.assertEqual(self.slots[-1]["press"]["label"], "Esc")
        del self.slots[-1]["press"]
        with patch.object(record.time, "monotonic", side_effect=[0, 3]):
            self.capture_events([(1, 1, 1), (0, 0, 0)])
        self.assertEqual(record.next_slot(self.data)["slot"], 62)

    def test_overflow_stops_before_mapping_wrong_slot(self):
        with self.assertRaisesRegex(OSError, "overflow"):
            self.capture_events([(0, 3, 0), (1, 30, 1)])
        self.assertEqual(record.next_slot(self.data)["slot"], 1)

    def test_undo_resumes_at_first_removed_slot(self):
        self.press(1)
        self.press(30)
        record.save(self.path, self.data)
        argv = ["record.py", "--layout", "de", "--variant", "", "--output", str(self.path),
                "--undo", "1"]
        with patch.object(record.sys, "argv", argv):
            record.main()
        self.assertEqual(record.next_slot(json.loads(self.path.read_text()))["slot"], 2)


if __name__ == "__main__":
    unittest.main()
