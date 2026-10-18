// Copyright (C) 2026 The Suhina contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! Several open projects, one of them active, like tabs. Commands go to the active project
//! unless a caller names another; only the active project plays (ADR 0007).

use serde_json::{Value, json};

use suhina_plugins::library::Library;

use crate::commands::{CommandKind, CommandSpec, Param, ParamType, commands, param};
use crate::plugins::plugin_commands;
use crate::{CommandError, Session};

/// Projects that may be open at once; opening more is refused. Each holds its own undo history.
pub const MAX_OPEN_PROJECTS: usize = 32;

/// Identifies an open project for as long as it stays open. Ids are never reused within one
/// [`Projects`], so a stale id cannot reach a project opened later.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct ProjectId(pub u32);

struct Open {
    id: ProjectId,
    name: String,
    session: Session,
}

/// The open projects in tab order, and which one is active; and the plug-in library, which
/// belongs to Suhina rather than to any one project.
#[derive(Default)]
pub struct Projects {
    open: Vec<Open>,
    active: Option<ProjectId>,
    last_id: u32,
    library: Library,
    /// What sessions start plug-ins with, made from the library when first needed and again
    /// after a scan or a change of how plug-ins are hosted.
    plugin_env: Option<std::sync::Arc<crate::PluginEnv>>,
    /// The window plug-in editors open over, for the active project's session.
    editor_owner: Option<u64>,
    /// Editors open embedded in that window's device panel (the window's setting).
    editor_embed: bool,
    /// The browser's library (ADR 0012); kept nowhere until one is given with
    /// [`Projects::set_file_library`].
    files: suhina_library::Library,
    /// The sample map's analysis (ADR 0013), made when first needed.
    pub(crate) sound_map: Option<suhina_soundmap::SoundMap>,
    /// Analysis runs only once started ([`Projects::start_sound_map`]), as indexing does.
    pub(crate) sound_map_started: bool,
    /// The library revision the sound map was last brought in line with.
    pub(crate) sound_map_revision: Option<u64>,
    /// The system's audio devices, when this program plays out loud (the desktop app, the MCP
    /// server with `--audio`).
    audio: Option<std::sync::Arc<dyn crate::AudioSystem>>,
    /// The output the user chose (`audio.setOutput`, the window's settings).
    output_choice: crate::OutputChoice,
    /// When a failed output was last opened again, so a device that keeps failing is tried
    /// every [`OUTPUT_RETRY`], not every frame.
    output_retried: Option<std::time::Instant>,
    /// When the audio system was last asked whether the chosen device is back, while the
    /// output plays in its place.
    output_watched: Option<std::time::Instant>,
    /// Backups of the open projects: the settings and the thread that writes them.
    pub(crate) backups: crate::backup::Backups,
    /// The MIDI input ports and where they send their notes (ADR 0017).
    midi: crate::midi::MidiInputs,
    /// With no project open, the empty session previews play in, with a player of its own
    /// (ADR 0011); made on the first preview and dropped before a project opens its output.
    preview: Option<Session>,
}

/// How often an output whose device reported an error is opened again.
const OUTPUT_RETRY: std::time::Duration = std::time::Duration::from_secs(2);
/// How often an output playing in place of the chosen device looks whether it is back.
const OUTPUT_WATCH: std::time::Duration = std::time::Duration::from_secs(3);

const PROJECT: Param = param("project", ParamType::Integer, "project id, from projects.list");

