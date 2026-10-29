// Copyright (C) 2026 The Suhina contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! Updates (`docs/spec/updates.md`): whether a newer build than the running one is out in the
//! update channel, and downloading it, checking it and putting it in place of the running one.
//! The work runs on a thread of its own; [`Updater::phase`] says where it is.

pub mod github;
pub mod install;
pub mod net;
pub mod version;

use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex, PoisonError};

pub use github::Build;
pub use version::BuildKind;

/// Where updates come from (Settings › Agents and updates › Updates › Channel).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Channel {
    #[default]
    Nightly,
    /// Test builds of pull requests: only in developer mode.
    TestBuilds,
}

/// What the updater is doing, or found.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub enum Phase {
    #[default]
    Idle,
    Checking,
    /// The channel's builds were read. `newer` is the one to offer without being asked for;
    /// `builds` are all the channel has (the nightly, or the test builds).
    Checked {
        newer: Option<Build>,
        builds: Vec<Build>,
    },
    /// A check failed: no connection, or an answer that could not be read.
    CheckFailed(String),
    Downloading {
        build: Build,
        done: u64,
    },
    Installing(Build),
    /// The build is in place; it runs after a restart.
    Installed(Build),
    InstallFailed {
        build: Build,
        reason: String,
    },
}

#[derive(Default)]
struct Shared {
    phase: Mutex<Phase>,
    done: AtomicU64,
}

/// Checks for updates and installs them on a thread of its own.
#[derive(Clone, Default)]
pub struct Updater {
    shared: Arc<Shared>,
}

impl Updater {
    pub fn phase(&self) -> Phase {
        let phase = self.shared.phase.lock().unwrap_or_else(PoisonError::into_inner).clone();
        match phase {
            Phase::Downloading { build, .. } => {
                Phase::Downloading { build, done: self.shared.done.load(Ordering::Relaxed) }
            }
            phase => phase,
        }
    }

    fn set(&self, phase: Phase) {
        *self.shared.phase.lock().unwrap_or_else(PoisonError::into_inner) = phase;
    }

    /// Whether a check, a download or an install is running.
    pub fn busy(&self) -> bool {
        matches!(self.phase(), Phase::Checking | Phase::Downloading { .. } | Phase::Installing(_))
    }

    /// Asks the channel for its builds and compares them with `current`, the running version;
    /// `done` is called when the answer is in (to repaint the window).
    pub fn check(&self, channel: Channel, current: &str, done: impl Fn() + Send + 'static) {
        if self.busy() {
            return;
        }
        self.set(Phase::Checking);
        let (updater, current) = (self.clone(), current.to_string());
        let spawned = std::thread::Builder::new().name("update check".into()).spawn(move || {
            updater.set(match check(channel, &current) {
                Ok((newer, builds)) => Phase::Checked { newer, builds },
                Err(reason) => Phase::CheckFailed(reason),
            });
            done();
        });
        if let Err(e) = spawned {
            self.set(Phase::CheckFailed(format!("could not start the check: {e}")));
        }
    }

    /// Downloads `build`, checks it and puts it in place of the program in `dir`; `token` signs
    /// the download in (a test build), `repaint` is called as it goes.
    pub fn install(&self, build: Build, dir: PathBuf, token: Option<String>, repaint: impl Fn() + Send + 'static) {
        if self.busy() {
            return;
        }
        self.shared.done.store(0, Ordering::Relaxed);
        self.set(Phase::Downloading { build: build.clone(), done: 0 });
        let (updater, failed) = (self.clone(), build.clone());
        let spawned = std::thread::Builder::new().name("update install".into()).spawn(move || {
            let result = updater.download_and_replace(&build, &dir, token.as_deref(), &repaint);
            let _removed = std::fs::remove_dir_all(install::staging_folder(&dir));
            updater.set(match result {
                Ok(()) => Phase::Installed(build),
                Err(reason) => Phase::InstallFailed { build, reason },
            });
            repaint();
        });
        if let Err(e) = spawned {
            self.set(Phase::InstallFailed { build: failed, reason: format!("could not start the download: {e}") });
        }
    }

