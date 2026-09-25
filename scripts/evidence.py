#!/usr/bin/env python3
"""Print a compact gate table from Pieced evidence folders.

Usage: python3 scripts/evidence.py EVIDENCE_DIR [EVIDENCE_DIR ...]

Each argument is a scenario run folder (containing summary.json) or a folder
of run folders. Runs are listed in the order given (folders expanded in name
order). Gate thresholds are re-evaluated here from the raw numbers, so the
table does not depend on the verdicts stored in the runs.

Gates (docs/GOAL.md):
  G1  launch to controllable < 5.0 s, in 3 consecutive launches
  G2  perf, after warm-up: mean frame interval 16.4-17.0 ms, 0 frames > 25 ms,
      >= 99% of frames < 18 ms; counts only on battery with Low Power Mode on,
      in a release build over a 300 s timed window
  G3  latency: median input-to-frame <= 33 ms (display scanout excluded)
  G6  ttk: rifle 1.0-2.0 s at 15 m on every kill (>= 5 kills), no pump kill
      from full, a wall soaks >= 1.0 s

Standard library only.
"""

import json
import math
import os
import sys

G1_LAUNCH_MAX_MS = 5000.0
G1_CONSECUTIVE = 3
G2_MEAN_MIN_MS, G2_MEAN_MAX_MS = 16.4, 17.0
G2_MAX_OVER_25 = 0
G2_MIN_PCT_UNDER_18 = 99.0
G2_GATE_SECONDS = 300.0
G3_MEDIAN_MAX_MS = 33.0
G3_MIN_SAMPLES = 100
G6_TTK_MIN_S, G6_TTK_MAX_S = 1.0, 2.0
G6_MIN_KILLS = 5
G6_WALL_SOAK_MIN_S = 1.0


def find_runs(paths):
    runs = []
    for path in paths:
        if os.path.isfile(os.path.join(path, "summary.json")):
            runs.append(path)
            continue
        found = 0
        if os.path.isdir(path):
            for name in sorted(os.listdir(path)):
                sub = os.path.join(path, name)
                if os.path.isfile(os.path.join(sub, "summary.json")):
                    runs.append(sub)
                    found += 1
        if not found:
            print(f"warning: no summary.json in {path}", file=sys.stderr)
    return runs


def load(path):
    with open(os.path.join(path, "summary.json"), encoding="utf-8") as f:
        return json.load(f)


def num(value, digits=1, unit=""):
    if value is None or (isinstance(value, float) and not math.isfinite(value)):
        return "-"
    return f"{value:.{digits}f}{unit}"


def verdict(ok):
    return "PASS" if ok else "FAIL"


def power(summary):
    start = summary.get("power_start") or {}
    end = summary.get("power_end") or {}
    src = (start.get("source") or "?").replace(" Power", "")
    src_end = (end.get("source") or "?").replace(" Power", "")
    source = src if src == src_end else f"{src} -> {src_end}"
    lpm_values = {start.get("low_power_mode"), end.get("low_power_mode")}
    if lpm_values == {True}:
        lpm = "on"
    elif lpm_values == {False}:
        lpm = "off"
    else:
        lpm = "mixed/unknown"
    battery = start.get("battery", "")
    pct = battery.split("\t")[-1].split(";")[0].strip() if battery else ""
    return source, lpm, pct


def on_battery_lpm(summary):
    for key in ("power_start", "power_end"):
        p = summary.get(key) or {}
        if "Battery" not in (p.get("source") or "") or p.get("low_power_mode") is not True:
            return False
    return True


def conditions(summary):
    c = (summary.get("scenario") or {}).get("run_conditions")
    if not c:
        return "-"
    start = (c.get("load_average_start") or [None])[0]
    end = (c.get("load_average_end") or [None])[0]
    return (
        f"occluded {num(c.get('window_occluded_ms'), 0)} ms, "
        f"load {num(start)} -> {num(end)}"
    )


def g2(summary):
    scn = summary.get("scenario") or {}
    frames = scn.get("frames") or summary.get("frames") or {}
    n = frames.get("frames", 0)
    mean = frames.get("mean_ms")
    over25 = frames.get("over_25_ms")
    under18 = frames.get("pct_under_18_ms")
    ok = (
        n > 0
        and mean is not None
        and G2_MEAN_MIN_MS <= mean <= G2_MEAN_MAX_MS
        and over25 is not None
        and over25 <= G2_MAX_OVER_25
        and under18 is not None
        and under18 >= G2_MIN_PCT_UNDER_18
    )
    seconds = scn.get("timed_seconds")
    occluded = ((scn.get("run_conditions") or {}).get("window_occluded_ms") or 0) > 0
    eligible = (
        not occluded
        and on_battery_lpm(summary)
        and summary.get("build") == "release"
        and seconds is not None
        and seconds >= G2_GATE_SECONDS
    )
    load = scn.get("load_timed_window") or {}
    measurement = (
        f"{n} frames over {num(seconds, 0)} s: mean {num(mean, 2)} ms, "
        f"p99 {num(frames.get('p99_ms'), 2)}, max {num(frames.get('max_ms'), 1)} ms, "
        f">25 ms {over25}, <18 ms {num(under18, 2)}%; load: "
        f"{load.get('pieces_placed', '-')} placed, {load.get('pieces_destroyed', '-')} broken, "
        f"{load.get('shots', '-')} shots, {load.get('hits', '-')} hits, "
        f"{load.get('eliminations', '-')} kills"
    )
    result = verdict(ok) + ("" if eligible else " (not gate run)")
    return measurement, "mean 16.4-17.0, 0 >25 ms, >=99% <18 ms", result