static PROJECT_COMMANDS: &[CommandSpec<Projects>] = &[
    CommandSpec {
        id: "projects.list",
        label: "List the open projects in tab order with id, name, source and whether each is active",
        params: &[],
        kind: CommandKind::Projects,
        run: list,
    },
    CommandSpec {
        id: "projects.switch",
        label: "Make an open project the active one; the previous one stops playing",
        params: &[PROJECT],
        kind: CommandKind::Projects,
        run: switch,
    },
    CommandSpec {
        id: "projects.next",
        label: "Make the next open project active, wrapping around after the last",
        params: &[],
        kind: CommandKind::Projects,
        run: next,
    },
    CommandSpec {
        id: "projects.previous",
        label: "Make the previous open project active, wrapping around before the first",
        params: &[],
        kind: CommandKind::Projects,
        run: previous,
    },
    CommandSpec {
        id: "projects.close",
        label: "Close an open project and its undo history; a neighbour becomes active if it was",
        params: &[PROJECT],
        kind: CommandKind::Projects,
        run: close,
    },
];

/// The commands that run on the open projects rather than on one project.
pub fn project_commands() -> &'static [CommandSpec<Projects>] {
    PROJECT_COMMANDS
}

impl Projects {
    pub fn new() -> Self {
        Self::default()
    }

    /// Lets the projects play out loud through `system`'s devices (`audio.*` commands,
    /// [`Projects::open_output`]).
    pub fn set_audio_system(&mut self, system: std::sync::Arc<dyn crate::AudioSystem>) {
        self.audio = Some(system);
    }

    pub fn audio_system(&self) -> Option<std::sync::Arc<dyn crate::AudioSystem>> {
        self.audio.clone()
    }

    /// Lets the projects hear MIDI controllers through `system`'s input ports (`midi.*`
    /// commands, ADR 0017), and opens every port that is on. Returns what the user should
    /// hear about: ports that could not be opened.
    pub fn set_midi_system(&mut self, system: std::sync::Arc<dyn crate::MidiSystem>) -> Vec<String> {
        self.midi.system = Some(system);
        self.midi.sync()
    }

    /// The MIDI input ports turned off, by name (the window keeps them in its settings).
    pub fn midi_inputs_off(&self) -> Vec<String> {
        self.midi.off.iter().cloned().collect()
    }

    /// Turns the ports `off` names off and every other one on, opening and closing them.
    pub fn set_midi_inputs_off(&mut self, off: impl IntoIterator<Item = String>) -> Vec<String> {
        self.midi.off = off.into_iter().collect();
        self.midi.sync()
    }

    /// Where the open MIDI ports send what they play: the active project's engine.
    pub fn live_input(&self) -> std::sync::Arc<crate::LiveInput> {
        std::sync::Arc::clone(&self.midi.input)
    }

    /// The MIDI input ports there were when they were last looked at (every two seconds while
    /// the window polls), by name: what a track's MIDI input can be set to.
    pub fn midi_ports(&self) -> &[String] {
        &self.midi.ports
    }

    pub(crate) fn midi_mut(&mut self) -> &mut crate::midi::MidiInputs {
        &mut self.midi
    }

    /// Sends the MIDI inputs' notes to the active project's engine when it is a new one, and
    /// looks at the ports again now and then, so a controller plugged in later is heard.
    fn poll_midi(&mut self) -> Vec<String> {
        if let Some(sender) = self.session_mut(None).ok().and_then(Session::take_live_input) {
            self.midi.input.connect(sender);
        }
        if self.midi.system.is_some() && self.midi.checked.is_none_or(|t| t.elapsed() >= crate::midi::MIDI_RESCAN) {
            return self.midi.sync();
        }
        Vec::new()
    }

    pub fn output_choice(&self) -> &crate::OutputChoice {
        &self.output_choice
    }

    /// Chooses the output players open on from now on; [`Projects::reopen_output`] moves the
    /// playing one there.
    pub fn set_output_choice(&mut self, choice: crate::OutputChoice) {
        self.output_choice = choice;
        self.output_watched = None;
    }

