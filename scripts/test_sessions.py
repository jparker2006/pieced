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


def session(root, name, launch_ms, kind, verdict="N/A", final=True, battery=True, wave=None):
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
    if wave is not None:
        doc["waves"] = {"max_wave": wave, "runs": [{"seed": 1, "start_wave": 1, "max_wave": wave}]}
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
        # Neither reached wave 6 (one predates the wave record).
        self.assertIn("No A7 session yet: it needs a PASS that reached wave >= 6.", out)

    def test_a7_needs_a_pass_that_reached_wave_six(self):
        session(self.root, "20260927-140000", 1500.0, "warm", verdict="PASS", wave=5)
        session(self.root, "20260927-150000", 1500.0, "warm", verdict="FAIL", wave=8)
        session(self.root, "20260927-160000", 1500.0, "warm", verdict="PASS", wave=7)
        out = run(["--root", self.root, "--s2"])
        rows = {line.split()[2]: line for line in out.splitlines() if "PIECED_S2" in line}
        self.assertRegex(rows["20260927-140000"], r"\s5\s+PIECED_S2 PASS")
        self.assertRegex(rows["20260927-130000"], r"\s-\s+PIECED_S2 PASS")
        self.assertIn(
            "First A7 session (PASS at wave >= 6): 20260927-160000 on abc123, wave 7", out
        )

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

    def test_spikes_attribute_each_spike_to_the_bucket_that_ran_long(self):
        write_frames(os.path.join(self.root, "20260927-130000"), attributed=True)
        out = run(["--root", self.root, "--spikes", "latest"])
        self.assertIn("Session 20260927-130000", out)
        self.assertIn("3 spikes > 25 ms in 2 runs", out)
        cause = {line.split()[0]: int(line.split()[1]) for line in cause_lines(out)}
        self.assertEqual(cause, {"graph": 2, "update": 1})
        # Events count on the spike row or the row before, so the Update
        # overrun right after a compile is tagged with it too.
        self.assertRegex(out, r"pipeline_compile\s+3\s+100\.0% of spikes")
        self.assertRegex(out, r"knight_spawn\s+1\s+33\.3% of spikes")
        self.assertIn("1 late-then-early pairs", out)
        self.assertIn("dt= 40.0 graph", out)

    def test_spikes_on_an_old_session_give_a_timeline(self):
        write_frames(os.path.join(self.root, "20260927-100000"), attributed=False)
        out = run(["--root", self.root, "--spikes", "20260927-100000"])
        self.assertIn("predates the attribution columns: 3 spikes", out)
        self.assertIn("20-30", out)

    def test_gpu_prints_each_pass_against_its_budget(self):
        write_frames(os.path.join(self.root, "20260927-130000"), attributed=True, gpu=True)
        out = run(["--root", self.root, "--gpu", "latest"])
        self.assertIn("Session 20260927-130000", out)
        # 1 frame in 8 timed over the counted 20 s, every 4th of those bare.
        self.assertRegex(out, r"(\d+) full and (\d+) bare GPU samples")
        full, bare = map(int, __import__("re").search(r"(\d+) full and (\d+) bare", out).groups())
        self.assertLessEqual(abs(bare * 3 - full), 3)
        lines = {line.split()[0]: line for line in out.splitlines() if line.startswith("  ")}
        # world: 4.0 ms, 6.0 ms on every 10th full sample (p95 lands there).
        self.assertRegex(lines["world"], r"world\s+4\.\d\d\s+6\.00\s+4\.50\s+\d+\s+OVER")
        self.assertRegex(lines["effects"], r"1\.50\s+1\.50\s+2\.00\s+\d+\s+ok")
        self.assertRegex(lines["ui"], r"0\.40\s+0\.40\s+0\.50")
        self.assertIn("n/a", lines["outlines"])
        self.assertIn("farres=half", lines["far"])
        self.assertRegex(lines["total"], r"total\s+\S+\s+13\.00\s+12\.00\s+\d+\s+OVER")
        self.assertIn("slack", lines)
        # Every 10th timed sample is a 13 ms full one; bare ones are at 6.4.
        self.assertIn("GPU total > 12 ms on 10.0% of timed frames", out)
        self.assertIn("Timing cost (mean full - mean bare total): 1.40 ms", out)
        self.assertIn("display pacing, not the GPU", out)

    def test_gpu_on_a_session_without_gpu_columns(self):
        write_frames(os.path.join(self.root, "20260927-130000"), attributed=True)
        with open(os.path.join(self.root, "20260927-130000", "frames.csv"), encoding="utf-8") as f:
            lines = f.read().splitlines()
        old = [",".join(line.split(",")[: HEADER.count(",") - 7]) for line in lines]
        with open(os.path.join(self.root, "20260927-130000", "frames.csv"), "w", encoding="utf-8") as f:
            f.write("\n".join(old) + "\n")
        out = run(["--root", self.root, "--gpu", "20260927-130000"])
        self.assertIn("predates per-pass GPU timing", out)

    def test_spikes_for_a_missing_session(self):
        out = run(["--root", self.root, "--spikes", "20990101-000000"])
        self.assertIn("No session '20990101-000000'", out)


