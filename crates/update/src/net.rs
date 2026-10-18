// Copyright (C) 2026 The Suhina contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! Requests through the system's `curl`, and the GitHub CLI's token
//! (`docs/spec/updates.md` › What is sent, › Connecting to the GitHub CLI).

use std::io::Write;
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::time::Duration;

/// How long a check may take before it gives up: no connection says nothing on the start panel.
const CHECK_SECONDS: &str = "15";
const CONNECT_SECONDS: &str = "5";

fn command(program: &str) -> Command {
    let mut command = Command::new(program);
    command.stdin(Stdio::null()).stdout(Stdio::null()).stderr(Stdio::null());
    #[cfg(windows)]
    {
        // No console window flashing up behind Suhina's own.
        use std::os::windows::process::CommandExt;
        const CREATE_NO_WINDOW: u32 = 0x0800_0000;
        command.creation_flags(CREATE_NO_WINDOW);
    }
    command
}

/// `curl` with what every request has: errors as errors, redirects followed, a user agent.
fn curl(token: Option<&str>) -> Command {
    let mut curl = command(if cfg!(windows) { "curl.exe" } else { "curl" });
    curl.args(["--silent", "--show-error", "--fail", "--location", "--connect-timeout", CONNECT_SECONDS])
        .args(["--header", "Accept: application/vnd.github+json"])
        .arg("--header")
        .arg(format!("User-Agent: Suhina/{}", env!("CARGO_PKG_VERSION")))
        .stderr(Stdio::piped());
    if token.is_some() {
        // The token's header is read from standard input, so it is not on the command line.
        curl.args(["--header", "@-"]).stdin(Stdio::piped());
    }
    curl
}

/// Starts `curl`, handing it the token's header on its standard input.
fn start(mut curl: Command, token: Option<&str>) -> Result<Child, String> {
    let mut child = curl.spawn().map_err(|e| {
        if e.kind() == std::io::ErrorKind::NotFound {
            "curl was not found: Suhina checks for updates through the system's curl".to_string()
        } else {
            format!("could not start curl: {e}")
        }
    })?;
    if let Some(token) = token
        && let Some(mut stdin) = child.stdin.take()
    {
        let header = format!("Authorization: Bearer {token}\n");
        stdin.write_all(header.as_bytes()).map_err(|e| format!("could not hand curl the sign-in: {e}"))?;
    }
    Ok(child)
}

fn failure(stderr: &[u8]) -> String {
    let text = String::from_utf8_lossy(stderr);
    let text = text.trim().trim_start_matches("curl: ");
    if text.is_empty() { "the request failed".to_string() } else { text.to_string() }
}

/// GETs `url` and returns the body.
pub fn get(url: &str, token: Option<&str>) -> Result<Vec<u8>, String> {
    let mut curl = curl(token);
    curl.args(["--max-time", CHECK_SECONDS]).arg(url).stdout(Stdio::piped());
    let output = start(curl, token)?.wait_with_output().map_err(|e| format!("curl: {e}"))?;
    if output.status.success() { Ok(output.stdout) } else { Err(failure(&output.stderr)) }
}

/// Downloads `url` to `to`, telling `progress` the number of bytes written so far, about ten
/// times a second.
pub fn download(url: &str, token: Option<&str>, to: &Path, progress: &mut dyn FnMut(u64)) -> Result<(), String> {
    let mut curl = curl(token);
    curl.args(["--retry", "2", "--output"]).arg(to).arg(url);
    let mut child = start(curl, token)?;
    let status = loop {
        if let Some(status) = child.try_wait().map_err(|e| format!("curl: {e}"))? {
            break status;
        }
        progress(std::fs::metadata(to).map_or(0, |m| m.len()));
        std::thread::sleep(Duration::from_millis(100));
    };
    progress(std::fs::metadata(to).map_or(0, |m| m.len()));
    if status.success() {
        return Ok(());
    }
    let mut stderr = Vec::new();
    if let Some(mut pipe) = child.stderr.take() {
        let _read = std::io::Read::read_to_end(&mut pipe, &mut stderr);
    }
    Err(failure(&stderr))
}

/// Where the GitHub CLI is, when it is on the `PATH`. Only looks: nothing is run.
pub fn gh() -> Option<PathBuf> {
    let name = if cfg!(windows) { "gh.exe" } else { "gh" };
    std::env::split_paths(&std::env::var_os("PATH")?).map(|dir| dir.join(name)).find(|path| path.is_file())
}

/// Asks the GitHub CLI at `gh` for its token: only when the user pressed *Connect Suhina to your
/// gh*.
pub fn gh_token(gh: &Path) -> Result<String, String> {
    let mut command = command(&gh.to_string_lossy());
    command.args(["auth", "token"]).stdout(Stdio::piped()).stderr(Stdio::piped());
    let output = command.output().map_err(|e| format!("could not run gh: {e}"))?;
    let token = String::from_utf8_lossy(&output.stdout).trim().to_string();
    if output.status.success() && !token.is_empty() && !token.contains(char::is_whitespace) {
        Ok(token)
    } else {
        Err("gh is not signed in: run gh auth login, then connect again".to_string())
    }
}
