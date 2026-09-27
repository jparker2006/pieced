//! Embeds the git commit at build time (the play binary runs outside git):
//! `PIECED_GIT_COMMIT` (short hash, or "unknown") and `PIECED_GIT_DIRTY`
//! ("1" when tracked files had uncommitted changes, else "0").

use std::{path::Path, process::Command};

fn git(args: &[&str]) -> Option<String> {
    let out = Command::new("git").args(args).output().ok()?;
    out.status
        .success()
        .then(|| String::from_utf8_lossy(&out.stdout).trim().to_string())
}

fn watch(path: Option<String>) {
    // A missing path would make cargo rerun this script on every build.
    if let Some(path) = path.filter(|p| !p.is_empty() && Path::new(p).exists()) {
        println!("cargo:rerun-if-changed={path}");
    }
}

fn main() {
    println!("cargo:rerun-if-changed=build.rs");
    // The dirty flag follows the sources (scripts/env.sh touches them before
    // every cargo command, so this reruns whenever the crate rebuilds).
    println!("cargo:rerun-if-changed=src");

    let commit = git(&["rev-parse", "--short=12", "HEAD"]).filter(|c| !c.is_empty());
    let dirty = commit.is_some()
        && git(&["status", "--porcelain", "--untracked-files=no"]).is_some_and(|s| !s.is_empty());

    // HEAD moves on commit and checkout; the branch ref (loose or packed) on commit.
    watch(git(&["rev-parse", "--git-path", "HEAD"]));
    watch(git(&["rev-parse", "--git-path", "packed-refs"]));
    watch(git(&["rev-parse", "--git-path", "index"]));
    if let Some(branch) = git(&["symbolic-ref", "-q", "HEAD"]) {
        watch(git(&["rev-parse", "--git-path", &branch]));
    }

    println!(
        "cargo:rustc-env=PIECED_GIT_COMMIT={}",
        commit.as_deref().unwrap_or("unknown")
    );
    println!(
        "cargo:rustc-env=PIECED_GIT_DIRTY={}",
        if dirty { "1" } else { "0" }
    );
}