    fn download_and_replace(
        &self,
        build: &Build,
        dir: &Path,
        token: Option<&str>,
        repaint: &dyn Fn(),
    ) -> Result<(), String> {
        if build.needs_sign_in && token.is_none() {
            return Err("downloading a test build needs Suhina connected to your gh".to_string());
        }
        // Staged beside the program, or beside its app bundle, so moving it in place is a rename on
        // one disk.
        let staging = install::staging_folder(dir);
        let _removed = std::fs::remove_dir_all(&staging);
        std::fs::create_dir_all(&staging).map_err(|e| {
            format!("Suhina's folder {} cannot be written to: {e}", staging.parent().unwrap_or(dir).display())
        })?;
        let archive = staging.join("download");
        let unpacked = staging.join("unpacked");
        let token = if build.needs_sign_in { token } else { None };
        net::download(&build.url, token, &archive, &mut |done| {
            self.shared.done.store(done, Ordering::Relaxed);
            repaint();
        })?;
        self.set(Phase::Installing(build.clone()));
        repaint();
        if let Some(expected) = &build.sha256 {
            let got = install::sha256(&archive)?;
            if &got != expected {
                return Err(format!("the download's checksum is {got}, GitHub lists {expected}: not installed"));
            }
        }
        std::fs::create_dir(&unpacked).map_err(|e| format!("could not unpack: {e}"))?;
        let program = install::unpack(&archive, &unpacked)?;
        install::put_in_place(&program, dir)
    }
}

/// Reads the channel's builds and picks the one newer than `current`.
fn check(channel: Channel, current: &str) -> Result<(Option<Build>, Vec<Build>), String> {
    let platform = github::platform().ok_or("there are no packages for this platform")?;
    match channel {
        Channel::Nightly => {
            let nightly = github::nightly(&net::get(github::NIGHTLY_URL, None)?, platform)?;
            let newer = version::nightly_is_newer(current, &nightly.version).then(|| nightly.clone());
            Ok((newer, vec![nightly]))
        }
        Channel::TestBuilds => {
            let builds =
                github::test_builds(&net::get(github::ARTIFACTS_URL, None)?, platform, &version::core(current))?;
            Ok((newer_test_build(current, &builds), builds))
        }
    }
}

/// The newest build of the pull request the running test build is of, when it is of another
/// commit.
fn newer_test_build(current: &str, builds: &[Build]) -> Option<Build> {
    let BuildKind::TestBuild { pr, commit } = BuildKind::of(current) else { return None };
    let commit = commit?;
    builds.iter().find(|b| b.pr == Some(pr) && !b.commit.starts_with(&commit)).cloned()
}

/// The folder the running program is in, which an update replaces the files of.
pub fn program_folder() -> Result<PathBuf, String> {
    let exe = std::env::current_exe().map_err(|e| format!("could not find the running program: {e}"))?;
    let exe = exe.canonicalize().unwrap_or(exe);
    exe.parent().map(Path::to_path_buf).ok_or_else(|| "the running program has no folder".to_string())
}

/// Deletes what an earlier update left beside the program; called once at start.
pub fn clean_up() {
    if let Ok(dir) = program_folder() {
        install::clean_up(&dir);
    }
}

/// Starts the program again (the updated one), with the arguments this one had; called as the
/// window closes after Restart now.
pub fn relaunch() -> Result<(), String> {
    let exe = std::env::current_exe().map_err(|e| format!("could not find the running program: {e}"))?;
    std::process::Command::new(exe)
        .args(std::env::args_os().skip(1))
        .spawn()
        .map(|_| ())
        .map_err(|e| format!("could not start Suhina again: {e}"))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn build(pr: u32, commit: &str) -> Build {
        Build { pr: Some(pr), commit: commit.into(), needs_sign_in: true, ..Build::default() }
    }

    #[test]
    fn a_test_build_is_offered_its_pull_requests_newer_commit_only() {
        let builds = [build(672, "f35ecdd921c4"), build(663, "10d7891ddc5e")];
        assert_eq!(newer_test_build("0.0.1-pr.663+4d01080", &builds), Some(build(663, "10d7891ddc5e")));
        assert_eq!(newer_test_build("0.0.1-pr.663+10d7891", &builds), None);
        assert_eq!(newer_test_build("0.0.1-pr.700+10d7891", &builds), None);
        assert_eq!(newer_test_build("0.0.1-nightly.20261018+c176a33", &builds), None);
    }

    #[test]
    fn a_test_build_is_not_downloaded_without_a_sign_in() {
        let updater = Updater::default();
        let dir = std::env::temp_dir().join(format!("suhina-update-sign-in-{}", std::process::id()));
        let result = updater.download_and_replace(&build(663, "10d7891"), &dir, None, &|| {});
        assert!(result.is_err_and(|e| e.contains("gh")));
    }
}
