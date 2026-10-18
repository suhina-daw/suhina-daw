// Copyright (C) 2026 The Suhina contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! Puts the build string the About dialog shows into the environment: the date of the
//! checked-out commit and its abbreviated hash, `2026_10_16_a15ad35`, as `SUHINA_BUILD`. Where
//! there is no git (a source export) it is `unknown`, and a release build may set the
//! environment itself, which this takes over what nothing else says.
//!
//! A moved HEAD — a commit, a branch switch or a rebase — is what the name names, so the git
//! index and refs are watched and the crate rebuilds when they move.

use std::path::PathBuf;
use std::process::Command;

fn main() {
    println!("cargo:rerun-if-changed=build.rs");
    if let Some(git_dir) = one(&["rev-parse", "--git-dir"]) {
        let git_dir = PathBuf::from(git_dir);
        for watched in ["HEAD", "packed-refs"] {
            println!("cargo:rerun-if-changed={}", git_dir.join(watched).display());
        }
        println!("cargo:rerun-if-changed={}", git_dir.join("refs").display());
    }
    let build = std::env::var("SUHINA_BUILD").ok().filter(|s| !s.trim().is_empty()).unwrap_or_else(|| {
        match (
            one(&["rev-parse", "--short", "HEAD"]),
            one(&["log", "-1", "--date=format-local:%Y_%m_%d", "--format=%cd"]),
        ) {
            (Some(hash), Some(date)) => format!("{date}_{hash}"),
            _ => String::from("unknown"),
        }
    });
    println!("cargo:rustc-env=SUHINA_BUILD={build}");
}

/// The one line git's stdout gives; `None` without a git checkout or for anything unwelcome.
fn one(args: &[&str]) -> Option<String> {
    let out = Command::new("git").args(args).output().ok()?;
    if !out.status.success() {
        return None;
    }
    let line = String::from_utf8_lossy(&out.stdout).trim().to_owned();
    if line.is_empty() || line.len() > 128 { None } else { Some(line) }
}
