// Copyright (C) 2026 The Suhina contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! A Suhina session: an open project, where it came from, and its undo history. Every change
//! goes through a command (see `docs/decisions/0002-commands.md`), so the CLI, the MCP server and
//! later the UI all drive the project the same way.

mod ara;
pub mod audio;
mod automation;
pub mod backup;
mod blank;
mod clip_color;
mod clip_edges;
mod clip_shape;
mod clipboards;
mod commands;
mod device_files;
mod device_groups;
mod devices;
mod editing;
mod execute;
mod export;
mod files;
mod follow;
mod history;
mod launch;
mod library;
mod live_input;
mod mapping;
mod meters;
mod midi;
mod note_tools;
mod notes;
mod player;
mod player_sync;
mod plugin_edits;
mod plugins;
mod preview;
mod project_audio;
mod projects;
mod rack;
mod range;
mod record_writer;
mod recording;
mod relink;
mod scales;
mod sends;
mod soundmap;
mod tempo;

use std::collections::HashMap;
use std::fmt;
use std::path::{Path, PathBuf};

use suhina_engine::Quantization;
use suhina_model::Project;

pub use ara::ClipNotes;
pub use audio::{AudioSystem, InputChoice, LiveOutput, OutputChoice, OutputInfo, audio_commands};
pub use blank::blank_project;
pub use commands::{
    Agents, CommandKind, CommandSpec, Param, ParamType, PathKind, Policy, Run, commands, parse_quantization,
};
pub use export::{ExportData, ExportGather, export_from};
pub use files::{MAX_FILES, file_tempo};
pub use library::{Category, Found, file_info, library_commands, search_items};
pub use mapping::{MAX_MAPPINGS, mapped_command, target_exists};
pub use midi::{LiveInput, MidiSystem, midi_commands};
pub use notes::{MAX_NOTES_PER_CLIP, MAX_NOTES_PER_COMMAND, midi_velocity};
pub use player::{OutputHandle, Player};
pub use plugins::{device_plugin, plugin_commands};
pub use projects::{
    MAX_OPEN_PROJECTS, ProjectId, Projects, all_commands, commands_markdown, params_of, policy, project_commands,
};
pub use rack::{
    BlockCounts, Hosted, OfflinePlugins, PluginEnv, PluginProgress, PluginReport, PluginStatus, ProcessGroup,
    is_instrument, plugin_group, start_offline,
};
pub use recording::{COUNT_IN_BARS, RECORDED_FOLDER};
pub use relink::COLLECTED;
pub use relink::{MAX_RELINK, search as find_candidates};
pub use soundmap::{scope_files, sound_map_commands};
pub use suhina_plugins::Placement;
pub use suhina_project_file::{EXTENSION as PROJECT_EXTENSION, is_project_file};

/// Undo steps kept per session; the oldest are dropped first.
const MAX_UNDO: usize = 256;

/// Why a command did not run. The session is unchanged when this is returned. Its text is read
/// with [`CommandError::message`] or taken with `String::from` (#764 › 12).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CommandError(String);

impl CommandError {
    /// What went wrong, as a person reads it.
    pub fn message(&self) -> &str {
        &self.0
    }
}

impl From<CommandError> for String {
    fn from(error: CommandError) -> Self {
        error.0
    }
}

impl fmt::Display for CommandError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

impl std::error::Error for CommandError {}

impl From<String> for CommandError {
    fn from(message: String) -> Self {
        Self(message)
    }
}

impl From<&str> for CommandError {
    fn from(message: &str) -> Self {
        Self(message.to_string())
    }
}

impl From<suhina_model::IdsExhausted> for CommandError {
    fn from(e: suhina_model::IdsExhausted) -> Self {
        Self(e.to_string())
    }
}

/// An edit that is only a device parameter's new value, which a playing engine can take
/// without a new graph.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct ParamEdit {
    pub track: suhina_model::TrackId,
    /// The device's index among the track channel's devices.
    pub device: usize,
    /// The parameter's index in the device's info.
    pub param: usize,
    pub value: f64,
}

pub struct Session {
    project: Project,
    source: Option<PathBuf>,
    history: history::History,
    clipboards: clipboards::Clipboards,
    /// The colours a track without one of its own shows, by its place among the tracks (the
    /// window's palette), and the one a new track gets instead, when set ([`Session::set_track_colours`]).
    track_colours: (Vec<suhina_model::Rgb>, Option<suhina_model::Rgb>),
    /// The project's plug-ins, running while it plays (ADR 0006).
    pub(crate) plugins: rack::Rack,
    audio: project_audio::ProjectAudio,
    device_files: device_files::DeviceFiles,
    pub(crate) sync: player_sync::PlayerSync,
    /// When the project was last backed up and where (`backup.rs`).
    pub(crate) backup: backup::SessionBackups,
    pub(crate) live: live_input::LiveInput,
}

/// Files kept decoded ahead of the commands that use them.
const MAX_DECODED: usize = 16;

impl Session {
    /// A session on `project`. Its lanes, clips and scenes without an id get one, so commands can
    /// name them (ADR 0025).
    pub fn new(mut project: Project) -> Self {
        // A project that has used every id there is still opens; only adding objects then fails.
        let _ = project.assign_ids();
        Self {
            project,
            source: None,
            history: history::History::default(),
            clipboards: clipboards::Clipboards::default(),
            track_colours: (Vec::new(), None),
            plugins: rack::Rack::default(),
            audio: project_audio::ProjectAudio::default(),
            device_files: device_files::DeviceFiles::default(),
            sync: player_sync::PlayerSync::default(),
            backup: backup::SessionBackups::default(),
            live: live_input::LiveInput::default(),
        }
    }

    /// Whether the clips copied for `clip.paste` play audio: `Some(true)` for audio, `Some(false)`
    /// for notes, `None` when nothing has been copied. The window uses it to pick the stacking
    /// setting a paste follows.
    pub fn copied_clip_is_audio(&self) -> Option<bool> {
        let (_, content) = self.clipboards.clips.first()?;
        Some(content.lanes.iter().any(|l| matches!(l.data, suhina_model::LaneData::Audio(_))))
    }

    /// How many notes `note.paste` would add; `None` when no notes have been copied. The piano
    /// roll uses it to select the pasted notes.
    pub fn copied_notes(&self) -> Option<usize> {
        self.clipboards.notes.as_ref().map(notes::NoteClipboard::len)
    }

