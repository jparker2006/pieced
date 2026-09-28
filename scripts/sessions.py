#!/usr/bin/env python3
"""List Pieced play sessions from the always-on session log.

Usage:
  python3 scripts/sessions.py [--root DIR] [--s2] [--launches] [--limit N]
  python3 scripts/sessions.py [--root DIR] --spikes SESSION

Reads userdata/sessions/<stamp>/session.json (stamps are UTC; times are shown
in local time). DIR defaults to $PIECED_ROOT/userdata/sessions, else the
repo's userdata/sessions.

  (default)    one line per session: when, commit, launch (ms, cold|warm),
               play and counted play, power and Low Power Mode, S2 verdict
  --s2         only sessions that qualified for S2 (PASS or FAIL), with the
               quit line; the first PASS closes M2's S2
  --launches   the launch series, oldest first, and the first run of
               3 warm launches in a row under 5 s (S3 / W8)
  --spikes S   the spike report for session S (a folder name, a path, or
               "latest"): for every counted frame over 25 ms, which part of
               the frame ran long (the bucket furthest above its normal-frame
               value), the events around it, the worst frames, and pacing
               jitter. Recomputed from frames.csv; sessions from before the
               attribution columns get a spike timeline only.

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


def list_s2(sessions):
    qualified = [d for d in sessions if verdict(d) in ("PASS", "FAIL")]
    if not qualified:
        print("No qualifying session yet (>= 5 min counted play, battery, Low Power Mode on,")
        print("window visible, release build, Battery preset).")
        return
    for doc in qualified:
        print(f"{when(doc)}  {doc['_name']}  {doc.get('commit', '?')}  {doc.get('s2_line', '')}")
    passes = [d for d in qualified if verdict(d) == "PASS"]
    if passes:
        first = passes[0]
        print(f"\nFirst PASS: {first['_name']} on {first.get('commit', '?')}")


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


def main():
    parser = argparse.ArgumentParser(description=__doc__.split("\n")[0])
    parser.add_argument("--root", default=default_root(), help="the sessions folder")
    parser.add_argument("--s2", action="store_true", help="only qualifying sessions")
    parser.add_argument("--launches", action="store_true", help="the launch series")
    parser.add_argument("--limit", type=int, default=0, help="only the newest N sessions")
    parser.add_argument("--spikes", metavar="SESSION", help="the spike report for one session")
    args = parser.parse_args()
    if args.spikes:
        folder = find_session(args.root, args.spikes)
        if folder is None or not os.path.isfile(os.path.join(folder, "frames.csv")):
            print(f"No session '{args.spikes}' with frames.csv in {args.root}")
            return
        print_spikes(folder)
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
