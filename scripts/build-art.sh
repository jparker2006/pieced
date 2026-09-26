#!/bin/sh
# Regenerates Pieced's Blender-made models, headless and at low priority.
#
#   scripts/build-art.sh                 # every asset -> assets/models + art/previews
#   scripts/build-art.sh rock_a tree_a   # just these (manifest.json still lists all)
#   scripts/build-art.sh --no-previews   # skip the EEVEE preview renders
#   scripts/build-art.sh --list          # list registered assets
#   scripts/build-art.sh --check         # rebuild everything into a temp dir and fail
#                                        # unless it matches the committed files byte for byte
#
# Needs Blender 5.2 (`blender` on PATH, or $BLENDER). Never opens a window.
# Exits non-zero on any error; the Blender script must also print its "ART OK" line.
set -eu
cd "$(dirname "$0")/.."
BLENDER="${BLENDER:-blender}"
command -v "$BLENDER" >/dev/null 2>&1 || { echo "build-art: blender not found (set BLENDER)" >&2; exit 1; }

run_blender() { # args after --
  log=$(mktemp -t pieced-art)
  status=0
  taskpolicy -c utility nice -n 10 "$BLENDER" -b --factory-startup --python-exit-code 1 \
    -P art/blender/build.py -- "$@" >"$log" 2>&1 || status=$?
  grep -E "^(ART|Traceback|  File|[A-Za-z]*Error)" "$log" || true
  if [ "$status" -ne 0 ] || ! grep -qE "^ART (OK|asset)" "$log"; then
    echo "build-art: FAILED (exit $status); full Blender log: $log" >&2
    exit 1
  fi
  rm -f "$log"
}

if [ "${1:-}" = "--check" ]; then
  tmp=$(mktemp -d -t pieced-art-check)
  run_blender --no-previews --out "$tmp"
  bad=0
  for f in "$tmp"/*; do
    name=$(basename "$f")
    if ! cmp -s "$f" "assets/models/$name"; then
      echo "build-art --check: assets/models/$name differs from a fresh build" >&2
      bad=1
    fi
  done
  for f in assets/models/*; do
    [ -e "$tmp/$(basename "$f")" ] || { echo "build-art --check: $f is not produced by the build" >&2; bad=1; }
  done
  rm -rf "$tmp"
  [ "$bad" -eq 0 ] || exit 1
  echo "build-art --check: assets/models matches a fresh build"
  exit 0
fi
run_blender "$@"
