// Copyright (C) 2026 The Suhina contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! The MCP server. Following PhotoCraft's conventions, it exposes a few generic tools rather than
//! one tool per command: agents discover commands with `command_list` and run them with
//! `command_run` or `command_batch`, so the tool list stays small as commands grow.

use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex, PoisonError};

use rmcp::handler::server::router::tool::ToolRouter;
use rmcp::handler::server::tool::ToolCallContext;
use rmcp::handler::server::wrapper::Parameters;
use rmcp::model::*;
use rmcp::service::RequestContext;
use rmcp::{ErrorData as McpError, RoleServer, ServerHandler, ServiceExt, tool, tool_router};
use schemars::JsonSchema;
use serde::Deserialize;
use serde_json::{Value, json};
use suhina_engine::model::{LaneData, Project};
use suhina_session::{
    AudioSystem, CommandKind, MidiSystem, Player, ProjectId, Projects, Session, all_commands, project_commands,
};

use crate::attach::AppHost;
use crate::render::{LaunchStep, render_launch_script, render_wav, scene_as_arrangement};
use crate::workspace::{Workspace, display_path};

const INSTRUCTIONS: &str = "Suhina, an open-source clip-launching DAW. Open a Suhina project (.suhina) or a Live Set (.als) with project_open \
(paths are relative to the read root granted at launch), look at it with project_inspect, discover command ids \
and parameters with command_list, and change it with command_run or command_batch (each editing command is one \
undo step; edit.undo reverts it). Several projects can be open, like tabs: each project_open adds one and makes it \
active; projects.list, projects.switch, projects.next and projects.close manage them, and the `project` argument \
of a tool targets another open project without switching. project_render writes a WAV file below the write root; with `launches` it \
renders clip launches offline. project.save writes a .suhina file only below the write root, and replaces an existing \
file (the project's own included) only with overwrite: true in its params. Transport commands (clip.launch, scene.launch, track.stop, transport.play, \
transport.stop, transport.setPosition, launcher.state) play on a real audio device only when the server was started with --audio.";

/// Most steps a single `command_batch` may run.
const MAX_BATCH_STEPS: usize = 256;

#[derive(Clone)]
pub struct SuhinaMcp {
    host: Host,
    /// The roots `suhina-cli mcp` was given; an attached server works out its own per call.
    workspace: Arc<Workspace>,
    /// Opened projects play out loud (`--audio`): the projects have an audio system.
    audio_output: bool,
    tool_router: ToolRouter<Self>,
}

/// Where the tools' projects live.
#[derive(Clone)]
enum Host {
    /// `suhina-cli mcp`: the server's own projects.
    Own(Arc<Mutex<Projects>>),
    /// The desktop app's open tabs: tools run as jobs the window runs between frames (ADR 0028).
    App(AppHost),
}

const INSTRUCTIONS_ATTACHED: &str = "Suhina, an open-source clip-launching DAW. You are attached to the Suhina app the person is using: \
you work on the projects open in its window, they see every change as you make it, and each editing command is one step \
in the undo history of its tab (edit.undo reverts the last step, theirs or yours). Look with project_inspect, discover \
command ids and parameters with command_list, and change projects with command_run or command_batch; projects.list, \
projects.switch and the `project` argument act on the window's tabs. Tool calls wait until the person allows the \
connection. You cannot open projects from disk or close tabs, and you cannot change the app's settings. You may write \
only inside the folder of the project you act on (a project never saved has none yet) and read that folder and the \
browser's library folders. Transport commands play on the window's audio output.";

#[derive(Debug, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct OpenParams {
    /// Path of a Suhina project (`.suhina`) or a Live Set (`.als`), relative to the read root.
    pub path: String,
}

#[derive(Debug, Default, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct InspectParams {
    /// Open project id from projects.list; the active project when left out.
    #[serde(default)]
    pub project: Option<u32>,
}

#[derive(Debug, Default, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct ListParams {
    /// Only commands whose id contains this text, e.g. `track`.
    #[serde(default)]
    pub filter: Option<String>,
}

#[derive(Debug, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct RunParams {
    /// Command id from command_list, e.g. `track.setPan`.
    pub id: String,
    /// Command parameters as a JSON object, e.g. `{"track": "Bass", "pan": -1}`.
    #[serde(default)]
    pub params: Option<Value>,
    /// Open project id from projects.list to run on; the active project when left out.
    #[serde(default)]
    pub project: Option<u32>,
}

#[derive(Debug, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct StepParams {
    /// Command id from command_list, e.g. `track.setPan`.
    pub id: String,
    /// Command parameters as a JSON object.
    #[serde(default)]
    pub params: Option<Value>,
}

#[derive(Debug, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct BatchParams {
    /// Commands to run in order, e.g. `[{"id": "transport.setTempo", "params": {"bpm": 128}}]`. Each editing
    /// command is its own undo step; the batch is not one transaction.
    pub steps: Vec<StepParams>,
    /// Stop at the first failing step (default true).
    #[serde(default)]
    pub stop_on_error: Option<bool>,
    /// Open project id from projects.list to run on; the active project when left out.
    #[serde(default)]
    pub project: Option<u32>,
}

#[derive(Debug, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct RenderParams {
    /// Output `.wav` path, relative to the write root.
    pub output: String,
    /// Render this launcher scene (1-based) instead of the arrangement.
    #[serde(default)]
    pub scene: Option<usize>,
    /// Render clip launches instead: commands run at times in seconds, starting with the transport
    /// stopped, e.g. `[{"at": 0, "id": "scene.launch", "params": {"scene": 1}}]`. Needs `seconds`.
    #[serde(default)]
    pub launches: Option<Vec<LaunchParams>>,
    /// Length of a `launches` render in seconds (at most 600).
    #[serde(default)]
    pub seconds: Option<f64>,
    /// Sample rate in Hz (default 48000).
    #[serde(default)]
    pub sample_rate: Option<u32>,
    /// Replace the file if it exists (default false).
    #[serde(default)]
    pub overwrite: Option<bool>,
    /// Open project id from projects.list to render; the active project when left out.
    #[serde(default)]
    pub project: Option<u32>,
}