    /// How many arrangement clips are copied for `clip.paste`.
    pub fn copied_clips(&self) -> usize {
        self.clipboards.clips.len()
    }

    /// Whether the copied clips came from more than one track, so a paste puts each back on its
    /// own track rather than all on one.
    /// How many devices `device.paste` would add.
    pub fn copied_devices(&self) -> usize {
        self.clipboards.devices.len()
    }

    pub fn copied_from_tracks(&self) -> bool {
        self.clipboards.tracks.len() == self.clipboards.clips.len()
            && self.clipboards.tracks.iter().any(|t| Some(t) != self.clipboards.tracks.first())
    }

    /// The project's decoded audio, which players are started with (ADR 0011).
    pub fn audio(&self) -> &suhina_engine::AudioPool {
        &self.audio.pool
    }

    /// The audio outside the project's folder that it may read (› Project file boundaries in
    /// `docs/spec/missing-files.md`); the app gives it before the audio is decoded.
    pub fn set_trusted(&mut self, trusted: suhina_media::Trusted) {
        self.audio.trusted = trusted;
    }

    pub fn trusted(&self) -> &suhina_media::Trusted {
        &self.audio.trusted
    }

    /// The absolute paths of the audio files the project names that are decoded now: what the
    /// person loaded or relinked, or what was trusted when it opened. Saving keeps them as
    /// trusted, so the project opens with them again.
    pub fn loaded_audio_paths(&self) -> Vec<PathBuf> {
        let mut out: Vec<PathBuf> = Vec::new();
        for (_, path) in self.loaded_audio() {
            if !out.contains(&path) {
                out.push(path);
            }
        }
        out
    }

    /// [`Session::loaded_audio_paths`] with the name the clips and devices give each (the key
    /// of its decoded audio), each name once.
    pub(crate) fn loaded_audio(&self) -> Vec<(String, PathBuf)> {
        let loaded = |file: &str| {
            self.audio.pool.files.contains_key(file)
                || self.audio.pool.files.contains_key(&suhina_model::stream_key(file, 0))
        };
        let from = self.audio_from();
        let path = |file: &str, absolute: Option<&str>| {
            absolute.map(PathBuf::from).filter(|p| p.is_absolute()).unwrap_or_else(|| from.join(file))
        };
        let mut out: Vec<(String, PathBuf)> = Vec::new();
        let devices = self.project.channels.iter().flat_map(|c| &c.devices);
        let named = devices.filter_map(|d| d.sample_file().map(|f| (f, d.sample_absolute_file()))).chain(
            self.project.contents.iter().flat_map(|c| &c.lanes).filter_map(|l| match &l.data {
                suhina_model::LaneData::Audio(region) if !region.file.is_empty() => {
                    Some((region.file.as_str(), region.absolute_file.as_deref()))
                }
                _ => None,
            }),
        );
        for (file, absolute) in named {
            if loaded(file) && !out.iter().any(|(f, _)| f == file) {
                let path = path(file, absolute);
                if path.is_absolute() {
                    out.push((file.to_string(), path));
                }
            }
        }
        out
    }

    /// Gives the session its project's decoded audio (`media::load_project_audio`), before a
    /// player is started with [`Session::audio`].
    pub fn set_audio(&mut self, audio: suhina_engine::AudioPool) {
        self.audio.pool = audio;
        self.audio.changed = true;
    }

    /// The parsed model or decoded IR for a file-backed device's file, from the assets already
    /// held, or `None` while a load failed or is still to run
    /// ([docs/spec/nam-amp-and-cab.md]).
    pub fn device_asset(&self, path: &str) -> Option<&suhina_device_registry::DeviceAsset> {
        self.device_files.assets.get(path)
    }

    /// What the session's NAM devices run, by path, for a renderer's construction (a `render`
    /// through `render_offline_from`) and the app's output open.
    pub fn device_assets(&self) -> &suhina_engine::DeviceAssets {
        &self.device_files.assets
    }

    /// What a device's file is called, for a device panel: the model's own name (its metadata)
    /// for a NAM Amp Modeler, the IR's parent folder's name for a NAM Cab Loader. `None` while
    /// the file is not in the assets.
    pub fn device_asset_name(&self, path: &str) -> Option<String> {
        match self.device_files.assets.get(path)? {
            suhina_device_registry::DeviceAsset::Nam(model) => {
                suhina_device_nam_amp_modeler::model::display_name(model)
            }
            suhina_device_registry::DeviceAsset::Ir(_) => std::path::Path::new(path)
                .parent()
                .and_then(|folder| folder.file_name())
                .map(|folder| folder.to_string_lossy().into_owned()),
        }
    }

    /// Why a device's file is not in the assets, by the path: what a read gave, for a device
    /// panel's note.
    pub fn device_asset_note(&self, path: &str) -> Option<&str> {
        self.device_files.notes.get(path).map(String::as_str)
    }

    /// Puts audio made by a command (a consolidated range) into the pool under `key`, for the
    /// player to hear with the next edit.
    pub(crate) fn add_audio(&mut self, key: String, data: suhina_engine::AudioData) {
        self.audio.pool.files.insert(key, std::sync::Arc::new(data));
        self.audio.changed = true;
    }

    /// Hands over a file decoded ahead (on another thread), so a command that uses `path` next
    /// (`clip.addFiles`, `preview.play`) need not decode it. Only the latest few are kept.
    pub fn provide_audio(&mut self, path: &Path, data: std::sync::Arc<suhina_engine::AudioData>) {
        self.provide_key(files::absolute(path).display().to_string(), data);
    }

    /// Hands over one audio track of a Stems file decoded ahead (track 0 is the master), as
    /// [`Session::provide_audio`] does for a sample.
    pub fn provide_stream(&mut self, path: &Path, stream: u16, data: std::sync::Arc<suhina_engine::AudioData>) {
        self.provide_key(suhina_model::stream_key(&files::absolute(path).display().to_string(), stream), data);
    }

    fn provide_key(&mut self, key: String, data: std::sync::Arc<suhina_engine::AudioData>) {
        self.audio.decoded.retain(|(k, _)| *k != key);
        if self.audio.decoded.len() >= MAX_DECODED {
            self.audio.decoded.remove(0);
        }
        self.audio.decoded.push((key, data));
    }