HEADER = (
    "frame,t_ms,dt_ms,state,occluded,pre_ms,fixed_ms,physics_ms,ticks,update_ms,post_ms,"
    "extract_ms,prepare_ms,acquire_ms,graph_ms,render_end_ms,idle_ms,vsync_dt_ms,gpu_ms,work_ms,"
    "knights,knights_spawned,orbs,orbs_fired,shots,damage,placed,cracked,broken,particles,debris,"
    "spell_fx,potions,damage_numbers,voices,voices_started,pipelines_compiled,entities,"
    "gpu_frame,gpu_full,gpu_world_ms,gpu_outlines_ms,gpu_far_ms,gpu_effects_ms,gpu_ui_ms,gpu_post_ms"
)


def write_frames(folder, attributed, gpu=False):
    """30 s of play at 60 fps with three spikes at 20 s: a 40 ms first-use
    compile in the render graph (after a knight spawn), a 34 ms Update
    overrun right after it, then a 30 ms compile; and one 19 ms frame
    followed by a 14 ms one (jitter) at 25 s."""
    cols = HEADER.split(",")
    lines = [HEADER if attributed else "frame,t_ms,dt_ms,state,occluded"]
    t = 0.0
    frame = 0
    spikes = {1200: 40.0, 1201: 34.0, 1203: 30.0, 1500: 19.0, 1501: 14.0}
    for i in range(1800):
        frame += 1
        dt = spikes.get(i, 16.667)
        t += dt
        row = {c: 0 for c in cols}
        row.update(frame=frame, t_ms=f"{t:.3f}", dt_ms=f"{dt:.3f}", state="playing", ticks=1)
        row.update(pre_ms=0.5, fixed_ms=2.0, update_ms=2.0, post_ms=1.0, extract_ms=0.5,
                   prepare_ms=1.0, acquire_ms=8.0, graph_ms=1.0, render_end_ms=0.1,
                   idle_ms=0.3, vsync_dt_ms=16.667, gpu_ms="", knights=4, entities=3000)
        for c in cols:
            if c.startswith("gpu_"):
                row[c] = ""
        if gpu and i % 8 == 0:
            # A GPU sample of an earlier frame: every 4th bare (total only),
            # every 10th full one heavy (world 6 ms, total 13 ms).
            sample = i // 8
            row.update(gpu_frame=frame - 3, gpu_full=int(sample % 4 != 0))
            if sample % 4 == 0:
                row["gpu_ms"] = 6.4
            else:
                heavy = sample % 10 == 1
                row.update(gpu_ms=13.0 if heavy else 7.0, gpu_world_ms=6.0 if heavy else 4.0,
                           gpu_effects_ms=1.5, gpu_ui_ms=0.4, gpu_post_ms=0.8)
        if i == 1199:
            row["knights_spawned"] = 1
        if i in (1200, 1203):
            row["graph_ms"] = 20.0
            row["pipelines_compiled"] = 2
        if i == 1201:
            row["update_ms"] = 12.0
        if attributed:
            lines.append(",".join(str(row[c]) for c in cols))
        else:
            lines.append(f"{frame},{t:.3f},{dt:.3f},playing,0")
    with open(os.path.join(folder, "frames.csv"), "w", encoding="utf-8") as f:
        f.write("\n".join(lines) + "\n")


def cause_lines(out):
    lines = out.splitlines()
    start = next(i for i, line in enumerate(lines) if line.startswith("Cause")) + 1
    rows = []
    for line in lines[start:]:
        if not line.strip():
            break
        rows.append(line)
    return rows


if __name__ == "__main__":
    unittest.main()
