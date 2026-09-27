#!/usr/bin/env python3
"""List Pieced play sessions from the always-on session log.

Usage:
  python3 scripts/sessions.py [--root DIR] [--s2] [--launches] [--limit N]

Reads userdata/sessions/<stamp>/session.json (stamps are UTC; times are shown
in local time). DIR defaults to $PIECED_ROOT/userdata/sessions, else the
repo's userdata/sessions.

  (default)    one line per session: when, commit, launch (ms, cold|warm),
               play and counted play, power and Low Power Mode, S2 verdict
  --s2         only sessions that qualified for S2 (PASS or FAIL), with the
               quit line; the first PASS closes M2's S2
  --launches   the launch series, oldest first, and the first run of
               3 warm launches in a row under 5 s (S3 / W8)

A session whose session.json says "final": false ended without quitting
(a crash or a kill); its numbers are from the last periodic rewrite.

Standard library only.
"""

import argparse
import datetime
import json
import os
import sys

WARM_LAUNCH_MAX_MS = 5000.0
WARM_RUN = 3


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


def main():
    parser = argparse.ArgumentParser(description=__doc__.split("\n")[0])
    parser.add_argument("--root", default=default_root(), help="the sessions folder")
    parser.add_argument("--s2", action="store_true", help="only qualifying sessions")
    parser.add_argument("--launches", action="store_true", help="the launch series")
    parser.add_argument("--limit", type=int, default=0, help="only the newest N sessions")
    args = parser.parse_args()
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
