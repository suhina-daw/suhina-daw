// Copyright (C) 2026 The Suhina contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! `suhina-cli audio`: the audio devices.

use crate::*;

/// `plugins …`: runs the matching `plugin.*` command and prints its result.
/// `audio devices`: runs `audio.devices` on the system's devices.
pub(crate) fn audio_devices(args: &[String]) -> Result<(), String> {
    if args.iter().map(String::as_str).ne(["devices"]) {
        return Err(format!("unknown audio command\n\n{USAGE}"));
    }
    let mut projects = suhina_session::Projects::new();
    projects.set_audio_system(std::sync::Arc::new(suhina_output::DeviceOutput::new()));
    let result = projects.execute("audio.devices", &serde_json::json!({})).map_err(|e| e.message().to_owned())?;
    println!("{}", serde_json::to_string_pretty(&result).map_err(|e| e.to_string())?);
    Ok(())
}
