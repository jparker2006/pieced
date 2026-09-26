#!/bin/bash
# Frees disk in the shared build cache by deleting stale builds of this crate and its
# test binaries (every worktree rebuilds them anyway), plus stale incremental caches.
# Dependency builds are left alone. Usage: scripts/prune-target.sh [minutes=120]
set -euo pipefail
cd "$(dirname "$0")/.."
. scripts/env.sh
age=${1:-120}
deps="$CARGO_TARGET_DIR/debug/deps"
before=$(df -h / | awk 'NR==2 {print $4}')
if [ -d "$deps" ]; then
  find "$deps" -maxdepth 1 -type f -mmin +"$age" \
    \( -name 'pieced-*' -o -name 'libpieced-*' -o -regex '.*/[a-z_]*-[0-9a-f]\{16\}' \) -delete 2>/dev/null || true
fi
find "$CARGO_TARGET_DIR/debug/incremental" -maxdepth 1 -mindepth 1 -mmin +"$age" -exec rm -rf {} + 2>/dev/null || true
echo "free disk: $before -> $(df -h / | awk 'NR==2 {print $4}')"
