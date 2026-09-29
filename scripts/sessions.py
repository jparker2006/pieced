#!/usr/bin/env python3
"""List Pieced play sessions from the always-on session log.

Usage:
  python3 scripts/sessions.py [--root DIR] [--s2] [--launches] [--limit N]
  python3 scripts/sessions.py [--root DIR] --spikes SESSION
  python3 scripts/sessions.py [--root DIR] --gpu SESSION
  python3 scripts/sessions.py [--root DIR] --latency SESSION

Reads userdata/sessions/<stamp>/session.json (stamps are UTC; times are shown
in local time). DIR defaults to $PIECED_ROOT/userdata/sessions, else the
repo's userdata/sessions.

  (default)    one line per session: when, commit, launch (ms, cold|warm),
               play and counted play, power and Low Power Mode, S2 verdict
  --s2         only sessions that qualified for S2 (PASS or FAIL), with the
               highest wave reached and the quit line (the verdict is on
               presented frames since M4; the line also shows the CPU-side
               measure). The first PASS closes M2's S2; the first PASS that
               reached wave >= 6 closes M4's A7 (and M3's W7)
  --launches   the launch series, oldest first, and the first run of
               3 warm launches in a row under 5 s (S3 / W8)
  --spikes S   the spike report for session S (a folder name, a path, or
               "latest"): for every counted frame over 25 ms, which part of
               the frame ran long (the bucket furthest above its normal-frame
               value), the events around it, the worst frames, and pacing
               jitter. Recomputed from frames.csv; sessions from before the
               attribution columns get a spike timeline only.
  --gpu S      GPU time per pass for session S (a folder name, a path, or
               "latest"): each pass's mean and p95 over the counted frames'
               GPU samples (1 frame in 8 by default) against its budget, the
               total against 12 ms, the share of timed frames over 12 ms, and
               the timing's own cost (full minus bare samples).
  --latency S  input-to-present latency for session S: for the counted
               frames that consumed a key or button press, and those that
               consumed trackpad look motion, the time from the OS input
               event to the present of the frame that used it (mean, median,
               p95, max) against M1's G3 target (median <= 33 ms). Excludes
               the GPU's work after present and the display's scanout.

A session whose session.json says "final": false ended without quitting
(a crash or a kill); its numbers are from the last periodic rewrite.

Standard library only.
"""

import argparse
import csv
import datetime
import json
import os
import sys

WARM_LAUNCH_MAX_MS = 5000.0
WARM_RUN = 3
# M4's A7 needs a qualifying S2 PASS in a session that reached this wave.
A7_MIN_WAVE = 6

# The S2 frame filter and the spike attribution (SpikeStats), both in
# src/session.rs; keep the two in step.
LAUNCH_EXCLUDE_MS = 10_000.0
REENTRY_EXCLUDE_MS = 1_000.0
SPIKE_MS = 25.0
BASELINE_ALPHA = 0.02
MIN_EXCESS_MS = 1.0
WORST_KEPT = 12
BUCKETS = [
    "pre", "fixed", "update", "post", "extract", "prepare", "acquire", "graph", "render_end", "idle",
]
BUCKET_HELP = {
    "pre": "First + PreUpdate: window/input events, state changes",
    "fixed": "fixed ticks: gameplay + physics",
    "update": "Update: gameplay presentation, HUD, audio queue",
    "post": "PostUpdate: transforms, visibility, UI layout, audio sinks",
    "extract": "render extract",
    "prepare": "render specialize/queue/prepare",
    "acquire": "waiting for the swapchain drawable (display or GPU behind)",
    "graph": "render graph: first-use pipeline compiles, encode, submit",
    "render_end": "render cleanup and frame limiter",
    "idle": "event loop and OS between frames",
    "unattributed": "no bucket ran long: time outside the markers",
}
EVENTS = [
    ("pipeline_compile", lambda r: r["pipelines_compiled"] > 0),
    ("knight_spawn", lambda r: r["knights_spawned"] > 0),
    ("orb_fired", lambda r: r["orbs_fired"] > 0),
    ("piece_placed", lambda r: r["placed"] > 0),
    ("piece_cracked", lambda r: r["cracked"] > 0),
    ("piece_broken", lambda r: r["broken"] > 0),
    ("player_shot", lambda r: r["shots"] > 0),
    ("damage", lambda r: r["damage"] > 0),
    ("sound_started", lambda r: r["voices_started"] > 0),
    ("two_fixed_ticks", lambda r: r["ticks"] >= 2),
]


