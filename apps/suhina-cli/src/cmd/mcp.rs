// Copyright (C) 2026 The Suhina contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! `suhina-cli mcp`: the MCP server over standard input and output.

use crate::*;

/// Serves MCP on stdin/stdout. Nothing else may be printed to stdout while it runs.
pub(crate) fn mcp(args: &[String]) -> Result<(), String> {
    let mut read_root = None;
    let mut write_root = None;
    let mut audio = false;
    let mut attach = false;
    let mut it = args.iter();
    while let Some(arg) = it.next() {
        match arg.as_str() {
            "--read-root" => read_root = Some(PathBuf::from(it.next().ok_or("--read-root needs a folder")?)),
            "--write-root" => write_root = Some(PathBuf::from(it.next().ok_or("--write-root needs a folder")?)),
            "--audio" => audio = true,
            "--attach" => attach = true,
            other => return Err(format!("unknown argument: {other}\n\n{USAGE}")),
        }
    }
    if attach {
        if read_root.is_some() || write_root.is_some() || audio {
            return Err("--attach takes no other arguments: the app decides what an agent may read and write, \
                        and plays on its own output"
                .into());
        }
        let folder = suhina_automation::endpoint::build_folder()
            .map_err(|e| format!("could not find this program's folder: {e}"))?;
        let runtime = tokio::runtime::Builder::new_multi_thread()
            .enable_all()
            .build()
            .map_err(|e| format!("could not start the async runtime: {e}"))?;
        return runtime.block_on(suhina_automation::attach::attach_stdio(&folder));
    }
    let workspace = Workspace::new(read_root.as_deref(), write_root.as_deref())?;
    let library = suhina_library::Library::new(suhina_plugins::library::settings_folder());
    let mut server = SuhinaMcp::new(workspace).with_file_library(library);
    if audio {
        server = server
            .with_audio_output(std::sync::Arc::new(suhina_output::DeviceOutput::new()))
            .with_midi_input(std::sync::Arc::new(suhina_midi::Midi::new()));
    }
    let runtime = tokio::runtime::Builder::new_multi_thread()
        .enable_all()
        .build()
        .map_err(|e| format!("could not start the async runtime: {e}"))?;
    runtime.block_on(server.serve_stdio())
}
