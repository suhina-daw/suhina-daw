// Copyright (C) 2026 The Suhina contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! What GitHub's API says about the nightly release and the test builds, read into [`Build`]s
//! (`docs/spec/updates.md` › What is sent).

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

/// The newest test build of each pull request for `platform`, newest pull request first, from
/// the artifacts' JSON. `core` is the version the builds share before `-pr.` (the running one's).
pub fn test_builds(json: &[u8], platform: &str, core: &str) -> Result<Vec<Build>, String> {
    let list: Value = serde_json::from_slice(json).map_err(|e| format!("GitHub's answer is not JSON: {e}"))?;
    let suffix = format!("-{platform}");
    let mut builds: Vec<Build> = Vec::new();
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
        let commit = artifact["workflow_run"]["head_sha"].as_str().unwrap_or_default().to_string();
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
mod tests {
    use super::*;

    const RELEASE: &str = r#"{
        "name": "Suhina nightly 0.0.1-nightly.20261018+c176a33",
        "tag_name": "nightly",
        "target_commitish": "c176a332f9c24c927717e1fd964fc2a147f15c7b",
        "assets": [
            {"name": "suhina-nightly-windows-x86_64.zip.sha256", "size": 100,
             "browser_download_url": "https://example.invalid/w.sha256", "digest": "sha256:7b36b58066fc5db9db028d7807797a0762f3c385f0bbcc943a26d089db72fe18"},
            {"name": "suhina-nightly-windows-x86_64.zip", "size": 17954272, "updated_at": "2026-10-18T07:32:16Z",
             "browser_download_url": "https://example.invalid/w.zip", "digest": "sha256:4D6D371FB9A69598531690942F8F0EF495E518FDAB4D3CC6E4ABCB84F00F056B"},
            {"name": "suhina-nightly-linux-x86_64.tar.gz", "size": 23335868,
             "browser_download_url": "https://example.invalid/l.tar.gz"}
        ]
    }"#;

    #[test]
    fn the_nightly_release_names_its_version_and_this_platforms_package() {
        let build = nightly(RELEASE.as_bytes(), "windows-x86_64").unwrap_or_else(|e| panic!("{e}"));
        assert_eq!(build.version, "0.0.1-nightly.20261018+c176a33");
        assert_eq!(build.url, "https://example.invalid/w.zip");
        assert_eq!(build.size, 17_954_272);
        assert_eq!(build.sha256.as_deref(), Some("4d6d371fb9a69598531690942f8f0ef495e518fdab4d3cc6e4abcb84f00f056b"));
        assert!(!build.needs_sign_in);
        // A package without a digest has no checksum to compare, and one for another platform
        // is an error.
        let linux = nightly(RELEASE.as_bytes(), "linux-x86_64").unwrap_or_else(|e| panic!("{e}"));
        assert_eq!(linux.sha256, None);
        assert!(nightly(RELEASE.as_bytes(), "macos-universal").is_err());
        assert!(nightly(b"<html>", "linux-x86_64").is_err());
    }

    #[test]
    fn test_builds_are_the_newest_of_each_pull_request_for_this_platform() {
        let json = r#"{"artifacts": [
            {"name": "suhina-pr663-windows-x86_64", "expired": false, "created_at": "2026-10-18T16:53:21Z",
             "archive_download_url": "https://api.example.invalid/1/zip", "size_in_bytes": 10,
             "workflow_run": {"head_sha": "4d01080d14944648d5d34f469469c36596ea677e"}},
            {"name": "suhina-pr663-windows-x86_64", "expired": false, "created_at": "2026-10-18T17:13:00Z",
             "archive_download_url": "https://api.example.invalid/2/zip", "size_in_bytes": 20,
             "digest": "sha256:35f795367fe9dc06fbbfaf6f44b607099ae5d2dcd8d659ca67d7ebee6efea047",
             "workflow_run": {"head_sha": "10d7891ddc5eb7998083af818ed6e3cf35b305d5"}},
            {"name": "suhina-pr672-windows-x86_64", "expired": false, "created_at": "2026-10-18T16:48:41Z",
             "archive_download_url": "https://api.example.invalid/3/zip", "workflow_run": {"head_sha": "f35ecdd"}},
            {"name": "suhina-pr672-linux-x86_64", "expired": false, "created_at": "2026-10-18T16:48:41Z",
             "archive_download_url": "https://api.example.invalid/4/zip", "workflow_run": {"head_sha": "f35ecdd"}},
            {"name": "suhina-pr600-windows-x86_64", "expired": true, "created_at": "2026-10-01T00:00:00Z",
             "archive_download_url": "https://api.example.invalid/5/zip", "workflow_run": {"head_sha": "aaaaaaa"}},
            {"name": "suhina-prs-302-303-windows-x86_64", "expired": false, "created_at": "2026-10-18T00:00:00Z",
             "archive_download_url": "https://api.example.invalid/6/zip", "workflow_run": {"head_sha": "bbbbbbb"}}
        ]}"#;
        let builds = test_builds(json.as_bytes(), "windows-x86_64", "0.0.1").unwrap_or_else(|e| panic!("{e}"));
        let listed: Vec<(Option<u32>, &str)> = builds.iter().map(|b| (b.pr, b.version.as_str())).collect();
        assert_eq!(listed, [(Some(672), "0.0.1-pr.672+f35ecdd"), (Some(663), "0.0.1-pr.663+10d7891")]);
        assert_eq!(builds[1].url, "https://api.example.invalid/2/zip");
        assert!(builds[1].needs_sign_in);
        assert_eq!(builds[1].page, "https://github.com/suhina-daw/suhina-daw/pull/663");
    }
}