    /// Opens the chosen output for project `id` (the active one when `None`), with its
    /// transport stopped. Returns a notice when the chosen device was not there and the default
    /// plays instead.
    pub fn open_output(&mut self, id: Option<ProjectId>) -> Result<Option<String>, CommandError> {
        let system = self.audio.clone().ok_or("no audio output here")?;
        let choice = self.output_choice.clone();
        let session = self.session_mut(id)?;
        let (mut notice, mut kept) = (None, false);
        session
            .replace_player(|project, audio, assets, previous| {
                system.open(project, audio, assets, &choice, previous).map(|opened| {
                    notice = opened.notice;
                    kept = opened.plugins_kept;
                    opened.player
                })
            })
            .map_err(|e| CommandError(format!("no audio output: {e}")))?;
        if !kept {
            session.restart_plugins();
        }
        Ok(notice)
    }

    /// Moves the active project's output to the one chosen now, keeping the transport where
    /// it is (the playhead, playing or stopped, and the metronome) and the plug-ins running.
    /// Nothing happens while no project plays. Returns a notice as [`Projects::open_output`].
    pub fn reopen_output(&mut self) -> Result<Option<String>, CommandError> {
        // The preview output opens on the new choice when next needed.
        self.close_preview_output();
        let Some(system) = self.audio.clone() else { return Ok(None) };
        let choice = self.output_choice.clone();
        let Ok(session) = self.session_mut(None) else { return Ok(None) };
        let Some(transport) = session.player().map(crate::audio::Transport::of) else { return Ok(None) };
        let (mut notice, mut kept) = (None, false);
        session
            .replace_player(|project, audio, assets, previous| {
                system.open(project, audio, assets, &choice, previous).map(|opened| {
                    notice = opened.notice;
                    kept = opened.plugins_kept;
                    opened.player
                })
            })
            .map_err(|e| CommandError(format!("no audio output: {e}")))?;
        if !kept {
            session.restart_plugins();
        }
        if let Ok(player) = session.player_mut() {
            transport.restore(player)?;
        }
        // Plug-ins that could not carry over (a new sample rate) start again.
        session.sync_plugins();
        Ok(notice)
    }

    pub fn len(&self) -> usize {
        self.open.len()
    }

    pub fn is_empty(&self) -> bool {
        self.open.is_empty()
    }

    /// Adds `session` after the last tab and makes it active. The previously active project
    /// stops playing. `name` defaults to the source file's name, or "Untitled N".
    pub fn add(&mut self, session: Session, name: Option<String>) -> Result<ProjectId, CommandError> {
        if self.open.len() >= MAX_OPEN_PROJECTS {
            return Err(format!("at most {MAX_OPEN_PROJECTS} projects can be open; close one first").into());
        }
        self.last_id = self.last_id.checked_add(1).ok_or("too many projects opened in this session")?;
        let id = ProjectId(self.last_id);
        let name = name
            .or_else(|| session.source().and_then(|s| s.file_stem()).map(|s| s.to_string_lossy().into_owned()))
            .unwrap_or_else(|| format!("Untitled {}", id.0));
        self.close_preview_output();
        self.open.push(Open { id, name, session });
        self.activate(id);
        Ok(id)
    }

    /// The session previews play in: the active project's, or with none open the empty one of
    /// the previews' own, once it has been made ([`Projects::preview_session_mut`]).
    pub fn preview_session(&self) -> Option<&Session> {
        match self.active {
            Some(id) => self.get(id),
            None => self.preview.as_ref(),
        }
    }

    /// The session previews play in, as [`Projects::preview_session`]; with no project open,
    /// the previews' own empty session is made and its player opened on the chosen output
    /// when there is none yet. Only one output is ever open: this one exists only while no
    /// project is, and a project's output opens after it is closed (ADR 0011).
    pub fn preview_session_mut(&mut self) -> Result<&mut Session, CommandError> {
        if self.active.is_some() {
            return self.session_mut(None);
        }
        if self.preview.as_ref().is_none_or(|s| s.player().is_none()) {
            let system = self.audio.clone().ok_or("no audio output here: open or make a project to preview")?;
            let choice = self.output_choice.clone();
            let mut session = Session::new(suhina_model::Project::default());
            session
                .replace_player(|project, audio, assets, previous| {
                    system.open(project, audio, assets, &choice, previous).map(|opened| opened.player)
                })
                .map_err(|e| CommandError(format!("no audio output: {e}")))?;
            self.preview = Some(session);
        }
        self.preview.as_mut().ok_or_else(no_project)
    }

