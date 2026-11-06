// Copyright (C) 2026 The Suhina contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! What kind of build a version names, and whether another build is newer
//! (`docs/spec/updates.md` › When a build is newer).

use semver::Version;

/// The kind of build a version string names (`docs/releasing.md` › Versions).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum BuildKind {
    /// `0.1.0` or a candidate, `0.1.0-rc.1`.
    Release,
    /// `0.0.1-nightly.20261013+a15ad35`: the day, and the commit when it is there.
    Nightly { day: u64, commit: Option<String> },
    /// `0.0.1-pr.663+4d01080`: a pull request's test build, and its commit.
    TestBuild { pr: u32, commit: Option<String> },
    /// `0.0.1`, built locally, or anything that is not one of the others.
    Local,
}

impl BuildKind {
    pub fn of(version: &str) -> Self {
        let Ok(parsed) = Version::parse(version) else { return Self::Local };
        let commit = (!parsed.build.is_empty()).then(|| parsed.build.as_str().to_string());
        let pre: Vec<&str> = parsed.pre.as_str().split('.').collect();
        match pre.as_slice() {
            [""] => {
                // A local build has the workspace version as it is, which is never released as
                // itself: releases are tagged and built by CI with the same number. Only a build
                // with no build metadata and a non-zero version reads as a release.
                if parsed.major == 0 && parsed.minor == 0 { Self::Local } else { Self::Release }
            }
            ["rc", n] if n.parse::<u32>().is_ok() => Self::Release,
            ["nightly", day] => day.parse().map_or(Self::Local, |day| Self::Nightly { day, commit }),
            ["pr", pr] => pr.parse().map_or(Self::Local, |pr| Self::TestBuild { pr, commit }),
            _ => Self::Local,
        }
    }
}

/// Whether the nightly `latest` should replace the running build `current` without being asked
/// for; a test build or a local build never is.
pub fn nightly_is_newer(current: &str, latest: &str) -> bool {
    let BuildKind::Nightly { day: latest_day, commit: latest_commit } = BuildKind::of(latest) else {
        return false;
    };
    match BuildKind::of(current) {
        BuildKind::Nightly { day, commit } => {
            latest_day > day || (latest_day == day && latest_commit.is_some() && latest_commit != commit)
        }
        BuildKind::Release => match (Version::parse(current), Version::parse(latest)) {
            (Ok(current), Ok(latest)) => latest.cmp_precedence(&current).is_gt(),
            _ => false,
        },
        BuildKind::TestBuild { .. } | BuildKind::Local => false,
    }
}

/// The version without its build metadata or pre-release part: `0.0.1` of
/// `0.0.1-nightly.20261013+a15ad35`.
pub fn core(version: &str) -> String {
    Version::parse(version).map_or_else(|_| version.to_string(), |v| format!("{}.{}.{}", v.major, v.minor, v.patch))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn versions_name_their_kind() {
        assert_eq!(
            BuildKind::of("0.0.1-nightly.20261013+a15ad35"),
            BuildKind::Nightly { day: 20_261_013, commit: Some("a15ad35".into()) }
        );
        assert_eq!(
            BuildKind::of("0.0.1-pr.663+4d01080"),
            BuildKind::TestBuild { pr: 663, commit: Some("4d01080".into()) }
        );
        assert_eq!(BuildKind::of("0.1.0"), BuildKind::Release);
        assert_eq!(BuildKind::of("0.1.0-rc.2"), BuildKind::Release);
        assert_eq!(BuildKind::of("0.0.1"), BuildKind::Local);
        assert_eq!(BuildKind::of("0.0.1-prs.302.303+abc1234"), BuildKind::Local);
        assert_eq!(BuildKind::of("not a version"), BuildKind::Local);
    }

    #[test]
    fn a_later_nightly_is_newer_and_the_same_one_is_not() {
        let running = "0.0.1-nightly.20261013+a15ad35";
        assert!(nightly_is_newer(running, "0.0.1-nightly.20261018+c176a33"));
        // The afternoon's build of the same day is another commit.
        assert!(nightly_is_newer(running, "0.0.1-nightly.20261013+bbbbbbb"));
        assert!(!nightly_is_newer(running, running));
        assert!(!nightly_is_newer(running, "0.0.1-nightly.20261012+0000000"));
    }

    #[test]
    fn local_and_test_builds_are_never_offered_the_nightly_on_their_own() {
        assert!(!nightly_is_newer("0.0.1", "0.0.1-nightly.20261018+c176a33"));
        assert!(!nightly_is_newer("0.0.1-pr.663+4d01080", "0.0.1-nightly.20261018+c176a33"));
        // A release is offered a nightly of a later version only.
        assert!(nightly_is_newer("0.1.0", "0.2.0-nightly.20270101+abcdef0"));
        assert!(!nightly_is_newer("0.1.0", "0.1.0-nightly.20270101+abcdef0"));
        assert!(!nightly_is_newer("0.0.1-nightly.20261013+a15ad35", "0.1.0"));
    }

    #[test]
    fn the_core_drops_the_pre_release_and_the_commit() {
        assert_eq!(core("0.0.1-nightly.20261013+a15ad35"), "0.0.1");
        assert_eq!(core("0.3.2"), "0.3.2");
    }
}