    /// The decoded audio of the file at `path` (absolute): from the pool, from what was
    /// provided, or decoded now.
    pub(crate) fn decoded_audio(&mut self, path: &Path) -> Result<std::sync::Arc<suhina_engine::AudioData>, String> {
        let key = path.display().to_string();
        let data = if let Some(data) = self.audio.pool.files.get(&key) {
            return Ok(std::sync::Arc::clone(data));
        } else if let Some((_, data)) = self.audio.decoded.iter().find(|(k, _)| *k == key) {
            std::sync::Arc::clone(data)
        } else {
            std::sync::Arc::new(suhina_media::decode_file(path)?)
        };
        self.audio.pool.files.insert(key, std::sync::Arc::clone(&data));
        self.audio.changed = true;
        Ok(data)
    }

    /// Every audio track of the Stems file at `path` (absolute), the master first: from the
    /// pool, from what was provided, or decoded now. Each joins the pool under its
    /// [`suhina_model::stream_key`].
    pub(crate) fn decoded_streams(
        &mut self,
        path: &Path,
        tracks: usize,
    ) -> Result<Vec<std::sync::Arc<suhina_engine::AudioData>>, String> {
        let file = path.display().to_string();
        let keys: Vec<String> =
            (0..tracks).map(|n| suhina_model::stream_key(&file, u16::try_from(n).unwrap_or(u16::MAX))).collect();
        let known: Vec<Option<std::sync::Arc<suhina_engine::AudioData>>> = keys
            .iter()
            .map(|key| {
                self.audio
                    .pool
                    .files
                    .get(key)
                    .or_else(|| self.audio.decoded.iter().find(|(k, _)| k == key).map(|(_, d)| d))
                    .map(std::sync::Arc::clone)
            })
            .collect();
        let all: Vec<std::sync::Arc<suhina_engine::AudioData>> = if known.iter().all(Option::is_some) {
            known.into_iter().flatten().collect()
        } else {
            suhina_media::stems::decode_streams(path)?.into_iter().map(std::sync::Arc::new).collect()
        };
        if all.len() != tracks {
            return Err(format!("{} audio tracks where {tracks} were expected", all.len()));
        }
        for (key, data) in keys.into_iter().zip(&all) {
            if let std::collections::hash_map::Entry::Vacant(slot) = self.audio.pool.files.entry(key) {
                slot.insert(std::sync::Arc::clone(data));
                self.audio.changed = true;
            }
        }
        Ok(all)
    }

    /// Decoded audio for a preview, which does not join the project's pool.
    pub(crate) fn preview_audio(&mut self, path: &Path) -> Result<std::sync::Arc<suhina_engine::AudioData>, String> {
        let key = path.display().to_string();
        if let Some(data) = self.audio.pool.files.get(&key) {
            return Ok(std::sync::Arc::clone(data));
        }
        if let Some((_, data)) = self.audio.decoded.iter().find(|(k, _)| *k == key) {
            return Ok(std::sync::Arc::clone(data));
        }
        let data = std::sync::Arc::new(suhina_media::decode_file(path)?);
        self.provide_audio(path, std::sync::Arc::clone(&data));
        Ok(data)
    }

    /// Connects an audio engine playing this session's project; transport and launcher commands
    /// go to it from now on. Replaces (and so closes) any previous player. It should have been
    /// started with [`Session::audio`]; audio added later reaches it with the next edit.
    pub fn attach_player(&mut self, mut player: Player) {
        // A new engine starts on its own default grid; it takes this session's instead. Its
        // command queue is empty, so this send only fails if the engine is gone, and then every
        // later command says so.
        let _ = player.send(suhina_engine::LaunchCommand::SetQuantization(self.live.quantization));
        // Its metronome too: on or off, level and sound, as this session has them.
        let (on, db, sound) = self.live.metronome;
        let gain = if db.is_finite() { 10f64.powf(db / 20.0) as f32 } else { 0.0 };
        let _ = player.send(suhina_engine::LaunchCommand::SetMetronome { on, gain: Some(gain) });
        let _ = player.send(suhina_engine::LaunchCommand::SetMetronomeSound(sound));
        // Takes of the engine this one replaces end where it was; a new engine records anew.
        self.end_takes_now();
        let sample_rate = player.sample_rate();
        self.live.recording.attach(player.take_recorder(), sample_rate);
        self.sync.player = Some(player);
        self.audio.changed = false;
        // A new engine hears the tracks armed before it.
        self.send_armed();
    }

    /// Synchronizes recording and monitoring targets after a new player, arm, edit or undo.
    /// Previous targets are included so switching In off, or deleting a track, clears it too.
    fn send_armed(&mut self) {
        let armed = self.armed();
        let active: Vec<_> = self
            .project
            .tracks
            .iter()
            .filter(|t| armed.contains(&t.id) || (t.content.notes && t.midi_input.monitor == suhina_model::Monitor::In))
            .map(|t| t.id)
            .collect();
        let mut targets = std::mem::replace(&mut self.live.targets, active.clone());
        for track in &active {
            if !targets.contains(track) {
                targets.push(*track);
            }
        }
        if let Some(player) = self.sync.player.as_mut() {
            for track in targets {
                let is_armed = armed.contains(&track);
                let monitoring = self.project.track(track).is_some_and(|t| {
                    if !t.content.notes {
                        return is_armed;
                    }
                    match t.midi_input.monitor {
                        suhina_model::Monitor::Auto => is_armed,
                        suhina_model::Monitor::In => true,
                        suhina_model::Monitor::Off => false,
                    }
                });
                let input = live_filter(&self.project, track);
                // A full queue means an engine that does not run; it has nothing to arm.
                let _ = player.send(suhina_engine::LaunchCommand::SetMidiInput {
                    track,
                    armed: is_armed,
                    monitoring,
                    input,
                });
            }
        }
    }

    /// The armed tracks that are in the project, in the order they were armed.
    pub fn armed(&self) -> Vec<suhina_model::TrackId> {
        self.live.armed.iter().copied().filter(|id| self.project.tracks.iter().any(|t| t.id == *id)).collect()
    }

    pub fn is_armed(&self, track: suhina_model::TrackId) -> bool {
        self.live.armed.contains(&track) && self.project.tracks.iter().any(|t| t.id == track)
    }