# The M4 GPU budget (docs/M4-SPEC.md, D99): ms of GPU per frame at p95 on
# battery with Low Power Mode at wave-6 load. The one place these numbers live;
# the orchestrator may rebalance them (and logs each change).
GPU_BUDGET_MS = {
    "world": 4.5,
    "outlines": 1.5,
    "far": 2.0,
    "effects": 2.0,
    "ui": 0.5,
    "post": 1.0,
    "slack": 0.5,
}
GPU_TOTAL_BUDGET_MS = sum(GPU_BUDGET_MS.values())
GPU_PASSES = ["world", "outlines", "far", "effects", "ui", "post"]
GPU_PASS_HELP = {
    "world": "toon world, props, pieces, knights, viewmodel",
    "outlines": "the hull outlines draw inside the opaque pass: counted in world",
    "far": "far layer on its own camera (farres=half); otherwise counted in world",
    "effects": "halos, particles, spell bursts, transparents",
    "ui": "the HUD at native resolution",
    "post": "FXAA, sharpening, the 3D copy, the one window-size composite",
    "slack": "",
}
# A timed frame whose GPU total is at least this long can't make 60 Hz.
GPU_FRAME_MS = 16.0

# M1's G3: median input-to-frame latency at most two 60 Hz frames.
LATENCY_TARGET_MS = 33.0
LATENCY_KINDS = [
    ("press", "press_latency_ms", "key and button presses (exact event time)"),
    ("motion", "motion_latency_ms", "trackpad look (newest event of each frame's batch)"),
]


def default_root():
    base = os.environ.get("PIECED_ROOT") or os.path.dirname(
        os.path.dirname(os.path.abspath(__file__))
    )
    return os.path.join(base, "userdata", "sessions")


def load_sessions(root):
    sessions = []
    if not os.path.isdir(root):
        return sessions
    for name in sorted(os.listdir(root)):
        path = os.path.join(root, name, "session.json")
        if not os.path.isfile(path):
            continue
        try:
            with open(path, encoding="utf-8") as f:
                doc = json.load(f)
        except (OSError, ValueError) as e:
            print(f"warning: {path}: {e}", file=sys.stderr)
            continue
        doc["_name"] = name
        sessions.append(doc)
    return sessions


def when(doc):
    started = doc.get("started_unix_s")
    if not started:
        return doc["_name"]
    return datetime.datetime.fromtimestamp(started).strftime("%Y-%m-%d %H:%M")


def launch_text(doc):
    launch = doc.get("launch") or {}
    ms = launch.get("ms")
    kind = launch.get("kind") or "?"
    return f"{ms:7.0f} ms {kind}" if ms is not None else f"      - ms {kind}"


def power_text(doc):
    samples = doc.get("power_samples") or []
    if not samples:
        return "power ?"
    n = len(samples)
    batt = sum(1 for s in samples if s.get("source") == "Battery Power")
    lpm = sum(1 for s in samples if s.get("low_power_mode") is True)
    return f"batt {batt}/{n} LPM {lpm}/{n}"


def verdict(doc):
    return (doc.get("s2") or {}).get("verdict", "?")


def fmt_s(seconds):
    seconds = seconds or 0.0
    return f"{int(seconds // 60):3d}m{int(seconds % 60):02d}s"


