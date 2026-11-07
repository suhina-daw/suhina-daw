// Copyright (C) 2026 The Suhina contributors
// SPDX-License-Identifier: GPL-3.0-or-later

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

/// A run of a branch in this repository (`fork: false`) or of a fork's.
fn run(id: u64, sha: &str, fork: bool) -> String {
    let head = if fork { 2 } else { 1 };
    format!(r#"{{"id": {id}, "repository_id": 1, "head_repository_id": {head}, "head_sha": "{sha}"}}"#)
}

const DOWNLOADS: &str = "https://api.example.invalid";

fn artifact(id: u64, name: &str, created: &str, run: &str) -> String {
    format!(
        r#"{{"id": {id}, "name": "{name}", "expired": false, "created_at": "{created}",
            "archive_download_url": "{DOWNLOADS}/{id}/zip", "size_in_bytes": {id}, "workflow_run": {run}}}"#
    )
}

fn list(artifacts: &[String]) -> String {
    format!(r#"{{"artifacts": [{}]}}"#, artifacts.join(","))
}

/// The marker test-build-links.yml uploads for pull request `pr`'s build in run `linked`, from its
/// own run `id` (of this repository unless `fork`).
fn marker(id: u64, pr: u32, linked: u64, fork: bool) -> String {
    artifact(id, &format!("suhina-linked-pr{pr}-run{linked}"), "2026-10-18T19:00:00Z", &run(900 + id, "0000000", fork))
}

#[test]
fn test_builds_are_the_newest_of_each_pull_request_for_this_platform() {
    let mut artifacts = vec![
        artifact(
            1,
            "suhina-pr663-windows-x86_64",
            "2026-10-18T16:53:21Z",
            &run(10, "4d01080d14944648d5d34f469469c36596ea677e", false),
        ),
        artifact(
            2,
            "suhina-pr663-windows-x86_64",
            "2026-10-18T17:13:00Z",
            &run(11, "10d7891ddc5eb7998083af818ed6e3cf35b305d5", false),
        )
        .replace(
            r#""workflow_run""#,
            r#""digest": "sha256:35f795367fe9dc06fbbfaf6f44b607099ae5d2dcd8d659ca67d7ebee6efea047", "workflow_run""#,
        ),
        artifact(3, "suhina-pr672-windows-x86_64", "2026-10-18T16:48:41Z", &run(12, "f35ecdd", false)),
        artifact(4, "suhina-pr672-linux-x86_64", "2026-10-18T16:48:41Z", &run(12, "f35ecdd", false)),
        artifact(5, "suhina-pr600-windows-x86_64", "2026-10-01T00:00:00Z", &run(13, "aaaaaaa", false))
            .replace(r#""expired": false"#, r#""expired": true"#),
        artifact(6, "suhina-prs-302-303-windows-x86_64", "2026-10-18T00:00:00Z", &run(14, "bbbbbbb", false)),
    ];
    // Without the ids, where a build came from is unknown, so it is not listed.
    artifacts.push(
        r#"{"id": 7, "name": "suhina-pr690-windows-x86_64", "expired": false, "created_at": "2026-10-18T00:00:00Z",
            "archive_download_url": "https://api.example.invalid/7/zip", "workflow_run": {"id": 15, "head_sha": "ccccccc"}}"#
            .to_string(),
    );
    let builds = test_builds(list(&artifacts).as_bytes(), "windows-x86_64", "0.0.1").unwrap_or_else(|e| panic!("{e}"));
    let listed: Vec<(Option<u32>, &str)> = builds.iter().map(|b| (b.pr, b.version.as_str())).collect();
    assert_eq!(listed, [(Some(672), "0.0.1-pr.672+f35ecdd"), (Some(663), "0.0.1-pr.663+10d7891")]);
    assert_eq!(builds[1].url, "https://api.example.invalid/2/zip");
    assert_eq!(builds[1].sha256.as_deref(), Some("35f795367fe9dc06fbbfaf6f44b607099ae5d2dcd8d659ca67d7ebee6efea047"));
    assert!(builds[1].needs_sign_in);
    assert_eq!(builds[1].page, "https://github.com/suhina-daw/suhina-daw/pull/663");
}

#[test]
fn a_build_from_a_fork_is_listed_only_when_a_run_of_this_repository_marks_it() {
    let artifacts = [
        // A fork's run names its artifact after pull request 663, newer than 663's real build: no
        // marker names it, so the real build stays the one listed and offered.
        artifact(1, "suhina-pr663-windows-x86_64", "2026-10-18T16:00:00Z", &run(10, "1111111", false)),
        artifact(2, "suhina-pr663-windows-x86_64", "2026-10-18T18:00:00Z", &run(20, "6666666", true)),
        // A fork's own pull request: both builds are marked, the newer one is listed.
        artifact(3, "suhina-pr700-windows-x86_64", "2026-10-18T16:00:00Z", &run(30, "3333333", true)),
        artifact(4, "suhina-pr700-windows-x86_64", "2026-10-18T17:00:00Z", &run(31, "4444444", true)),
        marker(5, 700, 30, false),
        marker(6, 700, 31, false),
        // Marked for another pull request, and for another run of its own: neither does.
        artifact(7, "suhina-pr701-windows-x86_64", "2026-10-18T17:00:00Z", &run(40, "5555555", true)),
        marker(8, 702, 40, false),
        marker(9, 701, 41, false),
        // Marked from a fork's run, which can upload any name: not listed.
        artifact(10, "suhina-pr702-windows-x86_64", "2026-10-18T17:00:00Z", &run(50, "7777777", true)),
        marker(11, 702, 50, true),
    ];
    let builds = test_builds(list(&artifacts).as_bytes(), "windows-x86_64", "0.0.1").unwrap_or_else(|e| panic!("{e}"));
    let listed: Vec<(Option<u32>, &str)> = builds.iter().map(|b| (b.pr, b.commit.as_str())).collect();
    assert_eq!(listed, [(Some(700), "4444444"), (Some(663), "1111111")]);
}

#[test]
fn a_markers_name_is_read_whole() {
    assert_eq!(linked_marker("suhina-linked-pr700-run31"), Some((700, 31)));
    for name in [
        "suhina-linked-pr-run31",
        "suhina-linked-pr700-run",
        "suhina-linked-pr700-run31x",
        "suhina-linked-pr+700-run31",
        "suhina-linked-pr700-run+31",
        "suhina-linked-pr700-run31-run5",
        "suhina-linked-pr99999999999-run31",
        "suhina-linked-pr700-run99999999999999999999999",
        "suhina-linked-pr700",
        "suhina-pr700-windows-x86_64",
    ] {
        assert_eq!(linked_marker(name), None, "{name}");
    }
}