    /// Arms `track` or disarms it; `exclusive` also disarms every other track. The playing
    /// engine hears it from its next block.
    pub(crate) fn set_armed(&mut self, track: suhina_model::TrackId, armed: bool, exclusive: bool) {
        // Tracks removed since they were armed are forgotten.
        let tracks = &self.project.tracks;
        self.live.armed.retain(|id| *id != track && !exclusive && tracks.iter().any(|t| t.id == *id));
        if armed {
            self.live.armed.push(track);
        }
        self.send_armed();
    }

    /// The sending end of the playing engine's live-note queue, the first time it is asked for.
    pub(crate) fn take_live_input(&mut self) -> Option<suhina_engine::LiveSender> {
        self.sync.player.as_mut().and_then(Player::take_live_input)
    }

    /// The launch quantisation `launcher.setQuantization` set last (1 bar until then); the
    /// Arrangement view's grid follows it too.
    pub fn quantization(&self) -> Quantization {
        self.live.quantization
    }

    pub(crate) fn set_quantization(&mut self, quantization: Quantization) -> Result<(), CommandError> {
        if let Some(player) = self.sync.player.as_mut() {
            player.send(suhina_engine::LaunchCommand::SetQuantization(quantization))?;
        }
        self.live.quantization = quantization;
        Ok(())
    }

    /// Replaces the player with the one `open` makes from the old one, keeping the project's
    /// plug-ins running (their processors move to the new engine when the output can carry
    /// them over; otherwise they start again). Without a player, `open` gets `None`. On an
    /// error the session has no player.
    pub fn replace_player<E>(
        &mut self,
        open: impl FnOnce(
            &Project,
            &suhina_engine::AudioPool,
            &suhina_engine::DeviceAssets,
            Option<Player>,
        ) -> Result<Player, E>,
    ) -> Result<(), E> {
        // A take ends where the engine it records from was (a lost device keeps what it had).
        self.end_takes_now();
        self.sync_device_files();
        let old = self.sync.player.take();
        match open(&self.project, &self.audio.pool, &self.device_files.assets, old) {
            Ok(player) => {
                self.attach_player(player);
                Ok(())
            }
            Err(e) => {
                self.plugins.stop_all();
                Err(e)
            }
        }
    }

    /// Stops the project's plug-ins so the next [`Session::sync_plugins`] starts them again
    /// (each with its last state): for a new player that has none of them.
    pub fn restart_plugins(&mut self) {
        self.plugins.stop_all();
    }

    /// Disconnects the audio engine; the project's plug-ins stop (each keeps its last state for
    /// when the project plays again).
    pub fn detach_player(&mut self) -> Option<Player> {
        self.plugins.stop_all();
        self.end_takes_now();
        self.sync.player.take()
    }

    /// Lets the session start the project's plug-ins: the catalog to find them in and how to
    /// host them (ADR 0006). Without it plug-in devices pass the sound through.
    pub fn set_plugin_env(&mut self, env: std::sync::Arc<PluginEnv>) {
        self.plugins.set_env(env);
    }

    /// Brings the project's running plug-ins in line with it (starts new ones in the
    /// background, stops removed ones, takes in started ones, notices crashes). Call it often
    /// from the session's thread, as the window does every frame. Returns what the user should
    /// hear about.
    pub fn sync_plugins(&mut self) -> Vec<String> {
        self.plugins.sync(&self.project, &self.audio.pool, self.sync.player.as_mut())
    }

    /// How often the state of each running plug-in is read for backups and the unsaved-changes
    /// check (every 30 seconds unless set). For tests, which need not wait that long.
    pub fn set_plugin_read_interval(&mut self, every: std::time::Duration) {
        self.plugins.set_save_every(every);
    }

    /// The plug-ins (by spec) found to edit clips over ARA since the last call although the
    /// catalog did not say so ([`Rack::take_ara_learned`](crate::rack::Rack::take_ara_learned)).
    pub fn take_ara_learned(&mut self) -> Vec<String> {
        self.plugins.take_ara_learned()
    }

    /// The window plug-in editors open over, owned by it (a Win32 `HWND`, an X11 window id):
    /// they float above it, have no taskbar entry of their own and go with it.
    pub fn set_editor_owner(&mut self, owner: Option<u64>) {
        self.plugins.set_owner(owner);
    }

    /// Whether plug-in editors open embedded in the owner window's device panel rather than
    /// in windows of their own (the window's setting; nothing without an owner window).
    pub fn set_editor_embedding(&mut self, embed: bool) {
        self.plugins.set_embedding(embed);
    }

    /// Where the window shows the embedded plug-in editors this frame, each by its device
    /// (`track`, index among the track's channel's devices) and in the owner window's own
    /// pixels; every other embedded editor is hidden. Never waits for a host; the window says
    /// it every frame.
    pub fn place_editors(&mut self, shown: &[(suhina_model::TrackId, usize, suhina_plugins::Placement)]) {
        let keyed: Vec<(u64, suhina_plugins::Placement)> = shown
            .iter()
            .filter_map(|(track, device, placement)| Some((plugin_key_at(&self.project, *track, *device)?, *placement)))
            .collect();
        self.plugins.place_editors(&keyed);
    }

    /// Tells the plug-ins on `track` that edit clips (ARA, ADR 0028) which of its arrangement
    /// clips is selected, so their editors show it; `None` selects none. Never waits for a host.
    pub fn select_ara_clip(&mut self, track: suhina_model::TrackId, clip: Option<suhina_model::ClipId>) {
        let Some(channel) = self.project.track(track).and_then(|t| t.channel) else {
            return;
        };
        let region = clip.map(|c| format!("clip-{}", c.0));
        for device in suhina_engine::plugin_devices(&self.project).into_iter().filter(|d| d.channel == channel) {
            self.plugins.select_ara(device.key, region.clone());
        }
    }

    /// The notes the ARA plug-in that edits `clip` found in it, for the arrangement to draw
    /// (#720): `None` when it has none, or its plug-in is bypassed or not running.
    pub fn clip_notes(&self, clip: suhina_model::ClipId) -> Option<&ClipNotes> {
        self.plugins.clip_notes(clip)
    }