    /// Closes the output previews play on while no project is open, so a project can open its
    /// own (a device may only be open once). The desktop app calls it before opening a project's
    /// output; adding a project and choosing another output do too.
    pub fn close_preview_output(&mut self) {
        self.preview = None;
    }

    pub fn active(&self) -> Option<ProjectId> {
        self.active
    }

    /// The ids of the open projects in tab order.
    pub fn ids(&self) -> Vec<ProjectId> {
        self.open.iter().map(|o| o.id).collect()
    }

    /// The name `id` is shown with, as in its tab.
    pub fn name(&self, id: ProjectId) -> Option<&str> {
        self.open.iter().find(|o| o.id == id).map(|o| o.name.as_str())
    }

    pub fn get(&self, id: ProjectId) -> Option<&Session> {
        self.open.iter().find(|o| o.id == id).map(|o| &o.session)
    }

    pub fn get_mut(&mut self, id: ProjectId) -> Option<&mut Session> {
        self.open.iter_mut().find(|o| o.id == id).map(|o| &mut o.session)
    }

    /// The session `id` names, or the active one when `id` is `None`.
    pub fn session_mut(&mut self, id: Option<ProjectId>) -> Result<&mut Session, CommandError> {
        let id = match id.or(self.active) {
            Some(id) => id,
            None => return Err(no_project()),
        };
        self.get_mut(id).ok_or_else(|| unknown_project(id))
    }

    /// Makes `id` active. The previously active project's player is closed, so only one project
    /// plays at a time.
    pub fn switch(&mut self, id: ProjectId) -> Result<(), CommandError> {
        if self.get(id).is_none() {
            return Err(unknown_project(id));
        }
        self.activate(id);
        Ok(())
    }

    /// Moves `step` tabs from the active project, wrapping around. Returns the new active id.
    pub fn cycle(&mut self, step: isize) -> Result<ProjectId, CommandError> {
        let len = self.open.len();
        let current = self.active.and_then(|a| self.open.iter().position(|o| o.id == a)).ok_or_else(no_project)?;
        let distance = step.unsigned_abs() % len;
        let target = if step >= 0 { (current + distance) % len } else { (current + len - distance) % len };
        let id = self.open.get(target).map(|o| o.id).ok_or_else(no_project)?;
        self.activate(id);
        Ok(id)
    }

    /// Closes `id` and returns its session. If it was active, the tab to its right becomes
    /// active (or the one to its left when it was the last).
    /// Closing is a critical operation: with backups before critical operations, a project with
    /// unsaved changes is backed up first.
    pub fn close(&mut self, id: ProjectId) -> Result<Session, CommandError> {
        if self.get(id).is_none() {
            return Err(unknown_project(id));
        }
        self.back_up_critical(id);
        self.backups_after_close(id);
        let index = self.open.iter().position(|o| o.id == id).ok_or_else(|| unknown_project(id))?;
        let closed = self.open.remove(index);
        if self.active == Some(id) {
            self.active = None;
            let neighbour = self.open.get(index).or_else(|| self.open.last()).map(|o| o.id);
            if let Some(next) = neighbour {
                self.activate(next);
            }
        }
        Ok(closed.session)
    }

    /// Uses `library` for the `plugins.*` commands instead of the one in Suhina's settings
    /// folder (for tests, or a portable setup).
    pub fn with_library(library: Library) -> Self {
        Self { library, ..Self::default() }
    }

    /// The plug-in library. Taking it for changing counts as changing it: sessions get a new
    /// catalog the next time they start a plug-in.
    pub fn library_mut(&mut self) -> &mut Library {
        self.plugin_env = None;
        &mut self.library
    }

