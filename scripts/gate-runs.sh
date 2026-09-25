#!/bin/bash
# Runs the screen-dependent Milestone 1 gate scenarios back to back, full screen,
# on the release build, then prints the gate table. Leave the Mac alone (window in
# front, no other apps) while it runs.
#
#   scripts/gate-runs.sh          # G1, G3, G5/G4 captures, G6, G8 gallery (~6 min)
#   scripts/gate-runs.sh --g2     # also the 5-minute G2 perf run (run on battery!)
#   scripts/gate-runs.sh --only-g2
set -euo pipefail
cd "$(dirname "$0")/.."
. scripts/env.sh
cargo build --release --locked --quiet
bin="$CARGO_TARGET_DIR/pieced-gate"
cp "$CARGO_TARGET_DIR/release/pieced" "$bin"
stamp=$(date +%Y%m%d-%H%M%S)
runs=()
run() { # name, args...
  local name=$1; shift
  local out="evidence/gate-$stamp-$name"
  echo "== $name"
  timeout 900 "$bin" --evidence "$out" "$@" | grep -E "PIECED_(LAUNCH_MS|SCENARIO_DONE)" || true
  runs+=("$out")
}
if [[ "${1:-}" != "--only-g2" ]]; then
  for i in 1 2 3; do run "launch$i" --scenario smoke --seconds 3; done
  run latency --scenario latency
  run ttk --scenario ttk
  run hud --scenario hud_check
  run fx --scenario fx_check
  run gallery --scenario gallery
fi
if [[ "${1:-}" == "--g2" || "${1:-}" == "--only-g2" ]]; then
  run perf --scenario perf --seconds 300
fi
python3 scripts/evidence.py "${runs[@]}"