def list_sessions(sessions):
    print(
        f"{'when':16}  {'commit':18}  {'launch':16}  {'play':>7}  {'counted':>7}  "
        f"{'power':20}  {'occl ms':>7}  S2"
    )
    for doc in sessions:
        crashed = "" if doc.get("final") else "  (no clean quit)"
        why = ",".join((doc.get("s2") or {}).get("reasons") or [])
        print(
            f"{when(doc):16}  {doc.get('commit', '?'):18}  {launch_text(doc):16}  "
            f"{fmt_s(doc.get('play_s'))}  {fmt_s(doc.get('counted_play_s'))}  "
            f"{power_text(doc):20}  {doc.get('occluded_counted_ms', 0):7.0f}  "
            f"{verdict(doc)} {why}{crashed}"
        )


def max_wave(doc):
    """The highest Waves wave the session reached (0 if unknown or none)."""
    return ((doc.get("waves") or {}).get("max_wave")) or 0


def list_s2(sessions):
    qualified = [d for d in sessions if verdict(d) in ("PASS", "FAIL")]
    if not qualified:
        print("No qualifying session yet (>= 5 min counted play, battery, Low Power Mode on,")
        print("window visible, release build, Battery preset).")
        return
    print(f"{'when':16}  {'session':15}  {'commit':18}  {'wave':>4}  quit line")
    for doc in qualified:
        wave = max_wave(doc)
        print(
            f"{when(doc):16}  {doc['_name']:15}  {doc.get('commit', '?'):18}  "
            f"{wave if wave else '-':>4}  {doc.get('s2_line', '')}"
        )
    passes = [d for d in qualified if verdict(d) == "PASS"]
    if passes:
        first = passes[0]
        print(f"\nFirst PASS: {first['_name']} on {first.get('commit', '?')}")
    a7 = [d for d in passes if max_wave(d) >= A7_MIN_WAVE]
    if a7:
        first = a7[0]
        print(
            f"First A7 session (PASS at wave >= {A7_MIN_WAVE}): {first['_name']} "
            f"on {first.get('commit', '?')}, wave {max_wave(first)}"
        )
    else:
        print(f"No A7 session yet: it needs a PASS that reached wave >= {A7_MIN_WAVE}.")


def list_launches(sessions):
    series = []
    for doc in sessions:
        launch = doc.get("launch") or {}
        if launch.get("ms") is None:
            continue
        series.append((doc, launch["ms"], launch.get("kind")))
    if not series:
        print("No launches recorded yet.")
        return
    for doc, ms, kind in series:
        phases = " ".join(
            f"{p['phase']}={p['ms']:.0f}" for p in (doc["launch"].get("boot_phases") or [])
        )
        print(f"{when(doc):16}  {doc.get('commit', '?'):18}  {ms:7.0f} ms {kind:4}  {phases}")
    run = []
    for doc, ms, kind in series:
        if kind == "warm" and ms < WARM_LAUNCH_MAX_MS:
            run.append((doc, ms))
            if len(run) == WARM_RUN:
                names = ", ".join(f"{d['_name']} ({m:.0f} ms)" for d, m in run)
                print(f"\n{WARM_RUN} warm launches in a row < {WARM_LAUNCH_MAX_MS:.0f} ms: {names}")
                return
        else:
            run = []
    print(f"\nNo run of {WARM_RUN} warm launches in a row < {WARM_LAUNCH_MAX_MS:.0f} ms yet.")


def find_session(root, which):
    """A session folder from a name, a path, or "latest"."""
    if which == "latest":
        names = sorted(
            n
            for n in (os.listdir(root) if os.path.isdir(root) else [])
            if os.path.isfile(os.path.join(root, n, "frames.csv"))
        )
        return os.path.join(root, names[-1]) if names else None
    if os.path.isdir(which):
        return which
    path = os.path.join(root, which)
    return path if os.path.isdir(path) else None


def read_frames(folder):
    """frames.csv rows as dicts of numbers (state stays a string, empty is None)."""
    rows = []
    with open(os.path.join(folder, "frames.csv"), encoding="utf-8", newline="") as f:
        for raw in csv.DictReader(f):
            row = {}
            for k, v in raw.items():
                if k == "state":
                    row[k] = v
                elif v in ("", None):
                    row[k] = None
                else:
                    try:
                        row[k] = float(v)
                    except ValueError:
                        row[k] = v
            rows.append(row)
    return rows


