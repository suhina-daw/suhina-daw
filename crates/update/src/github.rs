// Copyright (C) 2026 The Suhina contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! What GitHub's API says about the nightly release and the test builds, read into [`Build`]s
//! (`docs/spec/updates.md` › What is sent).

use std::collections::HashMap;

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

/// A pull request's comments, where test-build-links.yml links its test build
/// ([`linked_artifacts`]); the first page only, which holds the link from the first build on.
pub fn comments_url(pr: u32) -> String {
    format!("https://api.github.com/repos/{REPOSITORY}/issues/{pr}/comments?per_page=100")
}

/// The marker test-build-links.yml starts its comment with.
const LINK_MARKER: &str = "<!-- suhina-test-build -->";
/// The account test-build-links.yml comments as: the workflow's own token.
const LINK_AUTHOR: &str = "github-actions[bot]";

/// The builds a pull request's test-build comment links, as `(run, artifact)` ids, from the
/// comments' JSON. Only the comment test-build-links.yml writes counts: it runs from main and links
/// a fork's build only when a maintainer or trusted contributor asked for that commit to be built
/// (`docs/releasing.md` › Test builds). A comment anyone else wrote links nothing.
pub fn linked_artifacts(json: &[u8]) -> Result<Vec<(u64, u64)>, String> {
    let comments: Value = serde_json::from_slice(json).map_err(|e| format!("GitHub's answer is not JSON: {e}"))?;
    let prefix = format!("https://github.com/{REPOSITORY}/actions/runs/");
    let mut linked = Vec::new();
    for comment in comments.as_array().into_iter().flatten() {
        let user = &comment["user"];
        let body = comment["body"].as_str().unwrap_or_default();
        if user["login"].as_str() != Some(LINK_AUTHOR) || user["type"].as_str() != Some("Bot") {
            continue;
        }
        if !body.starts_with(LINK_MARKER) {
            continue;
        }
        for (at, _) in body.match_indices(&prefix) {
            let rest = body.get(at + prefix.len()..).unwrap_or_default();
            let digits = |s: &str| s.bytes().take_while(u8::is_ascii_digit).count();
            let run_len = digits(rest);
            let Some(after_run) = rest.get(run_len..).and_then(|r| r.strip_prefix("/artifacts/")) else {
                continue;
            };
            let id_len = digits(after_run);
            let run = rest.get(..run_len).and_then(|r| r.parse::<u64>().ok());
            let id = after_run.get(..id_len).and_then(|r| r.parse::<u64>().ok());
            if let (Some(run), Some(id)) = (run, id) {
                linked.push((run, id));
            }
        }
    }
    Ok(linked)
}

/// One test build in the artifacts' list, before it is trusted.
struct Listed {
    build: Build,
    run: u64,
    artifact: u64,
    from_fork: bool,
}

/// The newest trusted test build of each pull request for `platform`, newest pull request first,
/// from the artifacts' JSON. `core` is the version the builds share before `-pr.` (the running
/// one's).
///
/// An artifact's name says nothing about who made it: a pull request from a fork runs the workflow
/// files as the fork has them, so it can upload any name, another pull request's too. So a build is
/// listed only when its run is of a branch in this repository, which only people who can write to
/// it can push, or when `linked(pr)`, the builds that pull request's test-build comment links
/// ([`linked_artifacts`]), names its run and artifact. `linked` is asked once per pull request with
/// a build from a fork; an answer that cannot be read links nothing.
pub fn test_builds(
    json: &[u8],
    platform: &str,
    core: &str,
    mut linked: impl FnMut(u32) -> Vec<(u64, u64)>,
) -> Result<Vec<Build>, String> {
    let list: Value = serde_json::from_slice(json).map_err(|e| format!("GitHub's answer is not JSON: {e}"))?;
    let suffix = format!("-{platform}");
    let mut listed: Vec<Listed> = Vec::new();
    for artifact in list["artifacts"].as_array().into_iter().flatten() {
        if artifact["expired"].as_bool() != Some(false) {
            continue;
        }
        let name = artifact["name"].as_str().unwrap_or_default();
        let Some(pr) = name
            .strip_prefix("suhina-pr")
            .and_then(|rest| rest.strip_suffix(&suffix))
            .and_then(|n| n.parse::<u32>().ok())
        else {
            continue;
        };
        let run = &artifact["workflow_run"];
        // Without the ids, where the build came from is unknown: not listed.
        let (Some(id), Some(run_id), Some(repository), Some(head_repository)) = (
            artifact["id"].as_u64(),
            run["id"].as_u64(),
            run["repository_id"].as_u64(),
            run["head_repository_id"].as_u64(),
        ) else {
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
        listed.push(Listed { build, run: run_id, artifact: id, from_fork: head_repository != repository });
    }
    let mut asked: HashMap<u32, Vec<(u64, u64)>> = HashMap::new();
    let mut builds: Vec<Build> = Vec::new();
    for item in listed {
        let Some(pr) = item.build.pr else { continue };
        if item.from_fork && !asked.entry(pr).or_insert_with(|| linked(pr)).contains(&(item.run, item.artifact)) {
            continue;
        }
        let build = item.build;
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
