// Copyright (C) 2026 The Suhina contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! What GitHub's API says about the nightly release and the test builds, read into [`Build`]s
//! (`docs/spec/updates.md` › What is sent).

use std::collections::HashSet;

use serde_json::Value;

/// The repository the builds come from.
pub const REPOSITORY: &str = "suhina-daw/suhina-daw";
/// The nightly release, as the API lists it.
pub const NIGHTLY_URL: &str = "https://api.github.com/repos/suhina-daw/suhina-daw/releases/tags/nightly";
/// The nightly release's page, for a person.
pub const NIGHTLY_PAGE: &str = "https://github.com/suhina-daw/suhina-daw/releases/tag/nightly";
/// The newest workflow artifacts: the test builds among them.
pub const ARTIFACTS_URL: &str = "https://api.github.com/repos/suhina-daw/suhina-daw/actions/artifacts?per_page=100";

/// The package name's platform part for the platform Suhina was built for, as the release
/// workflow names its packages (`docs/releasing.md` › Packages); `None` where there is no package.
pub fn platform() -> Option<&'static str> {
    if cfg!(all(target_os = "windows", target_arch = "x86_64")) {
        Some("windows-x86_64")
    } else if cfg!(target_os = "macos") {
        Some("macos-universal")
    } else if cfg!(all(target_os = "linux", target_arch = "x86_64")) {
        Some("linux-x86_64")
    } else {
        None
    }
}

/// One build that can be downloaded.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct Build {
    /// Its version, as the build itself says it in Help › About.
    pub version: String,
    /// The file to download.
    pub url: String,
    /// The file's size, in bytes.
    pub size: u64,
    /// The file's SHA-256 as GitHub lists it, in lower-case hex.
    pub sha256: Option<String>,
    /// Downloading it needs a GitHub sign-in (a test build).
    pub needs_sign_in: bool,
    /// The pull request a test build is of.
    pub pr: Option<u32>,
    /// The commit it was built from, in full when GitHub gives it.
    pub commit: String,
    /// When it was built, as GitHub writes it (`2026-10-18T07:32:16Z`).
    pub created: String,
    /// A page about it for a person: the release, or the pull request.
    pub page: String,
}

fn sha256_of(digest: Option<&str>) -> Option<String> {
    digest
        .and_then(|d| d.strip_prefix("sha256:"))
        .filter(|hex| hex.len() == 64 && hex.bytes().all(|b| b.is_ascii_hexdigit()))
        .map(str::to_ascii_lowercase)
}

/// The nightly release's package for `platform`, from the release's JSON.
pub fn nightly(json: &[u8], platform: &str) -> Result<Build, String> {
    let release: Value = serde_json::from_slice(json).map_err(|e| format!("GitHub's answer is not JSON: {e}"))?;
    // The workflow titles it "Suhina nightly <version>".
    let name = release["name"].as_str().unwrap_or_default();
    let version = name.rsplit(' ').next().filter(|v| !v.is_empty()).ok_or("the nightly release has no version")?;
    let wanted = format!("suhina-nightly-{platform}.");
    let asset = release["assets"]
        .as_array()
        .into_iter()
        .flatten()
        .find(|a| a["name"].as_str().is_some_and(|n| n.starts_with(&wanted) && !n.ends_with(".sha256")))
        .ok_or_else(|| format!("the nightly has no package for {platform}"))?;
    Ok(Build {
        version: version.to_string(),
        url: asset["browser_download_url"].as_str().ok_or("the package has no download link")?.to_string(),
        size: asset["size"].as_u64().unwrap_or(0),
        sha256: sha256_of(asset["digest"].as_str()),
        needs_sign_in: false,
        pr: None,
        commit: release["target_commitish"].as_str().unwrap_or_default().to_string(),
        created: asset["updated_at"].as_str().unwrap_or_default().to_string(),
        page: NIGHTLY_PAGE.to_string(),
    })
}