    /// What an ARA plug-in on `track` is doing with its audio clips: its name, and how far its
    /// analysis has come, or `None` while it loads. `None` when it is doing neither.
    pub fn ara_activity(&self, track: suhina_model::TrackId) -> Option<(String, Option<f32>)> {
        self.plugins.ara_activity(track)
    }

    /// The first device on `track` that runs a plug-in bound to its audio clips (ARA), by its
    /// index in the track's chain.
    pub fn ara_device(&self, track: suhina_model::TrackId) -> Option<usize> {
        self.ara_tracks().into_iter().find(|(t, _)| *t == track).map(|(_, device)| device)
    }

    /// Whether clips' edits are still on their way to another track's plug-in (ADR 0028 step 6):
    /// a clip moved there with its edits kept, waiting for that plug-in to run with it.
    pub fn carrying_ara_edits(&self) -> bool {
        self.plugins.carrying()
    }

    /// Takes the editor of the plug-in device `device` on `track`, one that edits clips, out of
    /// the window to float in one of its own, or back into the clip's place (`float` false).
    pub fn set_ara_floating(
        &mut self,
        track: suhina_model::TrackId,
        device: usize,
        float: bool,
    ) -> Result<(), CommandError> {
        let key = plugin_key_at(&self.project, track, device).ok_or("no such plug-in device")?;
        self.plugins.set_ara_floating(key, float)
    }

    /// The installed plug-in a clip is opened in when its track has none that edits clips (ARA):
    /// its name, and what `device.addPlugin` takes for it. One the scan marked as ARA, Melodyne
    /// first; else Melodyne, which the host finds to support ARA when it loads. `None` without
    /// a catalog or such a plug-in.
    pub fn ara_plugin(&self) -> Option<(String, String)> {
        let catalog = &self.plugins.env()?.catalog;
        let melodyne = |p: &&suhina_plugins::scan::PluginInfo| {
            p.name.as_bytes().windows(8).any(|w| w.eq_ignore_ascii_case(b"melodyne"))
        };
        let chosen = catalog
            .plugins()
            .filter(|p| p.ara)
            .find(melodyne)
            .or_else(|| catalog.plugins().find(|p| p.ara))
            .or_else(|| catalog.plugins().filter(|p| p.spec().starts_with("vst3:")).find(melodyne))?;
        Some((chosen.name.clone(), chosen.spec()))
    }

    /// The tracks whose chain runs a plug-in bound to their audio clips (ARA), each with the
    /// first such device's index in its chain.
    pub fn ara_tracks(&self) -> Vec<(suhina_model::TrackId, usize)> {
        let mut found: Vec<(suhina_model::TrackId, usize)> = Vec::new();
        for device in suhina_engine::plugin_devices(&self.project) {
            let Some(track) = device.track else { continue };
            if !self.plugins.edits_clips(device.key) {
                continue;
            }
            match found.iter_mut().find(|(t, _)| *t == track) {
                Some(entry) => entry.1 = entry.1.min(device.position),
                None => found.push((track, device.position)),
            }
        }
        found
    }

    /// The room in pixels the window has for these plug-in devices' embedded editors (track,
    /// device, width and height), which they are asked to fit.
    pub fn fit_editors(&mut self, wanted: &[(suhina_model::TrackId, usize, (u32, u32))]) {
        let keyed: Vec<(u64, (u32, u32))> = wanted
            .iter()
            .filter_map(|(track, device, size)| Some((plugin_key_at(&self.project, *track, *device)?, *size)))
            .collect();
        self.plugins.fit_editors(&keyed);
    }

    /// How far the project's plug-ins have got with starting (all zero while it does not play).
    pub fn plugin_progress(&self) -> PluginProgress {
        self.plugins.progress()
    }

    /// Waits up to `timeout` until none of the project's plug-ins is still starting.
    pub fn wait_for_plugins(&mut self, timeout: std::time::Duration) -> Vec<String> {
        match self.sync.player.as_mut() {
            Some(player) => self.plugins.wait_started(&self.project, &self.audio.pool, player, timeout),
            None => Vec::new(),
        }
    }

    /// What is known about the plug-in device at `device` among `track`'s channel's devices;
    /// `None` for devices that are not plug-ins or while the project does not play.
    pub fn plugin_report(&mut self, track: suhina_model::TrackId, device: usize) -> Option<PluginReport> {
        let key = plugin_key_at(&self.project, track, device)?;
        self.plugins.report(key)
    }

    /// The report of the plug-in device at `path` on `track`, inside device groups too.
    pub fn plugin_report_at(&mut self, track: suhina_model::TrackId, path: &[usize]) -> Option<PluginReport> {
        let key = plugin_key_at_path(&self.project, track, path)?;
        self.plugins.report(key)
    }

    /// Has the plug-in device at `path` on `track` list its parameters again in the background,
    /// for its generated panel, when its list is a couple of seconds old
    /// ([`rack::Rack::list_params`]).
    pub fn list_plugin_params_at(&mut self, track: suhina_model::TrackId, path: &[usize]) {
        if let Some(key) = plugin_key_at_path(&self.project, track, path) {
            self.plugins.list_params(key);
        }
    }

    pub fn player(&self) -> Option<&Player> {
        self.sync.player.as_ref()
    }

    pub(crate) fn player_mut(&mut self) -> Result<&mut Player, CommandError> {
        self.sync.player.as_mut().ok_or_else(|| {
            CommandError(
                "no audio engine is playing this project: start one with `suhina-cli play --launcher`, or the MCP                  server with --audio"
                    .into(),
            )
        })
    }

    /// Opens a project: a Suhina project (`.suhina`) or a Live Set to import (`.als`). Returns
    /// the session and the importer's warnings. Any other file is refused by its extension.
    /// The NAM devices' files are read here, so the session is ready for the planned audio
    /// before a player is attached.
    pub fn open(path: &Path) -> Result<(Self, Vec<String>), CommandError> {
        let (project, warnings) = if suhina_project_file::is_project_file(path) {
            let project = suhina_project_file::read_file(path).map_err(|e| format!("{}: {e}", path.display()))?;
            (project, Vec::new())
        } else if !path.extension().is_some_and(|e| e.eq_ignore_ascii_case("als")) {
            return Err(format!(
                "{}: Suhina opens a Suhina project (.{}) or a Live Set (.als), not this file",
                path.display(),
                suhina_project_file::EXTENSION
            )
            .into());
        } else {
            let import = suhina_als::read_file(path).map_err(|e| format!("{}: {e}", path.display()))?;
            (import.project, import.warnings)
        };
        let mut session = Self::new(project);
        session.source = Some(path.to_path_buf());
        session.audio.folder = path.parent().map(Path::to_path_buf);
        session.sync_device_files();
        Ok((session, warnings))
    }