#[derive(Debug, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct LaunchParams {
    /// When to run the command, in seconds from the start of the render.
    pub at: f64,
    /// A transport or read command id, e.g. `clip.launch`.
    pub id: String,
    #[serde(default)]
    pub params: Option<Value>,
}

fn ok_json(value: &Value) -> CallToolResult {
    let text = serde_json::to_string_pretty(value).unwrap_or_else(|e| format!("{{\"error\": \"{e}\"}}"));
    CallToolResult::success(vec![ContentBlock::text(text)])
}

fn fail(message: impl std::fmt::Display) -> CallToolResult {
    CallToolResult::error(vec![ContentBlock::text(message.to_string())])
}

impl SuhinaMcp {
    /// A server with no project open.
    pub fn new(workspace: Workspace) -> Self {
        Self {
            host: Host::Own(Arc::new(Mutex::new(Projects::new()))),
            workspace: Arc::new(workspace),
            audio_output: false,
            tool_router: Self::tool_router(),
        }
    }

    /// A server for the projects open in the desktop app (ADR 0028), reached through `host`.
    pub fn attached(host: AppHost) -> Self {
        Self {
            host: Host::App(host),
            workspace: Arc::new(Workspace::default()),
            audio_output: false,
            tool_router: Self::tool_router(),
        }
    }

    fn is_attached(&self) -> bool {
        matches!(self.host, Host::App(_))
    }

    /// The server's own projects, for the builders below.
    fn own(&self) -> Option<std::sync::MutexGuard<'_, Projects>> {
        match &self.host {
            Host::Own(projects) => projects.lock().ok(),
            Host::App(_) => None,
        }
    }

    /// Gives agents the user's browser library (`library.*`, ADR 0012), kept up to date on a
    /// thread of its own. Agents may search it and read what it lists inside the read root.
    pub fn with_file_library(self, mut library: suhina_library::Library) -> Self {
        let _ = library.start();
        if let Some(mut projects) = self.own() {
            projects.set_file_library(library);
            // And the sample map's analysis of its samples (`soundmap.*`, ADR 0013).
            projects.start_sound_map();
        }
        self
    }

    /// Plays each opened project on `system`'s devices, so transport and launcher commands are
    /// heard and the `audio.*` commands choose the output.
    pub fn with_audio_output(mut self, system: Arc<dyn AudioSystem>) -> Self {
        let set = match self.own() {
            Some(mut projects) => {
                projects.set_audio_system(system);
                true
            }
            None => false,
        };
        self.audio_output = set;
        self
    }

    /// Hears MIDI controllers on `system`'s input ports: the armed tracks of the project that
    /// plays play them (`midi.*`, `track.setArm`, ADR 0017).
    pub fn with_midi_input(self, system: Arc<dyn MidiSystem>) -> Self {
        if let Some(mut projects) = self.own() {
            // A port that cannot be opened is listed by midi.inputs with why.
            let _ = projects.set_midi_system(system);
        }
        self
    }

    /// The file rules for a call on project `target`: the roots given at launch, or for an
    /// attached server that project's folder and the browser's library folders (ADR 0028).
    fn files(&self) -> Files {
        match self.host {
            Host::Own(_) => Files::Fixed(Arc::clone(&self.workspace)),
            Host::App(_) => Files::PerProject,
        }
    }

    /// Serves MCP over stdin and stdout until the client disconnects.
    pub async fn serve_stdio(self) -> Result<(), String> {
        let running = self.serve((tokio::io::stdin(), tokio::io::stdout())).await.map_err(|e| e.to_string())?;
        running.waiting().await.map(|_| ()).map_err(|e| e.to_string())
    }

    /// Runs `f` with the open projects on a blocking thread. A panic in a command would poison
    /// the lock, but commands restore the project on failure, so the projects stay usable.
    async fn with_projects<T: Send + 'static>(
        &self,
        f: impl FnOnce(&mut Projects) -> Result<T, String> + Send + 'static,
    ) -> Result<T, String> {
        match &self.host {
            Host::Own(projects) => {
                let projects = Arc::clone(projects);
                tokio::task::spawn_blocking(move || {
                    let mut guard = projects.lock().unwrap_or_else(PoisonError::into_inner);
                    f(&mut guard)
                })
                .await
                .unwrap_or_else(|e| Err(format!("task failed: {e}")))
            }
            Host::App(app) => app.run(f).await,
        }
    }

    /// Runs `f` with the session `project` names, or the active one.
    async fn with_session<T: Send + 'static>(
        &self,
        project: Option<u32>,
        f: impl FnOnce(&mut Session) -> Result<T, String> + Send + 'static,
    ) -> Result<T, String> {
        self.with_projects(move |projects| f(projects.session_mut(project.map(ProjectId)).map_err(|e| e.0)?)).await
    }
}

