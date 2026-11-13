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
    let (audio, audio_warnings, found) = audio_snapshot(session).complete_found(&project);
    warnings.extend(audio_warnings);
    // With where each file decoded now was read from, so a save uses that file and not one of
    // its name that turns up in the folder later (#617).
    session.set_audio_found(audio, found);
    match projects.open_output(Some(id)) {
        Ok(notice) => {
            warnings.extend(notice);
            let described = projects.get(id).and_then(Session::player).and_then(Player::output_info);
            json!(described.map(|i| i.describe()))
        }
        Err(e) => json!(format!("not available: {e}")),
    }
}

/// What the audio a project (the session's, or a scene of it) plays needs from the session,
/// taken quickly so that decoding the files it lacks can run off the window's thread: its files
/// are decoded from where the session's project file is, with what the session has decoded
/// already (files added since it opened, ADR 0011) for the rest.
pub(super) struct AudioSnapshot {
    folder: Option<PathBuf>,
    have: suhina_media::AudioPool,
    /// What the session trusts outside its folder (#764 › 3).
    trusted: suhina_media::Trusted,
}

pub(super) fn audio_snapshot(session: &Session) -> AudioSnapshot {
    let folder = session
        .audio_folder()
        .map(Path::to_path_buf)
        .or_else(|| session.source().and_then(|s| s.parent()).map(Path::to_path_buf));
    AudioSnapshot { folder, have: session.audio().clone(), trusted: session.trusted().clone() }
}

impl AudioSnapshot {
    /// The pool `project` plays, decoding the files the session has not, and the warnings.
    pub(super) fn complete(self, project: &Project) -> (suhina_media::AudioPool, Vec<String>) {
        let (pool, warnings, _) = self.complete_found(project);
        (pool, warnings)
    }

    /// [`AudioSnapshot::complete`], with the file each name decoded here was read from: only the
    /// names it adds, not the ones the session had decoded already.
    pub(super) fn complete_found(
        self,
        project: &Project,
    ) -> (suhina_media::AudioPool, Vec<String>, std::collections::HashMap<String, PathBuf>) {
        let AudioSnapshot { folder, mut have, trusted } = self;
        let mut found = std::collections::HashMap::new();
        let missing = project
            .contents
            .iter()
            .flat_map(|c| &c.lanes)
            .any(|lane| matches!(&lane.data, LaneData::Audio(region) if !have.files.contains_key(&region.file)));
        let mut warnings = Vec::new();
        if missing {
            // Trusted as the session trusts it, so a render reads what the project plays.
            let folder = folder.as_deref().unwrap_or(Path::new("."));
            let decoded = suhina_media::decode_project_audio_found(project, folder, &trusted);
            for (key, data) in decoded.pool.files {
                if let std::collections::hash_map::Entry::Vacant(slot) = have.files.entry(key.clone()) {
                    slot.insert(data);
                    if let Some(file) = decoded.found.get(&key) {
                        found.insert(key, file.clone());
                    }
                }
            }
            warnings = decoded.warnings;
        }
        (have, warnings, found)
    }
}

/// After a project command, starts the newly active project playing when the server has audio
/// output, since switching closes the previous project's player. Returns what was started.
pub(super) fn play_active(projects: &mut Projects, id: &str, output: bool) -> Option<Value> {
    if !output {
        return None;
    }
    if !project_commands().iter().any(|c| c.id == id && c.kind.target == suhina_session::Target::Projects) {
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
