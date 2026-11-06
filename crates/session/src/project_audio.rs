// Copyright (C) 2026 The Suhina contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! [`ProjectAudio`]: a project's decoded audio and where it may be read from (ADR 0011).

use std::path::PathBuf;
use std::sync::Arc;

use suhina_engine::{AudioData, AudioPool};

/// The audio a session's project plays, decoded.
#[derive(Default)]
pub(crate) struct ProjectAudio {
    /// The project's decoded audio, by the key its regions name.
    pub(crate) pool: AudioPool,
    /// Files added to `pool` since the player last got it.
    pub(crate) changed: bool,
    /// Files decoded ahead of a command that uses them (`provide_audio`), newest last.
    pub(crate) decoded: Vec<(String, Arc<AudioData>)>,
    /// The audio outside the project's folder it may read: the person's own saved files and
    /// library folders (`docs/spec/missing-files.md` › Project file boundaries).
    pub(crate) trusted: suhina_media::Trusted,
    /// The folder the project's relative audio paths start from: the folder of the file it was
    /// opened from (a template's, for a project made from one). It stays when the project is
    /// saved elsewhere, because the decoded audio is kept under those paths; saving writes them
    /// relative to the new file.
    pub(crate) folder: Option<PathBuf>,
}