#[tool_router]
impl SuhinaMcp {
    #[tool(
        description = "Open a Suhina project (.suhina) or a Live Set (.als) below the read root and make it the active project. Returns a summary and the warnings from reading it (unsupported content is skipped, not fatal)."
    )]
    async fn project_open(&self, Parameters(p): Parameters<OpenParams>) -> Result<CallToolResult, McpError> {
        if self.is_attached() {
            return Ok(fail(
                "attached to the app, agents work on the projects open in its window; the person opens projects (projects.list shows them)",
            ));
        }
        let path = match self.workspace.readable(&p.path) {
            Ok(path) => path,
            Err(e) => return Ok(fail(e)),
        };
        let output = self.audio_output;
        let result = self
            .with_projects(move |projects| {
                let (opened, mut warnings) = Session::open(&path).map_err(|e| e.0)?;
                let id = projects.add(opened, None).map_err(|e| e.0)?;
                let session = projects.session_mut(Some(id)).map_err(|e| e.0)?;
                let mut info = session.execute("project.info", &json!({})).map_err(|e| e.0)?;
                info["source"] = json!(display_path(&path));
                let audio_output = if output { start_audio(projects, id, &mut warnings) } else { Value::Null };
                Ok(json!({ "project": id.0, "info": info, "warnings": warnings, "audio_output": audio_output }))
            })
            .await;
        Ok(result.map_or_else(fail, |v| ok_json(&v)))
    }

    #[tool(
        description = "The open project as JSON: summary (tempo, counts, undo state), tracks with mixer settings and devices, and arrangement clips."
    )]
    async fn project_inspect(&self, Parameters(p): Parameters<InspectParams>) -> Result<CallToolResult, McpError> {
        let result = self
            .with_session(p.project, |session| {
                let mut read = |id: &str| session.execute(id, &json!({})).map_err(|e| e.0);
                Ok(json!({ "info": read("project.info")?, "tracks": read("project.tracks")?, "clips": read("project.clips")? }))
            })
            .await;
        Ok(result.map_or_else(fail, |v| ok_json(&v)))
    }

    #[tool(
        description = "List command ids with a description, kind (read, edit, history, transport, projects, plugins, plugin, library, audio) and parameter schema. Run them with command_run."
    )]
    async fn command_list(&self, Parameters(p): Parameters<ListParams>) -> Result<CallToolResult, McpError> {
        let filter = p.filter.unwrap_or_default();
        let all = match all_commands() {
            Value::Array(all) => all,
            _ => Vec::new(),
        };
        let list: Vec<Value> = all
            .into_iter()
            .filter(|c| c["id"].as_str().is_some_and(|id| id.contains(filter.as_str())))
            .map(|mut c| {
                if let Some(object) = c.as_object_mut()
                    && let Some(label) = object.remove("label")
                {
                    object.insert("description".into(), label);
                }
                c
            })
            .collect();
        Ok(ok_json(&Value::Array(list)))
    }

    #[tool(
        description = "Run one command by id with its parameters. Editing commands record one undo step; on error the project is unchanged."
    )]
    async fn command_run(&self, Parameters(p): Parameters<RunParams>) -> Result<CallToolResult, McpError> {
        let attached = self.is_attached();
        if let Some(why) = refused(&p.id, p.params.as_ref(), attached) {
            return Ok(fail(why));
        }
        let output = self.audio_output;
        let files = self.files();
        let target = p.project.map(ProjectId);
        let result = self
            .with_projects(move |projects| {
                let workspace = files.workspace(projects, target);
                let source = save_source(projects, target, &p.id);
                let params = confine(&workspace, &p.id, p.params.unwrap_or_else(|| json!({})), source.as_deref())?;
                let mut value = projects.execute_in(target, &p.id, &params).map_err(|e| e.0)?;
                if let Some(audio_output) = play_active(projects, &p.id, output)
                    && let Some(object) = value.as_object_mut()
                {
                    object.insert("audio_output".into(), audio_output);
                }
                Ok(value)
            })
            .await;
        Ok(result.map_or_else(fail, |v| ok_json(&v)))
    }

    #[tool(
        description = "Run several commands in order. Each editing command is its own undo step. Returns each step's result or error; stops at the first error unless stop_on_error is false."
    )]
    async fn command_batch(&self, Parameters(p): Parameters<BatchParams>) -> Result<CallToolResult, McpError> {
        if p.steps.len() > MAX_BATCH_STEPS {
            return Ok(fail(format!("a batch may have at most {MAX_BATCH_STEPS} steps, got {}", p.steps.len())));
        }
        let stop_on_error = p.stop_on_error.unwrap_or(true);
        let output = self.audio_output;
        let attached = self.is_attached();
        let files = self.files();
        let target = p.project.map(ProjectId);
        let result = self
            .with_projects(move |projects| {
                let mut results = Vec::new();
                let mut failed = false;
                for step in p.steps {
                    let workspace = files.workspace(projects, target);
                    let source = save_source(projects, target, &step.id);
                    let why = refused(&step.id, step.params.as_ref(), attached);
                    let params =
                        confine(&workspace, &step.id, step.params.unwrap_or_else(|| json!({})), source.as_deref());
                    let outcome = match (why, params) {
                        (Some(why), _) => Err(why.into()),
                        (None, Err(e)) => Err(suhina_session::CommandError(e)),
                        (None, Ok(params)) => projects.execute_in(target, &step.id, &params),
                    };
                    if outcome.is_ok() {
                        play_active(projects, &step.id, output);
                    }
                    match outcome {
                        Ok(value) => results.push(json!({ "id": step.id, "ok": true, "result": value })),
                        Err(e) => {
                            failed = true;
                            results.push(json!({ "id": step.id, "ok": false, "error": e.0 }));
                            if stop_on_error {
                                break;
                            }
                        }
                    }
                }
                Ok((failed, Value::Array(results)))
            })
            .await;
        Ok(match result {
            Ok((false, steps)) => ok_json(&json!({ "steps": steps })),
            Ok((true, steps)) => {
                let mut r = ok_json(&json!({ "steps": steps }));
                r.is_error = Some(true);
                r
            }
            Err(e) => fail(e),
        })
    }

    #[tool(
        description = "Render the arrangement (or one launcher scene, or a script of clip launches) offline to a 32-bit float stereo WAV below the write root. Built-in effects and instruments, and installed plug-ins (found in the plug-in catalog, with the state their running instances have now, else the saved one) run as in playback; notes on a track without an instrument play as sine tones; audio clips play. `plugins` in the result lists plug-ins that are missing or did not start."
    )]
    async fn project_render(&self, Parameters(p): Parameters<RenderParams>) -> Result<CallToolResult, McpError> {
        let files = self.files();
        let output = p.output;
        let overwrite = p.overwrite.unwrap_or(false);
        let sample_rate = p.sample_rate.unwrap_or(48_000);
        let scene = p.scene;
        let launches: Option<Vec<LaunchStep>> = p.launches.map(|steps| {
            steps
                .into_iter()
                .map(|s| LaunchStep { at: s.at, id: s.id, params: s.params.unwrap_or_else(|| json!({})) })
                .collect()
        });
        let seconds = p.seconds;
        let project_id = p.project;
        // Only the snapshot is taken with the projects: attached to the app that runs on the
        // window's thread, so decoding missing files and the render run on a blocking thread.
        let prepared = self
            .with_projects(move |projects| {
                let path = files.workspace(projects, project_id.map(ProjectId)).writable(&output, overwrite)?;
                let env = projects.plugin_env();
                let session = projects.session_mut(project_id.map(ProjectId)).map_err(|e| e.0)?;
                if launches.is_some() && scene.is_some() {
                    return Err("give either scene or launches, not both".into());
                }
                if launches.is_some() && seconds.is_none() {
                    return Err("launches needs seconds, the length of the render".into());
                }
                if launches.is_none() && seconds.is_some() {
                    return Err("seconds applies only to launches".into());
                }
                let scene = scene.map(|n| n.checked_sub(1).ok_or("scenes are numbered from 1")).transpose()?;
                // The running plug-ins' state as it is now, so the render plays what is heard.
                let heard = session.project_as_heard();
                let project = match (scene, &launches) {
                    (Some(index), None) => scene_as_arrangement(&heard, index)?,
                    _ => heard,
                };
                let audio = audio_snapshot(session);
                // The NAM devices' files, read when the project opened.
                let assets = session.device_assets().clone();
                Ok((path, env, project, audio, assets, launches))
            })
            .await;
        let result = match prepared {
            Err(e) => Err(e),
            Ok((path, env, project, audio, assets, launches)) => {
                tokio::task::spawn_blocking(move || -> Result<Value, String> {
                    // Files the session has not decoded yet are decoded here, off the window's thread.
                    let (audio, warnings) = audio.complete(&project);
                    if let Some(steps) = launches {
                        let seconds = seconds.ok_or("launches needs seconds, the length of the render")?;
                        let (summary, steps) =
                            render_launch_script(&project, &audio, &path, sample_rate, &steps, seconds, Some(&env))?;
                        return Ok(json!({
                            "output": display_path(&path),
                            "warnings": warnings,
                            "plugins": summary.plugins,
                            "seconds": summary.seconds,
                            "sample_rate": summary.sample_rate,
                            "peak": summary.peak,
                            "steps": steps,
                        }));
                    }
                    let summary = render_wav(&project, &audio, &assets, &path, sample_rate, Some(&env))?;
                    Ok(json!({
                        "output": display_path(&path),
                        "warnings": warnings,
                        "plugins": summary.plugins,
                        "seconds": summary.seconds,
                        "sample_rate": summary.sample_rate,
                        "peak": summary.peak,
                    }))
                })
                .await
                .unwrap_or_else(|e| Err(format!("task failed: {e}")))
            }
        };
        Ok(result.map_or_else(fail, |v| ok_json(&v)))
    }
}