/// The pull request and run a marker names, from its artifact's name
/// (`suhina-linked-pr<number>-run<run id>`): test-build-links.yml uploads one for each build from a
/// fork it links ([`test_builds`]).
fn linked_marker(name: &str) -> Option<(u32, u64)> {
    let (pr, run) = name.strip_prefix("suhina-linked-pr")?.split_once("-run")?;
    let digits = |s: &str| !s.is_empty() && s.bytes().all(|b| b.is_ascii_digit());
    if !digits(pr) || !digits(run) {
        return None;
    }
    Some((pr.parse().ok()?, run.parse().ok()?))
}

/// One test build in the artifacts' list, before it is trusted.
struct Listed {
    build: Build,
    run: u64,
    from_fork: bool,
}

/// The newest trusted test build of each pull request for `platform`, newest pull request first,
/// from the artifacts' JSON. `core` is the version the builds share before `-pr.` (the running
/// one's).
///
/// An artifact's name says nothing about who made it: a pull request from a fork runs the workflow
/// files as the fork has them, so it can upload any name, another pull request's too. So a build is
/// listed only when its run is of a branch in this repository, which only people who can write to
/// it can push, or when the same list holds the marker test-build-links.yml uploads for it
/// ([`linked_marker`]) from a run of this repository: that workflow runs from main and marks a
/// fork's build only after a `/test-build` request for its commit (`docs/releasing.md` › Test
/// builds), and no fork can upload into a run of this repository. A marker from a fork's run, or
/// for another pull request or run, marks nothing.
pub fn test_builds(json: &[u8], platform: &str, core: &str) -> Result<Vec<Build>, String> {
    let list: Value = serde_json::from_slice(json).map_err(|e| format!("GitHub's answer is not JSON: {e}"))?;
    let suffix = format!("-{platform}");
    let mut listed: Vec<Listed> = Vec::new();
    // The builds from forks test-build-links.yml marked, as (pull request, run).
    let mut linked: HashSet<(u32, u64)> = HashSet::new();
    for artifact in list["artifacts"].as_array().into_iter().flatten() {
        if artifact["expired"].as_bool() != Some(false) {
            continue;
        }
        let name = artifact["name"].as_str().unwrap_or_default();
        let run = &artifact["workflow_run"];
        // Without the ids, where the artifact came from is unknown: not used.
        let (Some(run_id), Some(repository), Some(head_repository)) =
            (run["id"].as_u64(), run["repository_id"].as_u64(), run["head_repository_id"].as_u64())
        else {
            continue;
        };
        let from_fork = head_repository != repository;
        if let Some(marker) = linked_marker(name) {
            if !from_fork {
                linked.insert(marker);
            }
            continue;
        }
        let Some(pr) = name
            .strip_prefix("suhina-pr")
            .and_then(|rest| rest.strip_suffix(&suffix))
            .and_then(|n| n.parse::<u32>().ok())
        else {
            continue;
        };
        let commit = run["head_sha"].as_str().unwrap_or_default().to_string();
        let created = artifact["created_at"].as_str().unwrap_or_default().to_string();
        let Some(url) = artifact["archive_download_url"].as_str() else { continue };
        let build = Build {
            version: format!("{core}-pr.{pr}+{}", commit.get(..7).unwrap_or(&commit)),
            url: url.to_string(),
            size: artifact["size_in_bytes"].as_u64().unwrap_or(0),
            sha256: sha256_of(artifact["digest"].as_str()),
            needs_sign_in: true,
            pr: Some(pr),
            commit,
            created,
            page: format!("https://github.com/{REPOSITORY}/pull/{pr}"),
        };
        listed.push(Listed { build, run: run_id, from_fork });
    }
    let mut builds: Vec<Build> = Vec::new();
    for Listed { build, run, from_fork } in listed {
        let Some(pr) = build.pr else { continue };
        if from_fork && !linked.contains(&(pr, run)) {
            continue;
        }
        match builds.iter_mut().find(|b| b.pr == Some(pr)) {
            // The times are all UTC in one format, so they sort as text.
            Some(older) if older.created < build.created => *older = build,
            Some(_) => {}
            None => builds.push(build),
        }
    }
    builds.sort_by_key(|b| std::cmp::Reverse(b.pr));
    Ok(builds)
}

#[cfg(test)]
#[path = "github_tests.rs"]
mod tests;