    /// A new, untitled project made from the template file `path` (any project Suhina opens).
    /// Its audio is still found where the template's is.
    pub fn from_template(path: &Path) -> Result<(Self, Vec<String>), CommandError> {
        let (mut session, warnings) = Self::open(path)?;
        session.source = None;
        Ok((session, warnings))
    }

    /// Opens a backup (`docs/spec/backups.md` › The offer on start) as an unsaved project tied
    /// to `file`, the project file it is a backup of: saving writes `file`, and the backup is
    /// left as it is. Without `file` (a project never saved) it opens untitled.
    pub fn recover(backup: &Path, file: Option<&Path>) -> Result<(Self, Vec<String>), CommandError> {
        if !suhina_project_file::is_project_file(backup) {
            return Err(format!("{} is not a Suhina project", backup.display()).into());
        }
        let (mut session, warnings) = Self::open(backup)?;
        session.source = file.map(Path::to_path_buf);
        // The changes since the project file was saved are in the backup only.
        session.history.changes = 1;
        // ...and the backup holds them, so it is not written again until something changes.
        session.backup.backed_at = 1;
        Ok((session, warnings))
    }

    /// The newest backup of the project written in this run.
    pub fn last_backup(&self) -> Option<&backup::Backup> {
        self.backup.last.as_ref()
    }

    /// The folder the project's relative audio paths start from, as the audio is loaded.
    pub fn audio_folder(&self) -> Option<&Path> {
        self.audio.folder.as_deref()
    }

    /// Whether the project changed since it was opened or last saved, by a command or in a
    /// plug-in's editor (noticed as its state is read now and then; see
    /// [`Session::check_plugin_edits`]).
    pub fn has_unsaved_changes(&self) -> bool {
        self.history.changes != self.history.saved_at || self.plugins.edited()
    }

    /// Reads the running plug-ins' states now, so a change made in a plug-in's editor since
    /// the last read shows in [`Session::has_unsaved_changes`]: before asking whether to save
    /// on close or quit. Waits at most one second for plug-ins that do not answer.
    pub fn check_plugin_edits(&mut self) {
        self.plugins.check_edits();
    }

    /// Advances a two-read check of all running plug-ins without waiting. Call each frame
    /// until true before deciding whether to offer saving; processing and MIDI count too.
    /// All rounds share one second, after which the last known states are used.
    pub fn poll_plugin_edits(&mut self) -> bool {
        self.plugins.poll_edits()
    }

    /// Saves the project as a Suhina project at `path` (`.suhina` is added when it does not end
    /// in it), with the running plug-ins' state as it is now and audio paths written relative to the new
    /// file. The session then belongs to that file, unless `copy` (a template, a backup), which
    /// leaves the session as it was. Returns the path written.
    pub fn save(&mut self, path: &Path, copy: bool) -> Result<PathBuf, CommandError> {
        let mut path = path.to_path_buf();
        if !suhina_project_file::is_project_file(&path) {
            let mut name = path.file_name().map(|n| n.to_os_string()).unwrap_or_default();
            if name.is_empty() {
                return Err("give the project a file name".into());
            }
            name.push(".");
            name.push(suhina_project_file::EXTENSION);
            path.set_file_name(name);
        }
        let to = path.parent().map_or_else(PathBuf::new, Path::to_path_buf);
        let (mut project, states) = self.heard();
        suhina_project_file::rebase_audio(&mut project, &self.audio_from(), &to);
        suhina_project_file::write_file(&path, &project).map_err(|e| CommandError(e.0))?;
        if !copy {
            self.source = Some(path.clone());
            self.history.saved_at = self.history.changes;
            self.plugins.saved(&states);
        }
        Ok(path)
    }

    /// The project as it would be saved into `folder`: with the running plug-ins' state as it
    /// is now ([`Session::project_as_heard`]) and audio paths relative to `folder`.
    pub(crate) fn snapshot(&mut self, folder: &Path) -> Project {
        // The plug-in states last read, not asked for now: asking waited up to a second for a
        // busy plug-in on the window's thread at every backup.
        let states = self.plugins.known_states();
        let mut project = self.with_states(&states);
        suhina_project_file::rebase_audio(&mut project, &self.audio_from(), folder);
        project
    }

    /// The folder the project's relative audio paths start from: its audio folder, or the
    /// current one.
    fn audio_from(&self) -> PathBuf {
        match &self.audio.folder {
            Some(folder) => folder.clone(),
            None => std::env::current_dir().unwrap_or_default(),
        }
    }

    pub fn project(&self) -> &Project {
        &self.project
    }

    /// A copy of the project with each running plug-in's state as it is now (what its editor
    /// changed since the project was opened), for a save or a render that should play what is
    /// heard (ADR 0006 decision 5). The component state goes into `Device::state` and a VST3
    /// controller's into its extension. Each host process is asked on a thread of its own and
    /// the answers are waited for at most a second, as a save does; a plug-in that is not running
    /// keeps the state it was last given. The open project is not changed: reading a plug-in's
    /// state is neither a change nor an undo step.
    pub fn project_as_heard(&mut self) -> Project {
        self.heard().0
    }

    /// [`Session::project_as_heard`], with the plug-in states it holds by device key.
    fn heard(&mut self) -> (Project, std::collections::HashMap<u64, suhina_plugins::SavedState>) {
        let states = self.plugins.current_states();
        (self.with_states(&states), states)
    }

