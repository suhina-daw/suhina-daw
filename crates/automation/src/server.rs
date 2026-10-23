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
    Agents, AudioSystem, CommandKind, MidiSystem, Player, ProjectId, Projects, Session, all_commands, project_commands,
};

use crate::attach::AppHost;
use crate::confine::confine;
use crate::render::{LaunchStep, render_launch_script, render_wav, scene_as_arrangement};
use crate::workspace::{Workspace, display_path};

const INSTRUCTIONS: &str = "Suhina, an open-source clip-launching DAW. Open a Suhina project (.suhina) or a Live Set (.als) with project_open \
(paths are relative to the read root granted at launch), look at it with project_inspect, discover command ids \
and parameters with command_list, and change it with command_run or command_batch (each editing command is one \
undo step; edit.undo reverts it). Several projects can be open, like tabs: each project_open adds one and makes it \
active; project.list, project.switch, project.next and project.close manage them, and the `project` argument \
of a tool targets another open project without switching. project_render writes a WAV file below the write root; with `launches` it \
renders clip launches offline. project.save writes a .suhina file only below the write root, and replaces an existing \
file (the project's own included) only with overwrite: true in its params. Transport commands (clip.launch, scene.launch, track.stop, transport.play, \
transport.stop, transport.setPosition, launcher.state) play on a real audio device only when the server was started with --audio.";

/// Most steps a single `command_batch` may run.
const MAX_BATCH_STEPS: usize = 256;

#[cfg(test)]
mod export_tests;

#[derive(Clone)]
pub struct SuhinaMcp {
    host: Host,
    /// The roots `suhina-cli mcp` was given; an attached server works out its own per call.
    workspace: Arc<Workspace>,
    /// Opened projects play out loud (`--audio`): the projects have an audio system.
    audio_output: bool,
    tool_router: ToolRouter<Self>,
}

