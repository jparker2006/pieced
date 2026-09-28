#!/bin/bash
# Pause (or resume) every background build process while Jake plays, so a
# logged play session measures the game, not the builders.
#
#   scripts/quiet.sh stop   # SIGSTOP cargo, rustc, test binaries in the build cache, headless Blender
#   scripts/quiet.sh cont   # SIGCONT everything paused by the last `stop`
#
# While paused, new cargo commands wait (scripts/env.sh checks the flag).
# Paused pids are kept in $CARGO_TARGET_DIR/.quiet-pids. The game itself
# (pieced-play) is never touched.
set -uo pipefail
cd "$(dirname "$0")/.."
. scripts/env.sh
pids="$CARGO_TARGET_DIR/.quiet-pids"
case "${1:-}" in
  stop)
    # New cargo commands (and build-art.sh) wait while this flag exists (env.sh).
    touch "$CARGO_TARGET_DIR/.quiet"
    ps -axo pid=,command= | awk -v target="$CARGO_TARGET_DIR/debug/" '
      $0 ~ /pieced-play/ { next }
      $2 ~ /\/(cargo|rustc)$/ || index($2, target) == 1 || ($2 ~ /Blender$/ && $0 ~ / -b /) { print $1 }
    ' > "$pids"
    [ -s "$pids" ] && xargs kill -STOP < "$pids" 2>/dev/null
    echo "paused $(wc -l < "$pids" | tr -d ' ') processes"
    ;;
  cont)
    rm -f "$CARGO_TARGET_DIR/.quiet"
    [ -f "$pids" ] && xargs kill -CONT < "$pids" 2>/dev/null
    # Anything that started after the stop and is itself stopped gets resumed too.
    ps -axo pid=,stat=,command= | awk -v target="$CARGO_TARGET_DIR/debug/" '
      $2 ~ /^T/ && ($3 ~ /\/(cargo|rustc)$/ || index($3, target) == 1 || $3 ~ /Blender$/) { print $1 }
    ' | xargs kill -CONT 2>/dev/null
    rm -f "$pids"
    echo "resumed"
    ;;
  *) echo "usage: $0 stop|cont" >&2; exit 2 ;;
esac
