// Copyright (C) 2026 The Suhina contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! Agent-facing automation: an MCP server (the official `rmcp` SDK) over a Suhina session,
//! file access limited to granted roots, the offline WAV render shared with the CLI, and the
//! desktop app's endpoints, through which agents work on its open projects (ADR 0028).

pub mod attach;
pub mod endpoint;
pub mod render;
pub mod server;
pub mod workspace;

pub use attach::{AppEndpoints, AppEvent, Running, StartError, Wake};
pub use render::{LaunchStep, RenderSummary, render_launch_script, render_wav, render_wavs, scene_as_arrangement};
pub use server::SuhinaMcp;
pub use workspace::{Workspace, display_path};
