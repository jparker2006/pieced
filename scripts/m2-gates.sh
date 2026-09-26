#!/bin/bash
# Milestone 2 native gate runs, full screen, release build. Needs a "go" window:
# Jake away from the Mac, window in front, nothing covering it. Runs muted.
#
#   scripts/m2-gates.sh            # S3 launches, S7 latency + ttk, S6 fx_check,
#                                  # S1 gallery (+ board page), S4 sky_check (~5 min)
#   scripts/m2-gates.sh --perf     # also the 5-minute S2 perf run (on battery!)
#   scripts/m2-gates.sh --only-perf
#
# Evidence lands in evidence/m2-<stamp>-<run>/ (git-ignored); the gate table and
# the board page path are printed at the end.
set -euo pipefail
cd "$(dirname "$0")/.."
. scripts/env.sh
PIECED_FULL_SPEED=1 cargo build --release --locked --quiet
bin="$CARGO_TARGET_DIR/pieced-m2-gate"
cp "$CARGO_TARGET_DIR/release/pieced" "$bin"
# Gate runs are muted: sounds are judged in Jake's play session, not here.
root="$(mktemp -d)"
mkdir -p "$root/userdata"
echo '{"audio":{"muted":true}}' > "$root/userdata/settings.json"
export PIECED_ROOT="$root"
stamp=$(date +%Y%m%d-%H%M%S)
echo "power: $(pmset -g batt | head -1)"
echo "low power mode: $(pmset -g | awk '/lowpowermode/ {print $2}')"
runs=()
run() { # name, args...
  local name=$1; shift
  local out="evidence/m2-$stamp-$name"
  echo "== $name"
  timeout 900 "$bin" --evidence "$out" "$@" | grep -E "PIECED_(LAUNCH_MS|SCENARIO_DONE)" || true
  runs+=("$out")
}
if [[ "${1:-}" != "--only-perf" ]]; then
  for i in 1 2 3; do run "launch$i" --scenario smoke --seconds 3; done
  run latency --scenario latency
  run ttk --scenario ttk
  run fx --scenario fx_check
  run gallery --scenario gallery
  run sky --scenario sky_check
fi
if [[ "${1:-}" == "--perf" || "${1:-}" == "--only-perf" ]]; then
  run perf --scenario perf --seconds 300
fi
python3 scripts/evidence.py "${runs[@]}"
if [ -d "evidence/m2-$stamp-gallery" ]; then
  python3 scripts/board.py "evidence/m2-$stamp-gallery" && echo "board: evidence/m2-board/index.html"
fi