/// Starts project `id` playing on the chosen output. Returns the device description, or why it
/// is not available; missing audio files and a fallback to the default device are added to
/// `warnings`.
fn start_audio(projects: &mut Projects, id: ProjectId, warnings: &mut Vec<String>) -> Value {
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
fn project_audio(session: &Session, project: &Project) -> (suhina_media::AudioPool, Vec<String>) {
    audio_snapshot(session).complete(project)
}

/// What [`project_audio`] needs from the session, taken quickly so that decoding the files it
/// lacks can run off the window's thread.
struct AudioSnapshot {
    folder: Option<PathBuf>,
    have: suhina_media::AudioPool,
}

fn audio_snapshot(session: &Session) -> AudioSnapshot {
    let folder = session
        .audio_folder()
        .map(Path::to_path_buf)
        .or_else(|| session.source().and_then(|s| s.parent()).map(Path::to_path_buf));
    AudioSnapshot { folder, have: session.audio().clone() }
}

impl AudioSnapshot {
    /// The pool `project` plays, decoding the files the session has not, and the warnings.
    fn complete(self, project: &Project) -> (suhina_media::AudioPool, Vec<String>) {
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

/// The file `project.save` without a path would write, and whose folder `project.autosave`
/// writes into: the source of the project it runs in. `None` for every other command.
fn save_source(projects: &mut Projects, target: Option<ProjectId>, id: &str) -> Option<PathBuf> {
    if id != "project.save" && id != "project.autosave" {
        return None;
    }
    projects.session_mut(target).ok()?.source().map(Path::to_path_buf)
}

/// `params` with every path a command reads checked against the read root and made real
/// (`docs/spec/browser.md` › Commands): files for `clip.addFiles`, `preview.play` and presets,
/// folders for `library.list` and a folder place, and file items for `library.*`. The file
/// `project.save` writes is checked against the write root ([`confine_save`]); `source` is
/// the file the project was opened from or last saved to.
fn confine(workspace: &Workspace, id: &str, mut params: Value, source: Option<&Path>) -> Result<Value, String> {
    match id {
        "project.save" => confine_save(workspace, &mut params, source)?,
        "project.autosave" => confine_autosave(workspace, &params, source)?,
        "clip.addFiles" => {
            if let Some(paths) = params.get_mut("paths").and_then(Value::as_array_mut) {
                for path in paths.iter_mut() {
                    let given = path.as_str().ok_or("paths must be a list of file paths")?;
                    *path = json!(display_path(&workspace.readable(given)?));
                }
            }
        }
        "preview.play" => confine_file(workspace, &mut params, "path")?,
        "project.relinkFiles" => {
            if let Some(paths) = params.get_mut("to").and_then(Value::as_array_mut) {
                for path in paths.iter_mut() {
                    let given = path.as_str().ok_or("to must be a list of file paths")?;
                    *path = json!(display_path(&workspace.readable(given)?));
                }
            }
        }
        "library.findFiles" => {
            if let Some(folder) = params.get("folder").and_then(Value::as_str) {
                params["folder"] = json!(display_path(&workspace.readable_folder(folder)?));
            }
        }
        "device.add" => confine_file(workspace, &mut params, "preset")?,
        "track.add" => {
            confine_file(workspace, &mut params, "preset")?;
            confine_file(workspace, &mut params, "sd")?;
        }
        "device.addSd" => confine_file(workspace, &mut params, "path")?,
        "library.list" => {
            let folder = params.get("folder").and_then(Value::as_str).ok_or("folder must be a text")?;
            params["folder"] = json!(display_path(&workspace.readable_folder(folder)?));
        }
        "soundmap.analyse" => {
            if let Some(folder) = params.get("folder").and_then(Value::as_str) {
                params["folder"] = json!(display_path(&workspace.readable_folder(folder)?));
            }
        }
        "soundmap.similar" | "soundmap.info" => {
            confine_file(workspace, &mut params, "item")?;
            confine_place(workspace, &mut params)?;
        }
        "soundmap.layout" => confine_place(workspace, &mut params)?,
        "soundmap.setKind" => confine_file(workspace, &mut params, "item")?,
        "library.search" => {
            if let Some(place) = params.get("place").and_then(Value::as_str)
                && place != "pinned"
                && !place.starts_with("recent:")
            {
                params["place"] = json!(display_path(&workspace.readable_folder(place)?));
            }
        }
        "library.info" | "library.pin" | "library.unpin" | "library.tag" => {
            if let Some(item) = params.get("item").and_then(Value::as_str)
                && !item.starts_with("device:")
                && !item.starts_with("plugin:")
            {
                let (file, voice) = match item.rsplit_once('#') {
                    Some((file, voice)) if voice.parse::<usize>().is_ok() => (file, Some(voice.to_string())),
                    _ => (item, None),
                };
                let file = display_path(&workspace.readable(file)?);
                params["item"] = json!(voice.map_or(file.clone(), |v| format!("{file}#{v}")));
            }
        }
        _ => {}
    }
    Ok(params)
}

/// `project.save` writes below the write root only. Its `path` (with `.suhina` added as the
/// session adds it) must be there; without a path it saves over the project's own `.suhina` file,
/// which must be there too. Either way an existing file is
/// replaced only with `overwrite: true`, an MCP parameter that is taken out before the command
/// runs.
fn confine_save(workspace: &Workspace, params: &mut Value, source: Option<&Path>) -> Result<(), String> {
    let overwrite = match params.get("overwrite") {
        None | Some(Value::Null) => false,
        Some(Value::Bool(b)) => *b,
        Some(_) => return Err("overwrite must be true or false".into()),
    };
    if let Some(object) = params.as_object_mut() {
        object.remove("overwrite");
    }
    let mut path = match params.get("path") {
        Some(Value::String(path)) if !path.trim().is_empty() => PathBuf::from(path.trim()),
        Some(Value::String(_)) | None | Some(Value::Null) => match source {
            Some(source) if suhina_session::is_project_file(source) => source.to_path_buf(),
            // The command itself says it needs a path.
            _ => return Ok(()),
        },
        Some(_) => return Err("path must be text".into()),
    };
    if !suhina_session::is_project_file(&path) {
        let mut name = path.file_name().map(|n| n.to_os_string()).ok_or("give the project a file name")?;
        name.push(".");
        name.push(suhina_session::PROJECT_EXTENSION);
        path.set_file_name(name);
    }
    let text = path.to_str().ok_or("the path is not valid text")?;
    let target = workspace.writable(text, overwrite)?;
    params["path"] = json!(display_path(&target));
    Ok(())
}

/// `project.autosave` writes a backup into `.backup` beside the project's file
/// (`docs/spec/backups.md`), so `now: true` needs that folder below the write root; backups before
/// critical operations would write there unasked, so agents cannot choose them.
fn confine_autosave(workspace: &Workspace, params: &Value, source: Option<&Path>) -> Result<(), String> {
    if params.get("interval").and_then(Value::as_str).is_some_and(|i| i.trim() == "critical") {
        return Err("agents cannot choose backups before critical operations; use now: true".into());
    }
    if params.get("now") == Some(&Value::Bool(true)) {
        let source = source.ok_or("over MCP only a project with a file below the write root can be backed up")?;
        let folder = source.parent().filter(|p| !p.as_os_str().is_empty()).unwrap_or(Path::new("."));
        workspace.writable_folder(folder)?;
        let backups = folder.join(suhina_session::backup::FOLDER);
        if std::fs::symlink_metadata(&backups).is_ok() {
            workspace.writable_folder(&backups)?;
        }
    }
    Ok(())
}

/// A folder `place` must be below the read root; `pinned` and `recent:…` are the library's own.
fn confine_place(workspace: &Workspace, params: &mut Value) -> Result<(), String> {
    if let Some(place) = params.get("place").and_then(Value::as_str)
        && place != "pinned"
        && !place.starts_with("recent:")
    {
        params["place"] = json!(display_path(&workspace.readable_folder(place)?));
    }
    Ok(())
}

fn confine_file(workspace: &Workspace, params: &mut Value, name: &str) -> Result<(), String> {
    if let Some(given) = params.get(name).and_then(Value::as_str) {
        let path = workspace.readable(given)?;
        params[name] = json!(display_path(&path));
    }
    Ok(())
}

/// Commands agents may not run. They choose the folders whose plug-ins a scan loads and runs,
/// so an agent could otherwise add its write root and run code it wrote there. The user chooses
/// plug-in folders (`suhina-cli plugins add-folder`, later the UI); agents may still scan them.
const NOT_FOR_AGENTS: [&str; 4] =
    ["plugins.addFolder", "plugins.removeFolder", "library.addFolder", "library.removeFolder"];

/// Commands an agent attached to the app may not run (ADR 0028): closing a tab loses its unsaved
/// changes and its undo history, and the rest change the app's settings or the person's library
/// rather than a project. A scan would also run on the window's thread for as long as plug-ins
/// take to load; the person scans from the plug-in manager.
const NOT_FOR_ATTACHED: [&str; 17] = [
    "projects.close",
    "audio.setOutput",
    "output.setCuePair",
    "output.setCueVolume",
    "midi.setInput",
    "plugins.setHosting",
    "plugins.setEnabled",
    "plugins.setFolderEnabled",
    "plugins.scan",
    "plugins.scanFile",
    "library.setPluginsView",
    "library.pin",
    "library.unpin",
    "library.tag",
    "library.setTag",
    "library.removeTag",
    "soundmap.setKind",
];

/// Why an agent may not run command `id` with `params`, if it may not; `attached` when it works
/// on the app's open tabs.
fn refused(id: &str, params: Option<&Value>, attached: bool) -> Option<String> {
    if NOT_FOR_AGENTS.contains(&id) {
        return Some(if id.starts_with("library.") {
            format!("{id} is not available to agents: the user chooses the browser's folders")
        } else {
            format!("{id} is not available to agents: the user chooses plug-in folders (suhina-cli plugins add-folder)")
        });
    }
    if !attached {
        return None;
    }
    if id == "projects.close" {
        return Some(
            "agents never close a tab: closing loses its unsaved changes and its undo history; the person closes tabs"
                .into(),
        );
    }
    if NOT_FOR_ATTACHED.contains(&id) {
        return Some(format!(
            "{id} changes the app's settings or the person's library; agents attached to the app work on projects"
        ));
    }
    let sets_backups = params.is_some_and(|p| p.get("interval").is_some() || p.get("clean_up").is_some());
    if id == "project.autosave" && sets_backups {
        return Some(
            "the backup settings are the person's; agents attached to the app may only back up now (now: true)".into(),
        );
    }
    None
}

/// The file rules a call follows.
#[derive(Clone)]
enum Files {
    /// The roots `suhina-cli mcp` was given at launch.
    Fixed(Arc<Workspace>),
    /// Attached to the app: the folder of the project the call acts on, and the library to read.
    PerProject,
}

impl Files {
    fn workspace(&self, projects: &mut Projects, target: Option<ProjectId>) -> Workspace {
        match self {
            Files::Fixed(workspace) => Workspace::clone(workspace),
            Files::PerProject => {
                let folder = projects
                    .session_mut(target)
                    .ok()
                    .and_then(|s| s.source().and_then(Path::parent).map(Path::to_path_buf))
                    .filter(|f| !f.as_os_str().is_empty());
                let library: Vec<PathBuf> = projects.file_library_mut().folders().into_iter().map(|f| f.path).collect();
                Workspace::attached(folder.as_deref(), &library)
            }
        }
    }
}

/// After a project command, starts the newly active project playing when the server has audio
/// output, since switching closes the previous project's player. Returns what was started.
fn play_active(projects: &mut Projects, id: &str, output: bool) -> Option<Value> {
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

/// Adds a title and behaviour hints to each tool, and closes its schema to unknown arguments.
fn annotated(mut tool: Tool) -> Tool {
    let words = tool.name.replace('_', " ");
    let mut chars = words.chars();
    let title = chars.next().map(|c| c.to_uppercase().collect::<String>() + chars.as_str()).unwrap_or_default();
    let read_only = matches!(tool.name.as_ref(), "project_inspect" | "command_list");
    // Opening replaces the session's project; commands and renders change the project or files.
    let destructive = matches!(tool.name.as_ref(), "project_open" | "command_run" | "command_batch" | "project_render");
    tool.title = Some(title.clone());
    tool.annotations =
        Some(ToolAnnotations::from_raw(Some(title), Some(read_only), Some(destructive), Some(read_only), Some(false)));
    Arc::make_mut(&mut tool.input_schema).insert("additionalProperties".into(), json!(false));
    tool
}

impl ServerHandler for SuhinaMcp {
    fn get_info(&self) -> ServerConfig {
        ServerConfig::new(ServerCapabilities::builder().enable_tools().build())
            .with_server_info(Implementation::new("suhina", env!("CARGO_PKG_VERSION")))
            .with_instructions(if self.is_attached() { INSTRUCTIONS_ATTACHED } else { INSTRUCTIONS })
    }

    async fn list_tools(
        &self,
        _: Option<PaginatedRequestParams>,
        _: RequestContext<RoleServer>,
    ) -> Result<ListToolsResult, McpError> {
        let tools: Vec<Tool> = self.tool_router.list_all().into_iter().map(annotated).collect();
        serde_json::from_value(json!({ "tools": tools })).map_err(|e| McpError::internal_error(e.to_string(), None))
    }

    async fn call_tool(
        &self,
        request: CallToolRequestParams,
        context: RequestContext<RoleServer>,
    ) -> Result<CallToolResponse, McpError> {
        if let Some(tool) = self.tool_router.get(&request.name) {
            let properties = tool.input_schema.get("properties").and_then(Value::as_object);
            if let Some(args) = &request.arguments
                && let Some(key) = args.keys().find(|k| properties.is_none_or(|p| !p.contains_key(*k)))
            {
                return Err(McpError::invalid_params(format!("unknown argument `{key}` for {}", request.name), None));
            }
        }
        self.tool_router.call(ToolCallContext::new(self, request, context)).await
    }
}

#[cfg(test)]
mod refused_tests {
    use super::refused;

    use serde_json::json;

    #[test]
    fn agents_may_scan_but_not_choose_plugin_folders() {
        for attached in [false, true] {
            assert!(refused("plugins.addFolder", None, attached).is_some());
            assert!(refused("plugins.removeFolder", None, attached).is_some());
            for id in ["plugins.list", "plugins.folders", "plugins.check", "track.setPan"] {
                assert!(refused(id, None, attached).is_none(), "{id}");
            }
        }
        for id in ["plugins.scan", "plugins.scanFile"] {
            assert!(refused(id, None, false).is_none(), "{id}");
        }
    }

    #[test]
    fn attached_agents_never_close_a_tab_nor_change_the_apps_settings() {
        for id in [
            "projects.close",
            "audio.setOutput",
            "midi.setInput",
            "plugins.setHosting",
            "plugins.setEnabled",
            "plugins.setFolderEnabled",
            "plugins.scan",
            "plugins.scanFile",
            "library.pin",
        ] {
            assert!(refused(id, None, true).is_some(), "{id}");
            assert!(refused(id, None, false).is_none(), "{id} stays open to suhina-cli mcp");
        }
        for id in ["projects.list", "projects.switch", "transport.play", "transport.stop", "edit.undo", "project.save"]
        {
            assert!(refused(id, None, true).is_none(), "{id}");
        }
        assert!(refused("project.autosave", Some(&json!({ "now": true })), true).is_none());
        assert!(refused("project.autosave", Some(&json!({ "interval": "5m" })), true).is_some());
        assert!(refused("project.autosave", Some(&json!({ "clean_up": true })), true).is_some());
    }
}

#[cfg(test)]
mod attached_files_tests {
    use super::*;

    #[test]
    fn an_attached_agent_writes_in_the_projects_folder_and_reads_the_library_too() {
        let dir = std::env::temp_dir().join(format!("suhina-attached-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        for folder in ["song", "library/drums", "elsewhere"] {
            std::fs::create_dir_all(dir.join(folder)).unwrap();
        }
        for file in ["song/vocal.wav", "library/drums/kick.wav", "elsewhere/secret.wav"] {
            std::fs::write(dir.join(file), b"x").unwrap();
        }
        let workspace = Workspace::attached(Some(&dir.join("song")), &[dir.join("library")]);
        // Relative paths start in the project's folder; the library is read by its full path.
        assert!(workspace.readable("vocal.wav").is_ok());
        assert!(workspace.readable(dir.join("library/drums/kick.wav").to_str().unwrap()).is_ok());
        assert!(workspace.readable(dir.join("elsewhere/secret.wav").to_str().unwrap()).is_err());
        assert!(workspace.readable("../elsewhere/secret.wav").is_err());
        // It writes only in the project's folder.
        assert!(workspace.writable("mix.wav", false).is_ok());
        assert!(workspace.writable(dir.join("library/drums/new.wav").to_str().unwrap(), false).is_err());
        // A project never saved has no folder: nothing is written, the library is still read.
        let unsaved = Workspace::attached(None, &[dir.join("library")]);
        assert!(unsaved.writable("mix.wav", false).is_err());
        assert!(unsaved.readable(dir.join("library/drums/kick.wav").to_str().unwrap()).is_ok());
        let _ = std::fs::remove_dir_all(&dir);
    }
}

#[cfg(test)]
mod confine_tests {
    use super::*;

    fn roots(name: &str) -> (std::path::PathBuf, Workspace) {
        let dir = std::env::temp_dir().join(format!("suhina-confine-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(dir.join("inside/pack")).unwrap();
        std::fs::create_dir_all(dir.join("outside")).unwrap();
        for file in ["inside/pack/kick.wav", "inside/bank.syx", "outside/secret.wav"] {
            std::fs::write(dir.join(file), b"x").unwrap();
        }
        let workspace = Workspace::new(Some(&dir.join("inside")), None).unwrap();
        (dir, workspace)
    }

    #[test]
    fn paths_agents_give_must_be_inside_the_read_root() {
        let (dir, workspace) = roots("paths");
        let ok =
            confine(&workspace, "clip.addFiles", json!({ "paths": ["pack/kick.wav"], "start": 0.0 }), None).unwrap();
        assert!(ok["paths"][0].as_str().unwrap().ends_with("kick.wav"));
        assert!(Path::new(ok["paths"][0].as_str().unwrap()).is_absolute());
        let outside = dir.join("outside").join("secret.wav").display().to_string();
        for (id, params) in [
            ("clip.addFiles", json!({ "paths": [outside.clone()] })),
            ("clip.addFiles", json!({ "paths": ["../outside/secret.wav"] })),
            ("preview.play", json!({ "path": outside.clone() })),
            ("device.add", json!({ "track": 1, "preset": outside.clone() })),
            ("library.list", json!({ "folder": dir.join("outside").display().to_string() })),
            ("library.search", json!({ "place": dir.join("outside").display().to_string() })),
            ("library.info", json!({ "item": outside.clone() })),
            ("library.tag", json!({ "item": format!("{outside}#3"), "tag": "Red", "on": true })),
            ("soundmap.layout", json!({ "place": dir.join("outside").display().to_string() })),
            ("soundmap.similar", json!({ "item": outside.clone() })),
            ("soundmap.info", json!({ "item": outside.clone() })),
            ("soundmap.setKind", json!({ "item": outside.clone(), "kind": "kick" })),
            ("soundmap.analyse", json!({ "folder": dir.join("outside").display().to_string() })),
            ("project.relinkFiles", json!({ "files": ["a.wav"], "to": [outside.clone()] })),
            ("library.findFiles", json!({ "names": ["a.wav"], "folder": dir.join("outside").display().to_string() })),
        ] {
            assert!(confine(&workspace, id, params.clone(), None).is_err(), "{id} {params}");
        }
        let kept = json!({ "item": "device:overdrive" });
        assert_eq!(confine(&workspace, "library.pin", kept.clone(), None).unwrap(), kept);
        let place = json!({ "place": "recent:samples" });
        assert_eq!(confine(&workspace, "library.search", place.clone(), None).unwrap(), place);
        assert_eq!(confine(&workspace, "soundmap.layout", place.clone(), None).unwrap(), place);
        let voice = confine(&workspace, "library.info", json!({ "item": "bank.syx#4" }), None).unwrap();
        assert!(voice["item"].as_str().unwrap().ends_with("bank.syx#4"));
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn project_autosave_writes_only_below_the_write_root_and_never_unasked() {
        let (dir, read_only) = roots("autosave");
        std::fs::create_dir_all(dir.join("out")).unwrap();
        let workspace = Workspace::new(Some(&dir.join("inside")), Some(&dir.join("out"))).unwrap();
        let autosave =
            |w: &Workspace, params: Value, source: Option<&Path>| confine(w, "project.autosave", params, source);
        let inside = dir.join("inside").join("song.suhina");
        let out = dir.join("out").join("song.suhina");
        // Reporting is always fine.
        assert!(autosave(&read_only, json!({}), Some(&inside)).is_ok());
        assert!(autosave(&workspace, json!({ "interval": "5m", "clean_up": false }), None).is_ok());
        // Backups before critical operations would write beside the project unasked.
        assert!(autosave(&workspace, json!({ "interval": "critical" }), Some(&out)).is_err());
        // Now writes beside the project's file, which must be below the write root.
        assert!(autosave(&workspace, json!({ "now": true }), None).is_err(), "no file of its own");
        assert!(autosave(&workspace, json!({ "now": true }), Some(&inside)).is_err());
        assert!(autosave(&read_only, json!({ "now": true }), Some(&out)).is_err());
        assert!(autosave(&workspace, json!({ "now": true }), Some(&out)).is_ok());
        #[cfg(unix)]
        {
            // A .backup that leads outside is refused.
            std::os::unix::fs::symlink(dir.join("inside"), dir.join("out").join(".backup")).unwrap();
            assert!(autosave(&workspace, json!({ "now": true }), Some(&out)).is_err());
        }
        let _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    fn project_save_writes_only_below_the_write_root() {
        let (dir, read_only) = roots("save");
        std::fs::create_dir_all(dir.join("out")).unwrap();
        let workspace = Workspace::new(Some(&dir.join("inside")), Some(&dir.join("out"))).unwrap();
        let save = |w: &Workspace, params: Value, source: Option<&Path>| confine(w, "project.save", params, source);

        // Outside the write root, by an absolute path or by '..'.
        let outside = dir.join("outside").join("song.suhina").display().to_string();
        assert!(save(&workspace, json!({ "path": outside }), None).is_err());
        assert!(save(&workspace, json!({ "path": "../outside/song" }), None).is_err());
        // Without a write root nothing is written, with a path or over the project's own file.
        assert!(save(&read_only, json!({ "path": "song.suhina" }), None).is_err());
        let own = dir.join("inside").join("own.suhina");
        std::fs::write(&own, b"x").unwrap();
        let own = own.canonicalize().unwrap();
        assert!(save(&read_only, json!({}), Some(&own)).is_err());
        assert!(save(&read_only, json!({ "overwrite": true }), Some(&own)).is_err());
        // Nor over a project file outside the write root.
        assert!(save(&workspace, json!({ "overwrite": true }), Some(&own)).is_err());

        // Below the write root it works; .suhina is added and `overwrite` is taken out.
        let ok = save(&workspace, json!({ "path": "song", "copy": true }), None).unwrap();
        let written = ok["path"].as_str().unwrap();
        assert!(written.ends_with("song.suhina"), "{written}");
        assert!(Path::new(written).is_absolute());
        assert_eq!(ok["copy"], true);

        // An existing file is replaced only with overwrite: true, also the project's own file.
        let mine = dir.join("out").join("song.suhina");
        std::fs::write(&mine, b"x").unwrap();
        assert!(save(&workspace, json!({ "path": "song.suhina" }), None).is_err());
        let ok = save(&workspace, json!({ "path": "song.suhina", "overwrite": true }), None).unwrap();
        assert!(ok.get("overwrite").is_none());
        let mine = mine.canonicalize().unwrap();
        assert!(save(&workspace, json!({}), Some(&mine)).is_err());
        let ok = save(&workspace, json!({ "overwrite": true }), Some(&mine)).unwrap();
        assert!(ok["path"].as_str().unwrap().ends_with("song.suhina"));
        assert!(save(&workspace, json!({ "path": "x", "overwrite": "yes" }), None).is_err());

        // A project without a project file and no path is left to the command, which asks for one.
        assert_eq!(save(&workspace, json!({}), None).unwrap(), json!({}));
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn agents_may_search_the_library_but_not_choose_its_folders() {
        for attached in [false, true] {
            assert!(refused("library.addFolder", None, attached).is_some());
            assert!(refused("library.removeFolder", None, attached).is_some());
            for id in ["library.search", "library.refresh", "library.folders", "clip.addFiles", "preview.play"] {
                assert!(refused(id, None, attached).is_none(), "{id}");
            }
        }
    }
}
