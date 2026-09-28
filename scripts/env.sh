# Source from anywhere inside the repo or one of its worktrees:  source scripts/env.sh
# Uses the workspace's isolated Rust toolchain and one shared build cache outside the repo
# (and outside iCloud).
_pieced_main="$(cd "$(git rev-parse --git-common-dir)/.." && pwd)"
_pieced_ws="$(cd "$_pieced_main/../.." && pwd)"
export CARGO_HOME="$_pieced_ws/work/cargo"
export RUSTUP_HOME="$_pieced_ws/work/rustup"
export PATH="$CARGO_HOME/bin:$PATH"
# The build cache lives outside ~/Documents: that folder is synced by iCloud Drive,
# and syncing gigabytes of build artifacts pegs the CPU (and fails when full).
export CARGO_TARGET_DIR="${PIECED_TARGET_DIR:-$HOME/Library/Caches/pieced-target}"
unset _pieced_main _pieced_ws

# Every worktree compiles this crate to the same artifact names in the shared cache,
# and cargo judges freshness by mtime, so it could reuse another worktree's build.
# Touch this checkout's sources before every cargo command so it always rebuilds them.
# While Jake plays a logged session, scripts/quiet.sh sets this flag: wait so the
# session measures the game, not a build.
pieced_wait_quiet() {
  while [ -f "$CARGO_TARGET_DIR/.quiet" ]; do
    [ -n "${_pieced_said_quiet:-}" ] || { echo "(paused: Jake is playing; waiting for scripts/quiet.sh cont)" >&2; _pieced_said_quiet=1; }
    sleep 10
  done
  unset _pieced_said_quiet
}

cargo() {
  pieced_wait_quiet
  # A full disk corrupts half-linked test binaries; refuse early instead.
  _pieced_free_kb=$(df -k "$HOME" | awk 'NR==2 {print $4}')
  if [ "${_pieced_free_kb:-0}" -lt 3000000 ]; then
    echo "cargo: under 3 GB free on disk; not building. Report to the orchestrator (don't prune)." >&2
    unset _pieced_free_kb
    return 1
  fi
  unset _pieced_free_kb
  _pieced_root="$(git rev-parse --show-toplevel 2>/dev/null)"
  if [ -n "$_pieced_root" ]; then
    find "$_pieced_root/src" "$_pieced_root/tests" -name '*.rs' -exec touch {} + 2>/dev/null
  fi
  unset _pieced_root
  # Jake uses this Mac while builds run: compile at utility QoS and nice 10 so his apps
  # stay smooth. Anything started through `cargo run` inherits this, so never time a game
  # launched that way (scripts/play.sh and gate-runs.sh exec the binary directly).
  # PIECED_FULL_SPEED=1 opts out during a "go" window.
  if [ -n "$PIECED_FULL_SPEED" ]; then
    command cargo "$@"
  else
    taskpolicy -c utility nice -n 10 cargo "$@"
  fi
}
# No incremental caches: the sources are touched before every build anyway, and
# with several worktrees they grew to 6 GB on a disk with ~10 GB free.
export CARGO_INCREMENTAL=0
# Tests each start Bevy apps with their own thread pools: cap parallel tests too.
export RUST_TEST_THREADS="${RUST_TEST_THREADS:-4}"
# Leave cores free for Jake's apps.
export CARGO_BUILD_JOBS="${CARGO_BUILD_JOBS:-6}"