def g3(summary):
    scn = summary.get("scenario") or {}
    arrival = scn.get("arrival_to_render_submit_ms") or {}
    apply = scn.get("apply_to_render_submit_ms") or {}
    upper = (scn.get("estimated_input_to_photon_upper_bound_ms") or {}).get("from_arrival") or {}
    readback = (scn.get("gpu_readback") or {})
    rb = readback.get("input_to_readback_ms") or {}

    def med(stats):
        return stats.get("median_ms") if stats.get("samples") else None

    n = arrival.get("samples", 0)
    median = med(arrival)
    covered = ((scn.get("run_conditions") or {}).get("window_occluded_ms") or 0) > 0
    ok = n >= G3_MIN_SAMPLES and median is not None and median <= G3_MEDIAN_MAX_MS
    measurement = (
        f"n={n}; arrival->submit median {num(median)} / p95 {num(arrival.get('p95_ms'))} / "
        f"max {num(arrival.get('max_ms'))} ms; apply->submit median {num(med(apply))} ms; "
        f"est. photon upper bound median {num(med(upper))} ms; GPU readback median "
        f"{num(med(rb))} ms ({readback.get('trials_with_visible_change', '-')}/"
        f"{readback.get('trials_with_window_visible', readback.get('trials', '-'))} seen, "
        f"{readback.get('change_visible_in_the_input_frame', '-')} "
        f"in the input frame); vsync {scn.get('vsync')}, cap {scn.get('frame_cap')}"
    )
    measurement += f"; window covered {num((scn.get('run_conditions') or {}).get('window_occluded_ms'), 0)} ms"
    note = " (window covered: not valid timing)" if covered else ""
    return measurement, "median <= 33 ms, n >= 100", verdict(ok) + note


def g6(summary):
    scn = summary.get("scenario") or {}
    kills = scn.get("rifle_kills") or []
    ttks = []
    for k in kills:
        t = k.get("ttk_first_damage_to_kill_s")
        ttks.append(t if t is not None else math.inf)
    pumps = scn.get("pump_shots") or []
    counted = [p for p in pumps if p.get("start_total_health", 0) >= 200 - 1e-3 and p.get("damage", 0) > 0]
    pump_kill = any(p.get("killed") for p in counted)
    soak = (scn.get("wall_soak") or {}).get("first_hit_to_break_s")
    ok = (
        len(ttks) >= G6_MIN_KILLS
        and all(G6_TTK_MIN_S <= t <= G6_TTK_MAX_S for t in ttks)
        and len(counted) >= 1
        and not pump_kill
        and soak is not None
        and soak >= G6_WALL_SOAK_MIN_S
    )
    finite = [t for t in ttks if math.isfinite(t)]
    ttk_text = f"{num(min(finite), 3)}-{num(max(finite), 3)} s" if finite else "-"
    max_pump = max((p.get("damage", 0) for p in counted), default=None)
    measurement = (
        f"rifle TTK {ttk_text} over {len(ttks)} kills; pump max {num(max_pump, 1)} dmg in "
        f"{len(counted)} shots, {'a kill' if pump_kill else 'no kill'}; wall soak {num(soak, 3)} s"
    )
    return measurement, "TTK 1.0-2.0 s x>=5, pump never kills, wall >= 1.0 s", verdict(ok)


GATES = {"perf": ("G2", g2), "latency": ("G3", g3), "ttk": ("G6", g6)}


def main(argv):
    if len(argv) < 2:
        print(__doc__.strip(), file=sys.stderr)
        return 2
    runs = find_runs(argv[1:])
    if not runs:
        print("no runs found", file=sys.stderr)
        return 1
    summaries = [(os.path.basename(os.path.normpath(r)), load(r)) for r in runs]

    print("| Run | Scenario | Commit | Build | Power | Low Power Mode | Battery | Graphics | Conditions |")
    print("|---|---|---|---|---|---|---|---|---|")
    for name, s in summaries:
        source, lpm, pct = power(s)
        print(
            f"| {name} | {s.get('scenario_name', '?')} | {s.get('commit', '?')} | "
            f"{s.get('build', '?')} | {source} | {lpm} | {pct} | {s.get('graphics', '?')} | "
            f"{conditions(s)} |"
        )
    print()
    print("| Gate | Run | Measurement | Threshold | Result |")
    print("|---|---|---|---|---|")
    launches = []
    for name, s in summaries:
        launch = s.get("launch_to_controllable_ms")
        if launch is not None:
            launches.append(launch)
            ok = launch < G1_LAUNCH_MAX_MS and s.get("build") == "release"
            print(f"| G1 | {name} | launch {num(launch, 0)} ms ({s.get('build')}) | < 5000 ms | {verdict(ok)} |")
    for name, s in summaries:
        gate = GATES.get(s.get("scenario_name"))
        if gate:
            measurement, threshold, result = gate[1](s)
            print(f"| {gate[0]} | {name} | {measurement} | {threshold} | {result} |")
    if launches:
        last = launches[-G1_CONSECUTIVE:]
        if len(last) < G1_CONSECUTIVE:
            result = f"UNVERIFIED (needs {G1_CONSECUTIVE} consecutive launches, {len(last)} listed)"
        else:
            result = verdict(all(l < G1_LAUNCH_MAX_MS for l in last))
        print()
        print(
            f"G1 over the last {G1_CONSECUTIVE} launches listed "
            f"({', '.join(num(l, 0) for l in last)} ms): {result}"
        )
    return 0


if __name__ == "__main__":
    sys.exit(main(sys.argv))