    /// What sessions start plug-ins with (ADR 0006): the catalog, the host executable and how
    /// plug-ins share host processes.
    pub fn plugin_env(&mut self) -> std::sync::Arc<crate::PluginEnv> {
        if let Some(env) = &self.plugin_env {
            return std::sync::Arc::clone(env);
        }
        let env = std::sync::Arc::new(crate::PluginEnv::from_library(&mut self.library));
        self.plugin_env = Some(std::sync::Arc::clone(&env));
        env
    }

    /// Uses `library` for the browser and the `library.*` commands: the desktop app and the
    /// MCP server give it Suhina's settings folder.
    pub fn set_file_library(&mut self, library: suhina_library::Library) {
        self.files = library;
        // The sound map keeps its caches beside the library's indexes.
        self.sound_map = None;
        self.sound_map_revision = None;
    }

    /// The browser's library.
    pub fn file_library_mut(&mut self) -> &mut suhina_library::Library {
        &mut self.files
    }

    /// The window plug-in editors open over (a Win32 `HWND`, an X11 window id): the desktop
    /// app's main window, so editors float above it. `None` gives them windows of their own.
    pub fn set_editor_owner(&mut self, owner: Option<u64>) {
        self.editor_owner = owner;
    }

    /// Whether plug-in editors open embedded in the owner window's device panel rather than
    /// in windows of their own (nothing without an owner window).
    pub fn set_editor_embedding(&mut self, embed: bool) {
        self.editor_embed = embed;
    }

    /// Keeps the active project's plug-ins in line with it (ADR 0006): starts new ones in the
    /// background, takes in the ones that have started, notices crashes. Call it often from the
    /// thread that owns the projects (the window calls it every frame); commands call it too.
    /// Returns what the user should hear about. It also lets the playing engine catch up with
    /// a drag that outran it ([`Session::catch_up_player`]).
    pub fn poll_plugins(&mut self) -> Vec<String> {
        let mut notices = Vec::new();
        if let Some(session) = self.active.and_then(|id| self.get_mut(id)) {
            session.catch_up_player();
            notices.extend(session.poll_recording());
        }
        notices.extend(self.check_output());
        notices.extend(self.sync_active_plugins());
        notices.extend(self.poll_midi());
        notices
    }

    /// Opens the active project's output again when its device reported an error: a driver
    /// that asked to be reset (ASIO, after its buffer size or sample rate changed in its own
    /// panel), or a device unplugged, which then falls back to the default one. While the
    /// output plays in place of the chosen device, looks every few seconds whether that device
    /// is back and, when it is, moves the output there, keeping the transport (as
    /// `audio.setOutput` does). Returns what the user should hear about.
    pub fn check_output(&mut self) -> Vec<String> {
        let Some(info) = self.session_mut(None).ok().and_then(|s| s.player()).and_then(crate::Player::output_info)
        else {
            return Vec::new();
        };
        if info.failed {
            if self.output_retried.is_some_and(|t| t.elapsed() < OUTPUT_RETRY) {
                return Vec::new();
            }
            self.output_retried = Some(std::time::Instant::now());
            return match self.reopen_output() {
                Ok(notice) => {
                    let mut said = vec!["the audio device reported an error; its output was opened again".to_string()];
                    said.extend(notice);
                    said
                }
                Err(e) => vec![e.0],
            };
        }
        if info.fallback { self.return_to_chosen() } else { Vec::new() }
    }

    /// Moves the output back to the chosen device when the audio system says it is there,
    /// asking at most every [`OUTPUT_WATCH`].
    fn return_to_chosen(&mut self) -> Vec<String> {
        let Some(system) = self.audio.clone() else { return Vec::new() };
        if self.output_watched.is_some_and(|t| t.elapsed() < OUTPUT_WATCH) {
            return Vec::new();
        }
        self.output_watched = Some(std::time::Instant::now());
        if system.chosen_present(&self.output_choice) != Some(true) {
            return Vec::new();
        }
        if let Err(e) = self.reopen_output() {
            return vec![e.0];
        }
        // Gone again meanwhile: it plays in place of it again, and the next look tries again.
        let now = self.session_mut(None).ok().and_then(|s| s.player()).and_then(crate::Player::output_info);
        match now {
            Some(info) if !info.fallback => vec![format!("the audio device {} is back; it plays again", info.device)],
            _ => Vec::new(),
        }
    }

