#!/bin/bash
# The one way builders run cargo: `scripts/cargo.sh test --locked ...`.
# Sources scripts/env.sh (shared cache, toolchain, CARGO_INCREMENTAL=0) and runs
# its cargo wrapper: utility QoS + nice 10, 6 jobs, waits while Jake plays
# (scripts/quiet.sh), refuses under 3 GB free. Works where `source` is blocked.
set -o pipefail
cd "$(git rev-parse --show-toplevel)" || exit 1
. scripts/env.sh
cargo "$@"
