#!/bin/sh
# Regenerates Pieced's Blender-made models and UI images, headless and at low priority.
#
#   scripts/build-art.sh                 # every asset -> assets/models (+ art/previews)
#                                        # and every UI image -> assets/ui
#   scripts/build-art.sh rock_a logo     # just these (manifest.json still lists all)
#   scripts/build-art.sh --no-previews   # skip the EEVEE preview renders
#   scripts/build-art.sh --list          # list registered assets and UI images
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
  run_blender --no-previews --out "$tmp/models" --ui-out "$tmp/ui"
  bad=0
  for dir in models ui; do
    [ -d "$tmp/$dir" ] || mkdir -p "$tmp/$dir"
    for f in $(cd "$tmp/$dir" && find . -type f | sort); do
      if ! cmp -s "$tmp/$dir/$f" "assets/$dir/$f"; then
        echo "build-art --check: assets/$dir/${f#./} differs from a fresh build" >&2
        bad=1
      fi
    done
    if [ -d "assets/$dir" ]; then
      for f in $(cd "assets/$dir" && find . -type f ! -name .DS_Store | sort); do
        [ -e "$tmp/$dir/$f" ] || { echo "build-art --check: assets/$dir/${f#./} is not produced by the build" >&2; bad=1; }
      done
    fi
  done
  rm -rf "$tmp"
  [ "$bad" -eq 0 ] || exit 1
  echo "build-art --check: assets/models and assets/ui match a fresh build"
  exit 0
fi
run_blender "$@"
