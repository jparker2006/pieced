#!/usr/bin/env python3
"""Tests for scripts/sessions.py. Run: python3 -m unittest scripts/test_sessions.py"""

import contextlib
import io
import json
import os
import sys
import tempfile
import unittest

sys.path.insert(0, os.path.dirname(os.path.abspath(__file__)))
import sessions  # noqa: E402


def session(root, name, launch_ms, kind, verdict="N/A", final=True, battery=True):
    os.makedirs(os.path.join(root, name))
    doc = {
        "final": final,
        "started_unix_s": 1790500000,
        "commit": "abc123",
        "launch": {
            "ms": launch_ms,
            "kind": kind,
            "boot_phases": [{"phase": "window", "ms": 300.0}, {"phase": "playing", "ms": 2000.0}],
        },
        "power_samples": [
            {"t_ms": 0, "source": "Battery Power" if battery else "AC Power", "low_power_mode": True}
        ],
        "play_s": 400.0,
        "counted_play_s": 380.0,
        "occluded_counted_ms": 0.0,
        "s2": {"verdict": verdict, "reasons": [] if verdict == "PASS" else ["play=10s<300s"]},
        "s2_line": f"PIECED_S2 {verdict} 16.70 17.10 0>25ms 99.50%<18ms ok",
    }
    with open(os.path.join(root, name, "session.json"), "w", encoding="utf-8") as f:
        json.dump(doc, f)


def run(args):
    out = io.StringIO()
    old = sys.argv
    sys.argv = ["sessions.py"] + args
    try:
        with contextlib.redirect_stdout(out):
            sessions.main()
    finally:
        sys.argv = old
    return out.getvalue()


class SessionsTest(unittest.TestCase):
    def setUp(self):
        self.tmp = tempfile.TemporaryDirectory()
        self.root = self.tmp.name
        session(self.root, "20260927-100000", 8721.0, "cold")
        session(self.root, "20260927-110000", 2100.0, "warm", final=False)
        session(self.root, "20260927-120000", 1800.0, "warm", verdict="FAIL", battery=False)
        session(self.root, "20260927-130000", 1500.0, "warm", verdict="PASS")
        os.makedirs(os.path.join(self.root, "notes"))

    def tearDown(self):
        self.tmp.cleanup()

    def test_lists_every_session(self):
        out = run(["--root", self.root])
        self.assertEqual(len(out.strip().splitlines()), 5)
        self.assertIn("8721 ms cold", out)
        self.assertIn("(no clean quit)", out)
        self.assertIn("batt 0/1", out)

    def test_s2_shows_only_qualifying_sessions(self):
        out = run(["--root", self.root, "--s2"])
        self.assertNotIn("20260927-100000", out)
        self.assertIn("20260927-120000", out)
        self.assertIn("First PASS: 20260927-130000", out)

    def test_launches_find_three_warm_in_a_row(self):
        out = run(["--root", self.root, "--launches"])
        self.assertIn("window=300 playing=2000", out)
        self.assertIn("3 warm launches in a row < 5000 ms: 20260927-110000", out)
        session(self.root, "20260927-140000", 6000.0, "warm")
        out = run(["--root", self.root, "--launches", "--limit", "2"])
        self.assertIn("No run of 3 warm launches", out)

    def test_empty_root(self):
        out = run(["--root", os.path.join(self.root, "missing")])
        self.assertIn("No sessions", out)


if __name__ == "__main__":
    unittest.main()