    fn sync_active_plugins(&mut self) -> Vec<String> {
        let Some(active) = self.active else { return Vec::new() };
        // The catalog is handed over even when the project has no player (a stopped headless
        // session, an agent's): the device commands look the installed plug-ins up in it, and
        // a sync without a player only keeps the rack stopped.
        let env = self.plugin_env();
        let (owner, embed) = (self.editor_owner, self.editor_embed);
        let Some(session) = self.get_mut(active) else { return Vec::new() };
        session.set_plugin_env(env);
        session.set_editor_owner(owner);
        session.set_editor_embedding(embed);
        session.sync_plugins()
    }

    /// Runs command `id`: a `projects.*` or `plugins.*` command on the open projects (and the
    /// plug-in library), anything else on the active project. `commands.list` lists every kind.
    pub fn execute(&mut self, id: &str, params: &Value) -> Result<Value, CommandError> {
        self.execute_in(None, id, params)
    }

    /// Like [`Projects::execute`], but a project command runs on `project` instead of the active
    /// one. Transport commands only work on the active project, the one that plays.
    pub fn execute_in(&mut self, project: Option<ProjectId>, id: &str, params: &Value) -> Result<Value, CommandError> {
        if id == "commands.list" {
            check_params(id, &[], params)?;
            return Ok(all_commands());
        }
        if let Some(spec) = projects_commands().find(|c| c.id == id) {
            if project.is_some() {
                return Err(format!("{id} does not run on one project; it takes no target project").into());
            }
            check_params(id, spec.params, params)?;
            return (spec.run)(self, params);
        }
        // With no project open, previews play in the previews' own session.
        if project.is_none() && self.active.is_none() && PREVIEW_COMMANDS.contains(&id) {
            // Nothing plays yet: stopping opens no output.
            if id == "preview.stop" && self.preview.is_none() {
                return Ok(json!({ "stopped": true }));
            }
            return self.preview_session_mut()?.execute(id, params);
        }
        // A command may need a plug-in that has started meanwhile, or add one.
        self.poll_plugins();
        let target = project.or(self.active);
        if let Some(target) = target
            && crate::backup::CRITICAL_COMMANDS.contains(&id)
        {
            self.back_up_critical(target);
        }
        let result = self.session_mut(project)?.execute(id, params);
        self.poll_plugins();
        if let Ok(value) = &result {
            crate::library::record_use(&mut self.files, id, params, value);
        }
        // A project saved under a new name is shown by it; its backups need no offering.
        if id == "project.save"
            && let Ok(saved) = &result
            && saved.get("copy") == Some(&Value::Bool(false))
            && let Some(target) = target
        {
            if let Some(open) = self.open.iter_mut().find(|o| o.id == target)
                && let Some(stem) = open.session.source().and_then(|s| s.file_stem())
            {
                open.name = stem.to_string_lossy().into_owned();
            }
            self.backups_after_save(target);
        }
        result
    }

    fn activate(&mut self, id: ProjectId) {
        if self.active == Some(id) {
            return;
        }
        if let Some(previous) = self.active.and_then(|a| self.get_mut(a)) {
            previous.detach_player();
        }
        self.active = Some(id);
    }
}

/// Every command, project commands first, as `commands.list` shows them.
pub fn all_commands() -> Value {
    let projects = projects_commands().map(CommandSpec::describe);
    Value::Array(projects.chain(commands().iter().map(CommandSpec::describe)).collect())
}