/// How often a server of its own takes in the plug-ins that started
/// ([`SuhinaMcp::keep_plugins_running`]).
const PLUGIN_TICK: std::time::Duration = std::time::Duration::from_millis(50);

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
command ids and parameters with command_list, and change projects with command_run or command_batch; project.list, \
project.switch and the `project` argument act on the window's tabs. Tool calls wait until the person allows the \
connection. You cannot open projects from disk or close tabs, and you cannot change the app's settings. You may write \
inside folders allowed by the person in Settings > Export (by default the project's folder), and read the project's folder and the \
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
    /// Open project id from project.list; the active project when left out.
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
    /// Open project id from project.list to run on; the active project when left out.
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
    /// Open project id from project.list to run on; the active project when left out.
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
    /// Open project id from project.list to render; the active project when left out.
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
        let ticking = self.keep_plugins_running();
        let running = self.serve((tokio::io::stdin(), tokio::io::stdout())).await.map_err(|e| e.to_string())?;
        let served = running.waiting().await.map(|_| ()).map_err(|e| e.to_string());
        if let Some(ticking) = ticking {
            ticking.abort();
        }
        served
    }

    /// Takes in the plug-ins that started since the last look, every [`PLUGIN_TICK`], as the
    /// desktop app does every frame: between an agent's commands (or with only `project.*` (on the open projects)
    /// ones, which do not look) they stayed loading, and the project played without them.
    /// Skips a tick while a command holds the projects. `None` when attached to the app, whose
    /// window does it.
    fn keep_plugins_running(&self) -> Option<tokio::task::JoinHandle<()>> {
        let Host::Own(projects) = &self.host else { return None };
        let projects = Arc::clone(projects);
        Some(tokio::spawn(async move {
            let mut tick = tokio::time::interval(PLUGIN_TICK);
            tick.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Delay);
            loop {
                tick.tick().await;
                let projects = Arc::clone(&projects);
                let _ = tokio::task::spawn_blocking(move || {
                    if let Ok(mut projects) = projects.try_lock() {
                        projects.poll_plugins();
                    }
                })
                .await;
            }
        }))
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
        self.with_projects(move |projects| f(projects.session_mut(project.map(ProjectId)).map_err(String::from)?)).await
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
                "attached to the app, agents work on the projects open in its window; the person opens projects (project.list shows them)",
            ));
        }
        let path = match self.workspace.readable(&p.path) {
            Ok(path) => path,
            Err(e) => return Ok(fail(e)),
        };
        let output = self.audio_output;
        let result = self
            .with_projects(move |projects| {
                let (opened, mut warnings) = Session::open(&path).map_err(String::from)?;
                let id = projects.add(opened, None).map_err(String::from)?;
                let session = projects.session_mut(Some(id)).map_err(String::from)?;
                let mut info = session.execute("project.info", &json!({})).map_err(String::from)?;
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
                let mut read = |id: &str| session.execute(id, &json!({})).map_err(String::from);
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
        Ok(self.run_command(p).await.map_or_else(fail, |v| ok_json(&v)))
    }

    /// Shared direct/batch dispatch. An attached export prepares across frames, then renders
    /// without holding the window's projects or any session lock.
    async fn run_command(&self, p: RunParams) -> Result<Value, String> {
        let attached = self.is_attached();
        if let Some(why) = refused(&p.id, p.params.as_ref(), attached) {
            return Err(why);
        }
        if p.id == "project.export"
            && let Host::App(app) = &self.host
        {
            let mut request = ExportRequest {
                target: p.project.map(ProjectId),
                params: p.params.unwrap_or_else(|| json!({})),
                pending: None,
            };
            let (data, params) = app.poll(move |projects, cancelled| request.poll(projects, cancelled)).await?;
            return tokio::task::spawn_blocking(move || {
                suhina_session::export_from(&data, &params).map_err(String::from)
            })
            .await
            .map_err(|e| format!("export worker failed: {e}"))?;
        }
        let output = self.audio_output;
        let files = self.files();
        let target = p.project.map(ProjectId);
        self.with_projects(move |projects| run_step(projects, &files, target, output, &p.id, p.params)).await
    }

    #[tool(
        description = "Run several commands in order. Each editing command is its own undo step. Returns each step's result or error; stops at the first error unless stop_on_error is false."
    )]
    async fn command_batch(&self, Parameters(p): Parameters<BatchParams>) -> Result<CallToolResult, McpError> {
        if p.steps.len() > MAX_BATCH_STEPS {
            return Ok(fail(format!("a batch may have at most {MAX_BATCH_STEPS} steps, got {}", p.steps.len())));
        }
        let stop_on_error = p.stop_on_error.unwrap_or(true);
        if self.is_attached() && p.steps.iter().any(|step| step.id == "project.export") {
            let mut results = Vec::new();
            let mut failed = false;
            for step in p.steps {
                let id = step.id.clone();
                match self.run_command(RunParams { id: step.id, params: step.params, project: p.project }).await {
                    Ok(value) => results.push(json!({ "id": id, "ok": true, "result": value })),
                    Err(error) => {
                        failed = true;
                        results.push(json!({ "id": id, "ok": false, "error": error }));
                        if stop_on_error {
                            break;
                        }
                    }
                }
            }
            let mut result = ok_json(&json!({ "steps": results }));
            if failed {
                result.is_error = Some(true);
            }
            return Ok(result);
        }
        let output = self.audio_output;
        let attached = self.is_attached();
        let files = self.files();
        let target = p.project.map(ProjectId);
        let result = self
            .with_projects(move |projects| {
                let mut results = Vec::new();
                let mut failed = false;
                for step in p.steps {
                    let outcome = match refused(&step.id, step.params.as_ref(), attached) {
                        Some(why) => Err(why),
                        None => run_step(projects, &files, target, output, &step.id, step.params),
                    };
                    match outcome {
                        Ok(value) => results.push(json!({ "id": step.id, "ok": true, "result": value })),
                        Err(e) => {
                            failed = true;
                            results.push(json!({ "id": step.id, "ok": false, "error": e }));
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
                let workspace = files.workspace(projects, project_id.map(ProjectId));
                let path = workspace.writable(&output, overwrite)?;
                let env = projects.plugin_env();
                let session = projects.session_mut(project_id.map(ProjectId)).map_err(String::from)?;
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
                Ok((path, workspace, env, project, audio, assets, launches))
            })
            .await;
        let result = match prepared {
            Err(e) => Err(e),
            Ok((path, workspace, env, project, audio, assets, launches)) => {
                tokio::task::spawn_blocking(move || -> Result<Value, String> {
                    // Files the session has not decoded yet are decoded here, off the window's thread.
                    let (audio, warnings) = audio.complete(&project);
                    if let Some(steps) = launches {
                        let seconds = seconds.ok_or("launches needs seconds, the length of the render")?;
                        let (summary, steps) = render_launch_script(
                            &project,
                            &audio,
                            &path,
                            sample_rate,
                            &steps,
                            seconds,
                            Some(&env),
                            Some(&workspace),
                        )?;
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

/// The file `project.save` without a path would write, and whose folder `project.autosave` and
/// `project.collect` without a folder write into: the source of the project it runs in. `None` for every other command.
fn save_source(projects: &mut Projects, target: Option<ProjectId>, id: &str) -> Option<PathBuf> {
    if !matches!(id, "project.save" | "project.autosave" | "project.collect") {
        return None;
    }
    projects.session_mut(target).ok()?.source().map(Path::to_path_buf)
}

/// Why an agent may not run command `id` with `params`, if it may not; `attached` when it works
/// on the app's open tabs.
fn refused(id: &str, params: Option<&Value>, attached: bool) -> Option<String> {
    match suhina_session::policy(id).map(|p| p.agents) {
        Some(Agents::Denied(why)) => return Some(format!("{id} is not available to agents: {why}")),
        Some(Agents::NotWhenAttached(why)) if attached => {
            // A reason that is a whole sentence of its own is given as it is.
            return Some(if why == Agents::SETTINGS { format!("{id} {why}") } else { why.to_string() });
        }
        _ => {}
    }
    if !attached {
        return None;
    }
    let sets_backups = params.is_some_and(|p| p.get("interval").is_some() || p.get("cleanUp").is_some());
    if id == "project.autosave" && sets_backups {
        return Some(
            "the backup settings are the person's; agents attached to the app may only back up now (now: true)".into(),
        );
    }
    None
}

/// One attached export's identity and authorized destination, fixed on its first frame.
struct ExportRequest {
    target: Option<ProjectId>,
    params: Value,
    pending: Option<(ProjectId, std::time::Instant)>,
}

impl ExportRequest {
    /// One bounded window-frame step; cancellation releases only this request's gather.
    fn poll(
        &mut self,
        projects: &mut Projects,
        cancelled: bool,
    ) -> Option<Result<(suhina_session::ExportData, Value), String>> {
        if cancelled {
            self.cancel(projects);
            return Some(Err("the export request was cancelled".into()));
        }
        let result = self.advance(projects);
        match result {
            Ok(None) => None,
            Ok(Some(data)) => Some(Ok((data, self.params.clone()))),
            Err(error) => {
                self.cancel(projects);
                Some(Err(error))
            }
        }
    }

    fn advance(&mut self, projects: &mut Projects) -> Result<Option<suhina_session::ExportData>, String> {
        let id =
            self.pending.map(|(id, _)| id).or(self.target).or_else(|| projects.active()).ok_or("no project is open")?;
        let workspace = Files::PerProject.workspace(projects, Some(id));
        // Permission changes during preparation apply before any worker can write.
        self.params = confine(&workspace, "project.export", self.params.clone(), None)?;
        let env = projects.plugin_env();
        let session = projects.get_mut(id).ok_or("the export's project was closed")?;
        session.set_plugin_env(env);
        let gathered = if let Some((_, deadline)) = self.pending {
            session.gather_export_data(deadline)
        } else {
            let deadline = std::time::Instant::now() + std::time::Duration::from_secs(1);
            let gathered = session.begin_export_gather(deadline).map_err(String::from)?;
            self.pending = Some((id, deadline));
            gathered
        };
        match gathered {
            suhina_session::ExportGather::Pending => Ok(None),
            suhina_session::ExportGather::Ready(data) => {
                self.pending = None;
                Ok(Some(*data))
            }
        }
    }

    fn cancel(&mut self, projects: &mut Projects) {
        if let Some((id, _)) = self.pending.take()
            && let Some(session) = projects.get_mut(id)
        {
            session.cancel_export_gather();
        }
    }
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
                let (include_project, folders) = projects.mcp_write_folders();
                Workspace::attached(folder.as_deref(), &library).with_write_folders(include_project, folders)
            }
        }
    }
}

/// One step of `command_run` or `command_batch`, on the projects, once the agent may run it:
/// its paths confined to the agent's roots, the command run, and a project it switched to started
/// playing, which the result names as `audio_output`.
fn run_step(
    projects: &mut Projects,
    files: &Files,
    target: Option<ProjectId>,
    output: bool,
    id: &str,
    params: Option<Value>,
) -> Result<Value, String> {
    let workspace = files.workspace(projects, target);
    let source = save_source(projects, target, id);
    let params = confine(&workspace, id, params.unwrap_or_else(|| json!({})), source.as_deref())?;
    let mut value = projects.execute_in(target, id, &params).map_err(String::from)?;
    if let Some(audio_output) = play_active(projects, id, output)
        && let Some(object) = value.as_object_mut()
    {
        object.insert("audio_output".into(), audio_output);
    }
    Ok(value)
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
            assert!(refused("plugin.addFolder", None, attached).is_some());
            assert!(refused("plugin.removeFolder", None, attached).is_some());
            for id in ["plugin.list", "plugin.folders", "plugin.check", "track.setPan"] {
                assert!(refused(id, None, attached).is_none(), "{id}");
            }
        }
        for id in ["plugin.scan", "plugin.scanFile"] {
            assert!(refused(id, None, false).is_none(), "{id}");
        }
    }

    #[test]
    fn attached_agents_never_close_a_tab_nor_change_the_apps_settings() {
        for id in [
            "project.close",
            "audio.setOutput",
            "midi.setInput",
            "plugin.setHosting",
            "plugin.setEnabled",
            "plugin.setFolderEnabled",
            "plugin.scan",
            "plugin.scanFile",
            "library.pin",
        ] {
            assert!(refused(id, None, true).is_some(), "{id}");
            assert!(refused(id, None, false).is_none(), "{id} stays open to suhina-cli mcp");
        }
        for id in ["project.list", "project.switch", "transport.play", "transport.stop", "edit.undo", "project.save"] {
            assert!(refused(id, None, true).is_none(), "{id}");
        }
        assert!(refused("project.autosave", Some(&json!({ "now": true })), true).is_none());
        assert!(refused("project.autosave", Some(&json!({ "interval": "5m" })), true).is_some());
        assert!(refused("project.autosave", Some(&json!({ "cleanUp": true })), true).is_some());
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

    /// Which parameters are places on disk is their type, [`ParamType::Path`], and [`confine`]
    /// checks every one: given a path outside the agent's roots, each must be refused, unless the
    /// whole command is refused to agents. A text parameter named like a file or a folder must be
    /// a `Path` too, or be listed here with why it is not one.
    #[test]
    fn every_path_parameter_is_confined_and_every_file_or_folder_parameter_is_a_path() {
        use crate::confine::confine;
        use crate::workspace::Workspace;
        use serde_json::Value;
        use suhina_session::{ParamType, PathKind};

        // Parameters of these commands name something other than a place on disk to read or write.
        const NOT_A_PLACE: [(&str, &str); 3] = [
            ("plugin.setFolderEnabled", "toggles a folder the user added; touches no files"),
            ("plugin.scanFile", "reads again a file in the plug-in catalog; any other is refused"),
            ("library.refresh", "reads again a folder the user added; any other is refused"),
        ];
        let dir = std::env::temp_dir().join(format!("suhina-confine-sweep-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(dir.join("inside")).unwrap();
        std::fs::create_dir_all(dir.join("outside")).unwrap();
        let workspace = Workspace::new(Some(&dir.join("inside")), None).unwrap();
        let outside = dir.join("outside").join("secret.wav").display().to_string();
        let (mut unconfined, mut untyped) = (Vec::new(), Vec::new());
        for command in suhina_session::all_commands().as_array().unwrap() {
            let id = command["id"].as_str().unwrap();
            if refused(id, None, false).is_some() {
                continue;
            }
            for param in suhina_session::params_of(id).unwrap() {
                let named_like_a_path =
                    ["path", "paths", "folder", "file", "to", "item", "preset", "sd", "output", "place"]
                        .contains(&param.name);
                let ParamType::Path(kind) = param.ty else {
                    let text = matches!(param.ty, ParamType::Text | ParamType::Texts);
                    if named_like_a_path && text && !NOT_A_PLACE.iter().any(|(c, _)| *c == id) {
                        untyped.push(format!("{id} {}", param.name));
                    }
                    continue;
                };
                let value = if kind == PathKind::Files { json!([outside]) } else { json!(outside) };
                let mut params = serde_json::Map::new();
                params.insert(param.name.to_string(), value);
                // Other required parameters are not looked at by confine, only this one.
                if confine(&workspace, id, Value::Object(params), None).is_ok() {
                    unconfined.push(format!("{id} {}", param.name));
                }
            }
        }
        let _ = std::fs::remove_dir_all(&dir);
        assert!(unconfined.is_empty(), "these path parameters pass `confine` an outside path: {unconfined:?}");
        assert!(
            untyped.is_empty(),
            "these parameters are named like a file or a folder but are text: make them ParamType::Path, or say \
             here why they are not a place: {untyped:?}"
        );
    }

    /// Every command a policy names exists: a policy is on the command's spec, so this holds by
    /// construction, and the refusals an agent gets come from it.
    #[test]
    fn denied_and_attached_refusals_come_from_the_commands_policy() {
        for command in suhina_session::all_commands().as_array().unwrap() {
            let id = command["id"].as_str().unwrap();
            let policy = suhina_session::policy(id).unwrap();
            assert_eq!(
                refused(id, None, false).is_some(),
                matches!(policy.agents, suhina_session::Agents::Denied(_)),
                "{id}"
            );
            if matches!(policy.agents, suhina_session::Agents::NotWhenAttached(_)) {
                assert!(refused(id, None, true).is_some(), "{id}");
            }
        }
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