def counted_flags(rows):
    """Which rows count for S2 (the same filter as src/session.rs)."""
    out = []
    since = None
    for r in rows:
        if r["state"] != "playing":
            since = None
            out.append(False)
            continue
        if since is None:
            since = r["t_ms"]
        out.append(
            r["dt_ms"] > 0
            and r["t_ms"] >= LAUNCH_EXCLUDE_MS
            and r["t_ms"] - since >= REENTRY_EXCLUDE_MS
        )
    return out


def spike_report(rows):
    """The same analysis as SpikeStats in src/session.rs."""
    flags = counted_flags(rows)
    rep = {
        "counted": 0,
        "spikes": 0,
        "runs": 0,
        "by_cause": {},
        "worst": [],
        "event_frames": {n: 0 for n, _ in EVENTS},
        "event_spikes": {n: 0 for n, _ in EVENTS},
        "spike_sum": {b: 0.0 for b in BUCKETS},
        "normal_sum": {b: 0.0 for b in BUCKETS},
        "normal": 0,
        "late_then_early": 0,
        "vsync_frames": 0,
        "vsync_under_18": 0,
        "dt_under_18": 0,
    }
    baseline = None
    previous = None
    last_was_spike = False
    for r, counted in zip(rows, flags):
        prev = previous
        previous = r if r["state"] == "playing" else None
        if not counted:
            last_was_spike = False
            continue
        rep["counted"] += 1
        if r["dt_ms"] < 18.0:
            rep["dt_under_18"] += 1
        if r["vsync_dt_ms"] and r["vsync_dt_ms"] > 0:
            rep["vsync_frames"] += 1
            if r["vsync_dt_ms"] < 18.0:
                rep["vsync_under_18"] += 1
        if prev is not None and 18.0 < prev["dt_ms"] <= SPIKE_MS and r["dt_ms"] < 15.5:
            rep["late_then_early"] += 1
        events = [n for n, test in EVENTS if test(r) or (prev is not None and test(prev))]
        for n in events:
            rep["event_frames"][n] += 1
        values = [r[b + "_ms"] for b in BUCKETS]
        if r["dt_ms"] <= SPIKE_MS:
            last_was_spike = False
            if r["dt_ms"] < 18.0:
                rep["normal"] += 1
                for b, v in zip(BUCKETS, values):
                    rep["normal_sum"][b] += v
                if baseline is None:
                    baseline = list(values)
                else:
                    baseline = [b + (v - b) * BASELINE_ALPHA for b, v in zip(baseline, values)]
            continue
        rep["spikes"] += 1
        if not last_was_spike:
            rep["runs"] += 1
        last_was_spike = True
        for b, v in zip(BUCKETS, values):
            rep["spike_sum"][b] += v
        for n in events:
            rep["event_spikes"][n] += 1
        base = baseline or [0.0] * len(BUCKETS)
        excess = [v - b for v, b in zip(values, base)]
        # The first bucket wins a tie, as in the Rust fold.
        i = max(range(len(excess)), key=lambda k: (excess[k], -k))
        cause = BUCKETS[i] if excess[i] >= MIN_EXCESS_MS else "unattributed"
        rep["by_cause"][cause] = rep["by_cause"].get(cause, 0) + 1
        rep["worst"].append((r, cause, max(excess[i], 0.0), events))
    rep["worst"].sort(key=lambda w: -w[0]["dt_ms"])
    rep["worst"] = rep["worst"][:WORST_KEPT]
    return rep


