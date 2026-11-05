// Copyright (C) 2026 The Suhina contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! What the MCP server plays (#764 › 12): a project's audio decoded and its output opened,
//! and the project a command switched to started playing.

use super::*;

/// Starts project `id` playing on the chosen output. Returns the device description, or why it
/// is not available; missing audio files and a fallback to the default device are added to
/// `warnings`.
pub(super) fn start_audio(projects: &mut Projects, id: ProjectId, warnings: &mut Vec<String>) -> Value {
    let Ok(session) = projects.session_mut(Some(id)) else { return Value::Null };
    let project = session.project().clone();
    let (audio, audio_warnings) = project_audio(session, &project);
    warnings.extend(audio_warnings);
    session.set_audio(audio);
    match projects.open_output(Some(id)) {
        Ok(notice) => {
            warnings.extend(notice);
            let described = projects.get(id).and_then(Session::player).and_then(Player::output_info);
            json!(described.map(|i| i.describe()))
        }
        Err(e) => json!(format!("not available: {e}")),
    }
}

/// The audio `project` (the session's, or a scene of it) plays: its files decoded from where
/// the session's project file is, with what the session has decoded already (files added since
/// it opened, ADR 0011) for the rest.
pub(super) fn project_audio(session: &Session, project: &Project) -> (suhina_media::AudioPool, Vec<String>) {
    audio_snapshot(session).complete(project)
}

/// What [`project_audio`] needs from the session, taken quickly so that decoding the files it
/// lacks can run off the window's thread.
pub(super) struct AudioSnapshot {
    folder: Option<PathBuf>,
    have: suhina_media::AudioPool,
}

pub(super) fn audio_snapshot(session: &Session) -> AudioSnapshot {
    let folder = session
        .audio_folder()
        .map(Path::to_path_buf)
        .or_else(|| session.source().and_then(|s| s.parent()).map(Path::to_path_buf));
    AudioSnapshot { folder, have: session.audio().clone() }
}

impl AudioSnapshot {
    /// The pool `project` plays, decoding the files the session has not, and the warnings.
    pub(super) fn complete(self, project: &Project) -> (suhina_media::AudioPool, Vec<String>) {
        let AudioSnapshot { folder, mut have } = self;
        let missing = project
            .contents
            .iter()
            .flat_map(|c| &c.lanes)
            .any(|lane| matches!(&lane.data, LaneData::Audio(region) if !have.files.contains_key(&region.file)));
        let mut warnings = Vec::new();
        if missing {
            let (loaded, found) =
                suhina_media::load_project_audio(project, folder.as_deref().unwrap_or(Path::new(".")));
            for (key, data) in loaded.files {
                have.files.entry(key).or_insert(data);
            }
            warnings = found;
        }
        (have, warnings)
    }
}

/// After a project command, starts the newly active project playing when the server has audio
/// output, since switching closes the previous project's player. Returns what was started.
pub(super) fn play_active(projects: &mut Projects, id: &str, output: bool) -> Option<Value> {
    if !output {
        return None;
    }
    if !project_commands().iter().any(|c| c.id == id && c.kind == CommandKind::Projects) {
        return None;
    }
    let active = projects.active()?;
    if projects.get(active)?.player().is_some() {
        return None;
    }
    let mut warnings = Vec::new();
    let started = start_audio(projects, active, &mut warnings);
    Some(json!({ "device": started, "warnings": warnings }))
}
