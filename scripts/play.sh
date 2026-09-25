#!/bin/sh
# Build (if needed) and play Pieced. Extra arguments go to the game, e.g.
#   scripts/play.sh --windowed
set -eu
cd "$(dirname "$0")/.."
. scripts/env.sh
cargo build --release --locked --quiet
exec "$CARGO_TARGET_DIR/release/pieced" "$@"
