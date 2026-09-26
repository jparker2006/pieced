#!/bin/sh
# Re-samples art/palette.json from the local target images (docs/design/concepts,
# git-ignored) and writes contact sheets to art/previews/palette/ for checking the
# sample points. Headless, low priority. Then update the Rust mirror:
#   python3 art/tools/palette_to_rust.py --write
set -eu
cd "$(dirname "$0")/.."
BLENDER="${BLENDER:-blender}"
# The targets are git-ignored, so a worktree has none: read the main checkout's.
concepts=docs/design/concepts
if [ ! -f "$concepts/T01-spawn-vista.png" ]; then
  concepts="$(cd "$(git rev-parse --git-common-dir)/.." && pwd)/docs/design/concepts"
fi
log=$(mktemp -t pieced-palette)
status=0
taskpolicy -c utility nice -n 10 "$BLENDER" -b --factory-startup --python-exit-code 1 \
  -P art/tools/sample_palette.py -- --concepts "$concepts" "$@" >"$log" 2>&1 || status=$?
grep -E "^PALETTE|Error|Traceback|  File" "$log" || true
if [ "$status" -ne 0 ] || ! grep -q "^PALETTE wrote" "$log"; then
  echo "sample-palette: FAILED (exit $status); full log: $log" >&2
  exit 1
fi
rm -f "$log"