    /// A copy of the project with `states` (by device key) in its plug-in devices.
    pub(crate) fn with_states(&self, states: &std::collections::HashMap<u64, suhina_plugins::SavedState>) -> Project {
        let mut project = self.project.clone();
        for device in suhina_engine::plugin_devices(&project) {
            let Some(state) = states.get(&device.key) else { continue };
            let slot = project
                .channels
                .iter_mut()
                .find(|c| c.id == device.channel)
                .and_then(|c| suhina_model::device_at_mut(&mut c.devices, &device.path));
            if let Some(slot) = slot {
                slot.state = Some(state.data.clone());
                slot.extensions.0.retain(|e| e.key != "Vst3ControllerState");
                if let Some(controller) = &state.controller {
                    slot.extensions.0.push(suhina_model::Extension {
                        format: "vst3".into(),
                        key: "Vst3ControllerState".into(),
                        data: controller.clone(),
                    });
                }
            }
        }
        project
    }

    pub fn source(&self) -> Option<&Path> {
        self.source.as_deref()
    }

    pub fn can_undo(&self) -> bool {
        !self.history.undo.is_empty()
    }

    pub fn can_redo(&self) -> bool {
        !self.history.redo.is_empty()
    }

    /// The colours a track without one of its own shows, by its place among the tracks (the
    /// window's palette, which follows the theme), and `fixed`, the colour a new track gets
    /// instead of the palette's, when set. Without a palette, tracks keep no colour of their own.
    pub fn set_track_colours(&mut self, palette: Vec<suhina_model::Rgb>, fixed: Option<suhina_model::Rgb>) {
        if self.track_colours.0 != palette || self.track_colours.1 != fixed {
            self.track_colours = (palette, fixed);
        }
    }

    /// After an edit that adds, removes or moves tracks: every track keeps the colour it showed
    /// (#690, #709). A track without a colour of its own showed its place's colour from the
    /// palette, so it is given that colour now, before the places shift; a new track gets the
    /// colour of its place, or the fixed one. Part of the same undo step.
    fn keep_track_colours(&mut self, before: &Project) {
        let (palette, fixed) = &self.track_colours;
        if palette.is_empty() {
            return;
        }
        let order = |p: &Project| p.tracks.iter().map(|t| t.id).collect::<Vec<_>>();
        let was = order(before);
        if was == order(&self.project) {
            return;
        }
        let colour = |place: usize| palette[place % palette.len()];
        for (place, track) in before.tracks.iter().enumerate().filter(|(_, t)| t.color.is_none()) {
            if let Some(now) = self.project.tracks.iter_mut().find(|t| t.id == track.id && t.color.is_none()) {
                now.color = Some(colour(place));
            }
        }
        for (place, track) in self.project.tracks.iter_mut().enumerate() {
            if track.color.is_none() && !was.contains(&track.id) {
                track.color = Some(fixed.unwrap_or_else(|| colour(place)));
            }
        }
    }

    /// Sends the project to the playing engine again if a continued edit could not reach it
    /// (its queue was full). The window calls it every frame, through `Projects::poll_plugins`.
    pub fn catch_up_player(&mut self) {
        if self.sync.behind && self.update_player(None).is_ok() {
            self.sync.behind = false;
        }
    }

    /// Reads the NAM devices' files into the assets ([docs/spec/nam-amp-and-cab.md]): each
    /// file-backed device's state, once per unique path and only when its file's last write is
    /// newer than what a load saw. A file that fails leaves its message in
    /// `device_asset_notes`, where a panel reads it. The reads run in the session's thread.
    pub fn sync_device_files(&mut self) {
        // Paths still attached to devices, by kind.
        let mut need: HashMap<String, String> = HashMap::new();
        for track in &self.project.tracks {
            let Some(channel) = track.channel.and_then(|ch| self.project.channel(ch)) else {
                continue;
            };
            for device in &channel.devices {
                let suhina_model::DeviceKind::Builtin(id) = &device.kind else { continue };
                if !suhina_device_registry::is_file_device(id) {
                    continue;
                }
                if let Some(path) = device
                    .state
                    .as_deref()
                    .and_then(|state| std::str::from_utf8(state).ok())
                    .filter(|path| !path.is_empty())
                {
                    need.insert(path.to_string(), id.clone());
                }
            }
        }
        if need.is_empty() {
            return;
        }
        let mut changed = false;
        for (path, kind) in need.iter() {
            let mtime = std::fs::metadata(path).ok().and_then(|m| m.modified().ok());
            if (self.device_files.assets.entries.contains_key(path.as_str())
                || self.device_files.notes.contains_key(path.as_str()))
                && self.device_files.modified.get(path).and_then(|seen| *seen) == mtime
            {
                continue;
            }
            let result = self.load_device_file(kind, path);
            self.device_files.modified.insert(path.clone(), mtime);
            match result {
                Ok(asset) => {
                    self.device_files.assets.set(path.clone(), asset);
                    self.device_files.notes.remove(path.as_str());
                }
                Err(e) => {
                    self.device_files.assets.entries.remove(path.as_str());
                    self.device_files.notes.insert(path.clone(), e);
                }
            }
            changed = true;
        }
        if changed {
            self.device_files.changed = true;
        }
    }

    /// What a file-backed device of `kind` runs for the file at `path`: a parsed model for the
    /// Amp Modeler or a decoded IR for the Cab Loader. The reads run here, at command or
    /// project-load time, never on the audio thread.
    pub(crate) fn load_device_file(
        &self,
        kind: &str,
        path: &str,
    ) -> Result<suhina_device_registry::DeviceAsset, String> {
        // The extension check `device.setNamFile` makes: a project's stored path gets the same
        // treatment, so a hand-made or foreign project file cannot point a device at some
        // other file (a UNC path, for one).
        let want_extension = match kind {
            "nam-amp-modeler" => "nam",
            "nam-cab-loader" => "wav",
            other => return Err(format!("{other:?} does not run a file")),
        };
        if !path.to_lowercase().ends_with(&format!(".{want_extension}")) {
            return Err(format!("a {kind} runs a .{want_extension} file"));
        }
        // An IR joins the project at the engine's rate; the session's rate is the player's, or
        // 48 kHz until the first output opens.
        let rate = self.sync.player.as_ref().map_or(48_000, Player::sample_rate);
        match kind {
            "nam-amp-modeler" => {
                let model = suhina_device_nam_amp_modeler::model::read(std::path::Path::new(path))?;
                // The runtime's verdict, so the panel can say why a file is silent.
                if let Err(e) = nam_rs::Model::from_nam(&model) {
                    return Err(format!("the model does not run: {e}"));
                }
                Ok(suhina_device_registry::DeviceAsset::Nam(std::sync::Arc::new(model)))
            }
            "nam-cab-loader" => Ok(suhina_device_registry::DeviceAsset::Ir(std::sync::Arc::new(
                suhina_device_nam_cab_loader::ir::read(std::path::Path::new(path), rate)?,
            ))),
            _ => Err(format!("{kind:?} does not run a file")),
        }
    }

