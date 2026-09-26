#!/bin/bash
# Performance baseline and cost breakdown (M2 Phase 0, and S2 tuning): one full perf
# run, then short runs that each switch one feature off via --knobs, so each
# feature's cost shows up in frame timing. Full screen: leave the Mac alone, window
# in front, on battery with Low Power Mode on for gate numbers.
#
#   scripts/perf-breakdown.sh <binary> [full_seconds=300] [knob_seconds=60] [knob ...]
#
# Default knobs are Milestone 1's; pass M2 knobs (outline=off, far=off, halos=off,
# blobs=off, ...) after the durations to break down the new look.
set -euo pipefail
cd "$(dirname "$0")/.."
bin=$1
full=${2:-300}
short=${3:-60}
shift $(( $# < 3 ? $# : 3 ))
knobs=("$@")
[ ${#knobs[@]} -eq 0 ] && knobs=(shadows=off msaa=1 vmmsaa=1 viewmodel=off scale=0.8 fog=off sky=off)
stamp=$(date +%Y%m%d-%H%M%S)
echo "binary: $bin"
echo "power: $(pmset -g batt | head -1)"
echo "low power mode: $(pmset -g | awk '/lowpowermode/ {print $2}')"
runs=()
run() { # name, args...
  local name=$1; shift
  local out="evidence/perf-$stamp-$name"
  echo "== $name"
  timeout 900 "$bin" --evidence "$out" --scenario perf "$@" | grep -E "PIECED_(LAUNCH_MS|SCENARIO_DONE)" || true
  runs+=("$out")
}
run full --seconds "$full"
for k in "${knobs[@]}"; do
  run "${k//=/-}" --seconds "$short" --knobs "$k"
done
python3 scripts/evidence.py "${runs[@]}"