/// The commands that run on [`Projects`] rather than on one project's session: the ones
/// [`Projects::execute_in`] runs, in the order [`all_commands`] and [`commands_markdown`] list them.
fn projects_commands() -> impl Iterator<Item = &'static CommandSpec<Projects>> {
    PROJECT_COMMANDS
        .iter()
        .chain(plugin_commands())
        .chain(crate::library_commands())
        .chain(crate::sound_map_commands())
        .chain(crate::audio_commands())
        .chain(crate::backup::backup_commands())
        .chain(crate::midi_commands())
}

/// `docs/commands.md`: every command in the order of [`all_commands`], as `suhina-cli commands
/// --markdown` prints it.
pub fn commands_markdown() -> String {
    let mut out = String::from(
        "# Commands\n\n\
         Every command Suhina has, generated from the command registry in `crates/session` by\n\
         `suhina-cli commands --markdown`; do not edit by hand. A workflow regenerates it on `main`\n\
         after every merge, so pull requests leave it alone. The CLI, the MCP server and the UI run\n\
         these same commands ([ADR 0002](decisions/0002-commands.md), [`mcp.md`](mcp.md)).\n\n\
         | Command | Kind | What it does | Parameters |\n\
         |---|---|---|---|\n",
    );
    let rows =
        projects_commands().map(CommandSpec::markdown_row).chain(commands().iter().map(CommandSpec::markdown_row));
    for row in rows {
        out.push_str(&row);
        out.push('\n');
    }
    out
}

fn check_params(id: &str, expected: &[Param], params: &Value) -> Result<(), CommandError> {
    if !(params.is_object() || params.is_null()) {
        return Err(format!("{id}: parameters must be a JSON object").into());
    }
    if let Some(object) = params.as_object()
        && let Some(unknown) = object.keys().find(|key| !expected.iter().any(|p| p.name == key.as_str()))
    {
        let known: Vec<&str> = expected.iter().map(|p| p.name).collect();
        return Err(format!("{id}: unknown parameter {unknown:?}; expected {known:?}").into());
    }
    Ok(())
}

/// The commands that run with no project open, in the previews' own session.
const PREVIEW_COMMANDS: [&str; 2] = ["preview.play", "preview.stop"];

fn no_project() -> CommandError {
    CommandError("no project is open".into())
}

fn unknown_project(id: ProjectId) -> CommandError {
    CommandError(format!("no open project has id {}; projects.list shows them", id.0))
}

pub(crate) fn project_param(params: &Value) -> Result<ProjectId, CommandError> {
    params
        .get("project")
        .and_then(Value::as_u64)
        .and_then(|n| u32::try_from(n).ok())
        .map(ProjectId)
        .ok_or_else(|| "project must be a project id from projects.list".into())
}

fn list(projects: &mut Projects, _: &Value) -> Result<Value, CommandError> {
    Ok(json!({
        "active": projects.active.map(|a| a.0),
        "projects": projects.open.iter().map(|o| json!({
            "project": o.id.0,
            "name": o.name,
            "source": o.session.source().map(|s| s.display().to_string()),
            "active": projects.active == Some(o.id),
            "playing": o.session.player().is_some(),
            "can_undo": o.session.can_undo(),
        })).collect::<Vec<_>>(),
    }))
}

fn switch(projects: &mut Projects, params: &Value) -> Result<Value, CommandError> {
    let id = project_param(params)?;
    projects.switch(id)?;
    Ok(json!({ "active": id.0 }))
}

fn next(projects: &mut Projects, _: &Value) -> Result<Value, CommandError> {
    Ok(json!({ "active": projects.cycle(1)?.0 }))
}

fn previous(projects: &mut Projects, _: &Value) -> Result<Value, CommandError> {
    Ok(json!({ "active": projects.cycle(-1)?.0 }))
}

fn close(projects: &mut Projects, params: &Value) -> Result<Value, CommandError> {
    let id = project_param(params)?;
    projects.close(id)?;
    Ok(json!({ "closed": id.0, "active": projects.active.map(|a| a.0) }))
}
