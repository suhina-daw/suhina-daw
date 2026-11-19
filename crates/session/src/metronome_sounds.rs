// Copyright (C) 2026 The Suhina contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! The metronome's sounds (`docs/spec/metronome.md` › Sounds): the families built into Suhina
//! and the library's, each folder of `Sounds/Metronome/` one family and each sound file in it one
//! of its sounds. A library sound's id is `library/<folder>/<file>`; it is read only from that
//! folder, never from a path that leaves it.

use std::path::{Path, PathBuf};
use std::sync::Arc;

use suhina_engine::{Click, ClickFamily};

use crate::Session;

/// The prefix of a library sound's id.
pub(crate) const LIBRARY: &str = "library/";
/// The most families, and sounds in one family, the library lists.
const MAX_FAMILIES: usize = 64;
const MAX_SOUNDS: usize = 64;
/// The largest sound file read for a click: a click is at most two seconds long.
const MAX_FILE_BYTES: u64 = 16 * 1024 * 1024;

/// The metronome as `transport.setMetronome` left it, with the sounds it can pick from.
#[derive(Debug, Clone, PartialEq)]
pub struct MetronomeState {
    pub on: bool,
    /// Its level in dB.
    pub db: f64,
    /// The id of the sound every beat plays.
    pub sound: String,
    /// The id of the sound a bar's first beat plays; `None` for the beat's sound, higher.
    pub accent: Option<String>,
    /// The families it picks from: the built-in ones, then the library's.
    pub families: Arc<Vec<ClickFamily>>,
}

impl Default for MetronomeState {
    /// Off, at -6 dB, the beep, higher on the bar, with the built-in sounds.
    fn default() -> Self {
        Self {
            on: false,
            db: -6.0,
            sound: "beep".to_string(),
            accent: None,
            families: Arc::new(suhina_engine::built_in_click_families()),
        }
    }
}

/// `Sounds/Metronome/` under the library's root.
pub fn library_folder(base: &Path) -> PathBuf {
    base.join("Sounds").join("Metronome")
}

/// The built-in families, then those in `base`'s `Sounds/Metronome/` when there is a base: its
/// folders in name order, each with its sound files in name order; an empty folder, a hidden one
/// and a name that is not text are left out.
pub(crate) fn families(base: Option<&Path>) -> Vec<ClickFamily> {
    let mut all = suhina_engine::built_in_click_families();
    if let Some(base) = base {
        all.extend(library_families(&library_folder(base)));
    }
    all
}

/// The names in `folder` that are folders (`dirs`) or sound files, hidden ones left out, sorted
/// without regard to case. Links are not followed.
fn names(folder: &Path, dirs: bool, most: usize) -> Vec<String> {
    let Ok(entries) = std::fs::read_dir(folder) else { return Vec::new() };
    let mut found: Vec<String> = entries
        .flatten()
        .filter(|e| e.file_type().is_ok_and(|t| if dirs { t.is_dir() } else { t.is_file() }))
        .filter_map(|e| e.file_name().into_string().ok())
        .filter(|name| plain(name))
        .filter(|name| dirs || is_sound(Path::new(name)))
        .take(most.saturating_mul(4))
        .collect();
    found.sort_by_key(|name| name.to_lowercase());
    found.truncate(most);
    found
}

fn is_sound(path: &Path) -> bool {
    suhina_media::info::kind_of(path) == Some(suhina_media::info::FileKind::Sample)
}

fn library_families(folder: &Path) -> Vec<ClickFamily> {
    names(folder, true, MAX_FAMILIES)
        .into_iter()
        .filter_map(|family| {
            let sounds: Vec<(String, String)> = names(&folder.join(&family), false, MAX_SOUNDS)
                .into_iter()
                .map(|file| {
                    let name = Path::new(&file).file_stem().and_then(|s| s.to_str()).unwrap_or(&file).to_string();
                    (format!("{LIBRARY}{family}/{file}"), name)
                })
                .collect();
            (!sounds.is_empty()).then(|| ClickFamily { name: family.clone(), folder: Some(family), sounds })
        })
        .collect()
}

/// A plain name: not empty, no separator, not `.` or `..` and not hidden.
fn plain(name: &str) -> bool {
    !name.is_empty() && !name.contains(['/', '\\', ':', '\0']) && !name.starts_with('.')
}

/// The click `id` names: a built-in one, or a sound file of the library's
/// (`library/<folder>/<file>`), read and decoded now.
pub(crate) fn click(base: Option<&Path>, id: &str) -> Result<Click, String> {
    if let Some(click) = Click::built_in(id) {
        return Ok(click);
    }
    let unknown = || format!("no metronome sound {id:?}: transport.metronomeSounds lists them");
    let rest = id.strip_prefix(LIBRARY).ok_or_else(unknown)?;
    let (family, file) = rest.split_once('/').ok_or_else(unknown)?;
    if !plain(family) || !plain(file) || !is_sound(Path::new(file)) {
        return Err(unknown());
    }
    let base = base.ok_or("there is no library to read the sound from")?;
    let folder = library_folder(base);
    // Use the same containment and link checks as project audio, including the family folder.
    let relative = format!("{family}/{file}");
    let path = suhina_media::locate(&relative, None, &folder, &suhina_media::Trusted::default()).ok_or_else(unknown)?;
    let meta = std::fs::symlink_metadata(&path).map_err(|_| unknown())?;
    if !meta.is_file() {
        return Err(unknown());
    }
    if meta.len() > MAX_FILE_BYTES {
        return Err(format!("{id:?} is larger than {} MB; a click is at most two seconds", MAX_FILE_BYTES >> 20));
    }
    let data = suhina_media::decode_file_prefix(&path, suhina_engine::MAX_CLICK_SECONDS)
        .map_err(|e| format!("{id:?} could not be read: {e}"))?;
    Click::from_audio(&data).ok_or_else(|| format!("{id:?} holds no sound"))
}

impl Session {
    /// The metronome as `transport.setMetronome` left it, with the sounds it can pick from as
    /// they were last looked for ([`Session::find_metronome_sounds`]).
    pub fn metronome(&self) -> MetronomeState {
        let (on, db, ref sound) = self.live.metronome;
        MetronomeState {
            on,
            db,
            sound: sound.clone(),
            accent: self.live.accent.clone(),
            families: Arc::clone(&self.live.click_families),
        }
    }

    /// Looks for the library's metronome sounds again, as the metronome's menu opens.
    pub fn find_metronome_sounds(&mut self) -> &[ClickFamily] {
        self.live.click_families = Arc::new(families(self.library_base()));
        &self.live.click_families
    }
}
