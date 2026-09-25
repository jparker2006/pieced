# Source from anywhere inside the repo or one of its worktrees:  source scripts/env.sh
# Uses the workspace's isolated Rust toolchain and one shared build cache outside the repo.
_pieced_main="$(cd "$(git rev-parse --git-common-dir)/.." && pwd)"
_pieced_ws="$(cd "$_pieced_main/../.." && pwd)"
export CARGO_HOME="$_pieced_ws/work/cargo"
export RUSTUP_HOME="$_pieced_ws/work/rustup"
export PATH="$CARGO_HOME/bin:$PATH"
export CARGO_TARGET_DIR="$(cd "$_pieced_main/.." && pwd)/pieced-target"
unset _pieced_main _pieced_ws