def spike_timeline(rows, bin_s=10.0):
    """Counted spikes per bin of session time (works on any frames.csv)."""
    flags = counted_flags(rows)
    bins = {}
    for r, counted in zip(rows, flags):
        if counted and r["dt_ms"] > SPIKE_MS:
            k = int(r["t_ms"] / 1000.0 // bin_s)
            bins[k] = bins.get(k, 0) + 1
    return bins, sum(flags)


def print_spikes(folder):
    rows = read_frames(folder)
    name = os.path.basename(os.path.normpath(folder))
    doc = {}
    json_path = os.path.join(folder, "session.json")
    if os.path.isfile(json_path):
        with open(json_path, encoding="utf-8") as f:
            doc = json.load(f)
    print(f"Session {name}  commit {doc.get('commit', '?')}  {doc.get('graphics', '')}")
    if doc.get("s2_line"):
        print(doc["s2_line"])
    if not rows or "acquire_ms" not in rows[0]:
        bins, counted = spike_timeline(rows)
        total = sum(bins.values())
        print(
            f"\nThis session predates the attribution columns: {total} spikes > "
            f"{SPIKE_MS:.0f} ms in {counted} counted frames. Spikes per 10 s of session time:"
        )
        for k in sorted(bins):
            print(f"  {k * 10:5d}-{k * 10 + 10:<5d} s  {'#' * min(bins[k], 60)} {bins[k]}")
        return
    rep = spike_report(rows)
    n = rep["spikes"]
    print(f"\n{rep['counted']} counted frames, {n} spikes > {SPIKE_MS:.0f} ms in {rep['runs']} runs")
    if rep["counted"]:
        vs = rep["vsync_under_18"] * 100.0 / rep["vsync_frames"] if rep["vsync_frames"] else 0.0
        dt = rep["dt_under_18"] * 100.0 / rep["counted"]
        print(
            f"< 18 ms: {dt:.2f}% by dt (S2's measure), {vs:.2f}% by the drawable cadence; "
            f"{rep['late_then_early']} late-then-early pairs (pacing jitter)"
        )
    if n == 0:
        return
    print("\nCause (the bucket furthest above its normal-frame value):")
    for cause, k in sorted(rep["by_cause"].items(), key=lambda kv: -kv[1]):
        print(f"  {cause:13} {k:5d}  {k * 100.0 / n:5.1f}%  {BUCKET_HELP.get(cause, '')}")
    print("\nMean ms per bucket:    " + " ".join(f"{b:>10}" for b in BUCKETS))
    for label, sums, count in (
        ("on spikes", rep["spike_sum"], n),
        ("on normal frames", rep["normal_sum"], rep["normal"]),
    ):
        vals = " ".join(f"{(sums[b] / count if count else 0.0):10.2f}" for b in BUCKETS)
        print(f"  {label:20} {vals}")
    print("\nEvents on the spike frame or the one before (lift = share of spikes / share of frames):")
    for ev, _ in EVENTS:
        k = rep["event_spikes"][ev]
        share_s = k / n
        share_f = rep["event_frames"][ev] / rep["counted"] if rep["counted"] else 0.0
        lift = share_s / share_f if share_f > 0 else 0.0
        print(
            f"  {ev:17} {k:5d}  {share_s * 100:5.1f}% of spikes  "
            f"{share_f * 100:5.1f}% of frames  lift {lift:4.1f}"
        )
    print(f"\nWorst {len(rep['worst'])}:")
    for r, cause, excess, events in rep["worst"]:
        parts = " ".join(f"{b}={r[b + '_ms']:.1f}" for b in BUCKETS if r[b + "_ms"] >= 0.5)
        live = (
            f"knights={r['knights']:.0f} orbs={r['orbs']:.0f} particles={r['particles']:.0f} "
            f"debris={r['debris']:.0f} entities={r['entities']:.0f}"
        )
        gpu = f" gpu={r['gpu_ms']:.1f}" if r.get("gpu_ms") is not None else ""
        print(
            f"  t={r['t_ms'] / 1000.0:7.2f}s dt={r['dt_ms']:5.1f} {cause:12} +{excess:4.1f}  "
            f"{parts}{gpu}  [{','.join(events) or '-'}]  {live}"
        )


def mean_p95(values):
    """(mean, nearest-rank p95) of a list, or (None, None)."""
    if not values:
        return None, None
    ordered = sorted(values)
    rank = max(1, min(len(ordered), -(-95 * len(ordered) // 100)))
    return sum(ordered) / len(ordered), ordered[rank - 1]


def gpu_report(rows):
    """Per-pass GPU samples of the counted frames (the S2 filter)."""
    flags = counted_flags(rows)
    rep = {"passes": {p: [] for p in GPU_PASSES}, "full": [], "bare": [], "acquire": []}
    for r, counted in zip(rows, flags):
        if not counted:
            continue
        if r.get("acquire_ms") is not None:
            rep["acquire"].append(r["acquire_ms"])
        if r.get("gpu_frame") is None or r.get("gpu_ms") is None:
            continue
        if r.get("gpu_full") == 1:
            rep["full"].append(r["gpu_ms"])
            for p in GPU_PASSES:
                v = r.get(f"gpu_{p}_ms")
                if v is not None:
                    rep["passes"][p].append(v)
        else:
            rep["bare"].append(r["gpu_ms"])
    return rep


def fmt_ms(v):
    return f"{v:6.2f}" if v is not None else "     -"


def print_gpu(folder):
    rows = read_frames(folder)
    name = os.path.basename(os.path.normpath(folder))
    doc = {}
    json_path = os.path.join(folder, "session.json")
    if os.path.isfile(json_path):
        with open(json_path, encoding="utf-8") as f:
            doc = json.load(f)
    print(f"Session {name}  commit {doc.get('commit', '?')}  {doc.get('graphics', '')}")
    print(power_text(doc) if doc else "power ?")
    if not rows or "gpu_world_ms" not in rows[0]:
        print("\nThis session predates per-pass GPU timing (M4 chunk 0).")
        return
    rep = gpu_report(rows)
    timed = rep["full"] + rep["bare"]
    if not timed:
        print("\nNo GPU samples in the counted frames (timing off, or no counted play).")
        return
    print(
        f"\n{len(rep['full'])} full and {len(rep['bare'])} bare GPU samples in the counted "
        f"frames. Budget: {GPU_TOTAL_BUDGET_MS:.1f} ms at p95."
    )
    print(f"\n  {'pass':9} {'mean':>6} {'p95':>6} {'budget':>6}  {'n':>6}  status")
    for p in GPU_PASSES + ["slack"]:
        budget = GPU_BUDGET_MS[p]
        values = rep["passes"].get(p, [])
        mean, p95 = mean_p95(values)
        if p == "slack":
            status = ""
        elif p95 is None:
            status = f"n/a  {GPU_PASS_HELP[p]}" if p in ("outlines", "far") else "n/a"
        else:
            status = ("ok" if p95 <= budget else "OVER") + f"  {GPU_PASS_HELP[p]}"
        print(f"  {p:9} {fmt_ms(mean)} {fmt_ms(p95)} {budget:6.2f}  {len(values):6d}  {status}")
    mean, p95 = mean_p95(rep["full"])
    verdict = "ok" if p95 is not None and p95 <= GPU_TOTAL_BUDGET_MS else "OVER"
    print(
        f"  {'total':9} {fmt_ms(mean)} {fmt_ms(p95)} {GPU_TOTAL_BUDGET_MS:6.2f}  "
        f"{len(rep['full']):6d}  {verdict if p95 is not None else 'n/a'}"
    )
    over = sum(1 for v in timed if v > GPU_TOTAL_BUDGET_MS)
    late = sum(1 for v in timed if v >= GPU_FRAME_MS)
    print(
        f"\nGPU total > {GPU_TOTAL_BUDGET_MS:.0f} ms on {over * 100.0 / len(timed):.1f}% of timed "
        f"frames; >= {GPU_FRAME_MS:.0f} ms (can't make 60 Hz) on {late * 100.0 / len(timed):.1f}%."
    )
    if len(rep["full"]) >= 10 and len(rep["bare"]) >= 10:
        cost = sum(rep["full"]) / len(rep["full"]) - sum(rep["bare"]) / len(rep["bare"])
        print(f"Timing cost (mean full - mean bare total): {cost:.2f} ms per timed frame.")
    acquire, _ = mean_p95(rep["acquire"])
    if acquire is not None and p95 is not None:
        hint = (
            "the GPU being behind"
            if p95 >= GPU_FRAME_MS
            else "display pacing, not the GPU (its p95 leaves room)"
        )
        print(f"Mean acquire wait {acquire:.2f} ms; with this GPU p95 that points at {hint}.")


def latency_stats(values):
    """(n, mean, median, nearest-rank p95, max), or None without samples."""
    if not values:
        return None
    ordered = sorted(values)
    n = len(ordered)
    median = ordered[n // 2] if n % 2 else (ordered[n // 2 - 1] + ordered[n // 2]) / 2
    rank = max(1, min(n, -(-95 * n // 100)))
    return n, sum(ordered) / n, median, ordered[rank - 1], ordered[-1]


def latency_report(rows):
    """Each kind's input-to-present samples in the counted frames."""
    flags = counted_flags(rows)
    rep = {kind: [] for kind, _, _ in LATENCY_KINDS}
    for r, counted in zip(rows, flags):
        if not counted:
            continue
        for kind, column, _ in LATENCY_KINDS:
            v = r.get(column)
            if v is not None and v > 0:
                rep[kind].append(v)
    return rep


def print_latency(folder):
    rows = read_frames(folder)
    name = os.path.basename(os.path.normpath(folder))
    doc = {}
    json_path = os.path.join(folder, "session.json")
    if os.path.isfile(json_path):
        with open(json_path, encoding="utf-8") as f:
            doc = json.load(f)
    print(f"Session {name}  commit {doc.get('commit', '?')}  {doc.get('graphics', '')}")
    pacing = (doc.get("input_latency") or {}).get("pacing")
    if pacing:
        print(f"Pacing: {pacing}")
    if not rows or "press_latency_ms" not in rows[0]:
        print("\nThis session predates the input-to-present latency columns.")
        return
    rep = latency_report(rows)
    print(
        f"\nInput to present (ms) over the counted frames; G3 target: median <= "
        f"{LATENCY_TARGET_MS:.0f} ms (GPU work after present and scanout excluded)."
    )
    print(f"\n  {'kind':7} {'n':>6} {'mean':>6} {'median':>6} {'p95':>6} {'max':>6}  status")
    for kind, _, help_text in LATENCY_KINDS:
        stats = latency_stats(rep[kind])
        if stats is None:
            print(f"  {kind:7} {0:6d} {'-':>6} {'-':>6} {'-':>6} {'-':>6}  n/a  {help_text}")
            continue
        n, mean, median, p95, top = stats
        status = "ok" if median <= LATENCY_TARGET_MS else "OVER"
        print(
            f"  {kind:7} {n:6d} {mean:6.2f} {median:6.2f} {p95:6.2f} {top:6.2f}  "
            f"{status}  {help_text}"
        )


def main():
    parser = argparse.ArgumentParser(description=__doc__.split("\n")[0])
    parser.add_argument("--root", default=default_root(), help="the sessions folder")
    parser.add_argument("--s2", action="store_true", help="only qualifying sessions")
    parser.add_argument("--launches", action="store_true", help="the launch series")
    parser.add_argument("--limit", type=int, default=0, help="only the newest N sessions")
    parser.add_argument("--spikes", metavar="SESSION", help="the spike report for one session")
    parser.add_argument("--gpu", metavar="SESSION", help="GPU time per pass for one session")
    parser.add_argument(
        "--latency", metavar="SESSION", help="input-to-present latency for one session"
    )
    args = parser.parse_args()
    for which, report in (
        (args.spikes, print_spikes),
        (args.gpu, print_gpu),
        (args.latency, print_latency),
    ):
        if not which:
            continue
        folder = find_session(args.root, which)
        if folder is None or not os.path.isfile(os.path.join(folder, "frames.csv")):
            print(f"No session '{which}' with frames.csv in {args.root}")
            return
        report(folder)
        return
    sessions = load_sessions(args.root)
    if args.limit > 0:
        sessions = sessions[-args.limit :]
    if not sessions:
        print(f"No sessions in {args.root}")
        return
    if args.s2:
        list_s2(sessions)
    elif args.launches:
        list_launches(sessions)
    else:
        list_sessions(sessions)


if __name__ == "__main__":
    main()