    /// Lets a playing engine hear the project as it is now, with any audio added to the pool.
    /// The edit stays made if this fails.
    /// `scenes` is the scene inserted or removed since the last update, if that is what changed.
    fn update_player(&mut self, scenes: Option<suhina_engine::SceneShift>) -> Result<(), CommandError> {
        self.sync_device_files();
        let Some(player) = self.sync.player.as_mut() else { return Ok(()) };
        if self.audio.changed {
            player.set_audio(std::sync::Arc::new(self.audio.pool.clone()));
            self.audio.changed = false;
        }
        if self.device_files.changed {
            player.set_device_assets(std::sync::Arc::new(self.device_files.assets.clone()));
            self.device_files.changed = false;
        }
        player.update_with_scenes(&self.project, scenes).map_err(|e| {
            CommandError(format!("the edit was made, but the playing engine could not follow it: {}", e.0))
        })
    }

    fn push_undo(&mut self, before: Project) {
        self.history.open_step = None;
        if self.history.undo.len() >= MAX_UNDO {
            self.history.undo.remove(0);
        }
        self.history.undo.push(before);
        self.history.redo.clear();
    }

    fn undo(&mut self) -> bool {
        let Some(previous) = self.history.undo.pop() else { return false };
        let undone = std::mem::replace(&mut self.project, previous);
        self.keep_counter(&undone);
        self.history.redo.push(undone);
        self.history.changes = self.history.changes.wrapping_add(1);
        true
    }

    fn redo(&mut self) -> bool {
        let Some(next) = self.history.redo.pop() else { return false };
        let undone = std::mem::replace(&mut self.project, next);
        self.keep_counter(&undone);
        self.history.undo.push(undone);
        self.history.changes = self.history.changes.wrapping_add(1);
        true
    }

    /// Keeps the id counter of the project an undo or redo stepped away from: an id handed out
    /// before the step stays used, so an object added after an undo never gets the id of the one
    /// the undo took away (ADR 0025).
    fn keep_counter(&mut self, left: &Project) {
        self.project.next_id = self.project.next_id.max(left.next_id);
    }
}

/// The key of the plug-in device at `device` among `track`'s channel's devices.
/// The track and name of the plug-in device whose key is `key`, for the trace.
pub fn plugin_device_name(project: &Project, key: u64) -> Option<String> {
    project.tracks.iter().find_map(|track| {
        let channel = project.channels.iter().find(|c| Some(c.id) == track.channel)?;
        (0..channel.devices.len()).find_map(|index| {
            (plugin_key_at(project, track.id, index) == Some(key))
                .then(|| format!("{} on {}", channel.devices.get(index).map_or("?", |d| d.name.as_str()), track.name))
        })
    })
}

pub(crate) fn plugin_key_at(project: &Project, track: suhina_model::TrackId, device: usize) -> Option<u64> {
    plugin_key_at_path(project, track, &[device])
}

/// The key of the plug-in device at `path` on `track`'s channel, inside device groups too.
pub(crate) fn plugin_key_at_path(project: &Project, track: suhina_model::TrackId, path: &[usize]) -> Option<u64> {
    let channel = project.track(track)?.channel?;
    suhina_engine::plugin_devices(project).into_iter().find(|d| d.channel == channel && d.path == path).map(|d| d.key)
}

fn unknown_command(id: &str) -> CommandError {
    let area = id.split('.').next().unwrap_or_default();
    let similar: Vec<&str> = commands().iter().map(|c| c.id).filter(|c| c.starts_with(area)).collect();
    if similar.is_empty() {
        CommandError(format!("unknown command {id}; command.list shows them all"))
    } else {
        CommandError(format!("unknown command {id}; did you mean one of: {}", similar.join(", ")))
    }
}

/// What `track` hears while armed, for the engine: its MIDI input, by port id (ADR 0017).
fn live_filter(project: &Project, track: suhina_model::TrackId) -> suhina_engine::LiveFilter {
    use suhina_engine::{LiveFilter, LivePorts};
    use suhina_model::MidiPort;
    let Some(input) = project.track(track).map(|t| &t.midi_input) else {
        return LiveFilter::default();
    };
    let ports = match &input.port {
        MidiPort::All => LivePorts::All,
        MidiPort::Named(name) => LivePorts::One(suhina_engine::port_id(name)),
        MidiPort::Nothing => LivePorts::Nothing,
    };
    LiveFilter { ports, channel: input.channel.and_then(|c| c.checked_sub(1)) }
}

#[cfg(test)]
mod tests {
    use serde_json::json;
    use suhina_plugins::SavedState;

    use super::*;

    /// Two reads of plug-in device `key` that agree, as the rack makes them now and then.
    fn read_twice(session: &mut Session, key: u64, byte: u8) {
        for _ in 0..2 {
            session.plugins.edits.read(key, &SavedState::from(vec![byte]));
        }
    }

    #[test]
    fn a_change_in_a_plug_ins_editor_is_unsaved_until_the_project_is_saved() {
        let dir = std::env::temp_dir().join(format!("suhina-plugin-edits-{}", std::process::id()));
        let mut session = Session::new(blank_project());
        read_twice(&mut session, 7, 1);
        assert!(!session.has_unsaved_changes(), "the state a plug-in runs with is not a change");

        read_twice(&mut session, 7, 2);
        assert!(session.has_unsaved_changes());
        assert_eq!(session.execute("project.info", &json!({})).unwrap()["unsaved_changes"], true);
        assert!(!session.can_undo(), "an editor tweak is not an undo step");

        // A copy (a template, a backup) is not the project saved.
        let path = dir.join("song.suhina").display().to_string();
        session
            .execute("project.save", &json!({ "path": dir.join("copy").display().to_string(), "copy": true }))
            .unwrap();
        assert!(session.has_unsaved_changes());

        session.execute("project.save", &json!({ "path": path })).unwrap();
        assert!(!session.has_unsaved_changes());
        let _clean = std::fs::remove_dir_all(&dir);
    }
}
