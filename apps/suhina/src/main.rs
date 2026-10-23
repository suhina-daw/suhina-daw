// Copyright (C) 2026 The Suhina contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! Suhina, the desktop app: opens a project, plays it on the default audio output and shows it
//! in the main window (`suhina-ui`).

// Release builds on Windows start without a console window.
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

#[cfg(target_os = "macos")]
mod mac_menu;
mod recent;
mod save_error;
mod theme_files;

use std::path::{Path, PathBuf};
use std::process::ExitCode;

use suhina_session::{OutputChoice, Session};
use suhina_ui::app::splash::Splash;
use suhina_ui::settings::keymap::Keymap;
use suhina_ui::settings::settings_view::About;
use suhina_ui::settings::themes::FileAction;
use suhina_ui::settings::{Graphics, Settings};
use suhina_ui::{App, NewProject, OpenAs, Status};

const USAGE: &str = "\
usage: suhina [<project.suhina> | <project.als> | --demo] [--renderer wgpu|glow] [--no-animations]
              [--new-instance] [--trace <file>]

  <project.suhina>   a Suhina project to open, or a Live Set (.als) to import
  --demo             open a small built-in project
  --renderer         how the window is drawn this time: wgpu or glow (OpenGL); the
                     default is the one chosen in Options > Settings
  --no-animations    turn the interface's animations off this time
  --new-instance     start another window even though this build is running (for
                     development); it serves no agents
  --trace <file>     append a diagnostic log to <file>, the plug-in hosts' too (for
                     development, docs/debugging.md); SUHINA_TRACE=<file> does the same
While this build of Suhina is running, starting it again opens the project in the running
window as a tab instead. Agents reach the running window with suhina-cli mcp --attach once
Options > Settings > General > Let agents work on open projects is on.
Without a project the window opens what Options > Settings > General > At start says (the
New or open dialog unless changed); Ctrl+T makes a new one or opens one, and a .suhina
or .als file dropped on it opens.
Settings are kept in settings.txt, the keys in keymap.json and themes in themes/<name>.json,
in Suhina's folder of the user's app data.";

/// The folder eframe keeps Suhina's window state in; the settings file lives beside it.
const APP_ID: &str = "Suhina";
/// The app id before the program was renamed: where its settings were kept.
const OLD_APP_ID: &str = "Loopwerk"; // rename_suhina: keep
const SETTINGS_FILE: &str = "settings.txt";
const KEYMAP_FILE: &str = "keymap.json";
const THEMES_FOLDER: &str = "themes";
const RECENT_FILE: &str = "recent.txt";

#[derive(Debug, PartialEq)]
enum Source {
    Nothing,
    Demo,
    Als(PathBuf),
}

#[derive(Debug, PartialEq)]
struct Args {
    source: Source,
    /// Chosen on the command line for this run, over the saved setting.
    renderer: Option<eframe::Renderer>,
    animations: bool,
    /// Start even though this build is running (ADR 0028).
    new_instance: bool,
}

/// The build's endpoints (ADR 0028), or `None` when another app of this build is running and has
/// taken the project, so this one exits. A start with `--new-instance`, or one whose endpoints
/// cannot be made, runs without them.
fn endpoints(
    args: &Args,
    repaint: &std::sync::Arc<std::sync::OnceLock<eframe::egui::Context>>,
) -> Result<Option<suhina_automation::AppEndpoints>, ()> {
    if args.new_instance {
        return Ok(None);
    }
    let Ok(folder) = suhina_automation::endpoint::build_folder() else { return Ok(None) };
    let repaint = std::sync::Arc::clone(repaint);
    let wake: suhina_automation::Wake = std::sync::Arc::new(move || {
        if let Some(ctx) = repaint.get() {
            ctx.request_repaint();
        }
    });
    match suhina_automation::AppEndpoints::start(&folder, wake) {
        Ok(endpoints) => Ok(Some(endpoints)),
        Err(suhina_automation::StartError::Running(running)) => {
            let files: Vec<PathBuf> = match &args.source {
                Source::Als(path) => vec![path.clone()],
                Source::Nothing | Source::Demo => Vec::new(),
            };
            match running.hand_over(&files) {
                Ok(()) => Err(()),
                Err(e) => {
                    eprintln!("warning: Suhina is running but did not take the project ({e}); starting another window");
                    Ok(None)
                }
            }
        }
        Err(suhina_automation::StartError::Io(e)) => {
            eprintln!("warning: agents cannot connect this time: {e}");
            Ok(None)
        }
    }
}

fn main() -> ExitCode {
    let args = match parse_args(&std::env::args().skip(1).collect::<Vec<_>>()) {
        Ok(Some(args)) => args,
        Ok(None) => {
            println!("{USAGE}");
            return ExitCode::SUCCESS;
        }
        Err(message) => {
            eprintln!("error: {message}\n\n{USAGE}");
            return ExitCode::from(2);
        }
    };
    // One app per build: a second start hands its project to the first and exits.
    let repaint = std::sync::Arc::new(std::sync::OnceLock::new());
    let Ok(agents) = endpoints(&args, &repaint) else { return ExitCode::SUCCESS };
    // Settings kept under the program's old name are copied to the new one on the first start.
    let old_folders = [
        (suhina_plugins::library::old_settings_folder(), suhina_plugins::library::new_settings_folder()),
        (eframe::storage_dir(OLD_APP_ID), eframe::storage_dir(APP_ID)),
    ];
    let copied: Vec<String> =
        old_folders.into_iter().filter_map(|(old, new)| suhina_plugins::library::adopt_old_folder(old, new)).collect();
    let (settings, mut settings_warnings) = load_settings();
    settings_warnings.extend(copied);
    let (keymap, keymap_warnings) = load_keymap();
    settings_warnings.extend(keymap_warnings);
    let renderer = args.renderer.unwrap_or(match settings.graphics {
        Graphics::Gpu => eframe::Renderer::Wgpu,
        Graphics::OpenGl => eframe::Renderer::Glow,
    });
    let graphics_now = match renderer {
        eframe::Renderer::Glow => Graphics::OpenGl,
        eframe::Renderer::Wgpu => Graphics::Gpu,
    };
    let mut themes = theme_files::ThemeFiles::new(eframe::storage_dir(app_id()).map(|d| d.join(THEMES_FOLDER)));
    let (theme_list, theme_warnings) = themes.read_all();
    settings_warnings.extend(theme_warnings);
    let recent = recent::Recent::load(eframe::storage_dir(app_id()).map(|d| d.join(RECENT_FILE)));
    let mut window = Window {
        app: App::new(),
        agents,
        graphics_now,
        editors_embeddable: false,
        themes,
        recent,
        #[cfg(target_os = "macos")]
        mac_menu: None,
        splash: Some(Splash::new(env!("CARGO_PKG_VERSION"))),
        start_up: Some(StartUp {
            source: args.source,
            output_choice: settings.output_choice(),
            warnings: settings_warnings,
        }),
    };
    // The audio devices, and the output the settings choose (Settings › Audio, `audio.*`).
    window.app.projects_mut().set_audio_system(std::sync::Arc::new(suhina_output::DeviceOutput::new()));
    window.app.projects_mut().set_output_choice(settings.output_choice());
    // The MIDI controllers, every port on but the ones the settings turn off (Settings › MIDI,
    // `midi.*`, ADR 0017).
    window.app.projects_mut().set_midi_inputs_off(settings.midi_inputs_off.clone());
    let midi_warnings = window.app.projects_mut().set_midi_system(std::sync::Arc::new(suhina_midi::Midi::new()));
    if let Some(start_up) = window.start_up.as_mut() {
        start_up.warnings.extend(midi_warnings);
    }
    let (projects, templates) = default_folders();
    window.app.set_default_folders(projects, templates);
    // The NAM devices' folders: the defaults exist from the start, so a user's first step is
    // only to drop files in ([docs/spec/nam-amp-and-cab.md]).
    let nam_base = documents_base();
    let (nam_models, irs) = (nam_base.join("NAM models"), nam_base.join("IRs"));
    let _ = std::fs::create_dir_all(&nam_models);
    let _ = std::fs::create_dir_all(&irs);
    window.app.set_nam_default_folders(nam_models, irs);
    // Backups of projects never saved, and the journal of the newest backups, beside the
    // settings; what a crash left there is offered once the window starts (`docs/spec/backups.md`).
    let backups = eframe::storage_dir(app_id()).map(|d| d.join(suhina_session::backup::SETTINGS_SUBFOLDER));
    window.app.projects_mut().set_backup_folder(backups);

    let options = eframe::NativeOptions {
        viewport: eframe::egui::ViewportBuilder::default()
            .with_title("Suhina")
            .with_inner_size([1280.0, 800.0])
            .with_min_inner_size([640.0, 400.0])
            .with_drag_and_drop(true),
        renderer,
        event_loop_builder: x11_first(),
        ..Default::default()
    };
    let animations = args.animations;
    let result = eframe::run_native(
        app_id(),
        options,
        Box::new(move |cc| {
            // Agents' work and later starts' projects ask the window for a frame through it.
            let _ = repaint.set(cc.egui_ctx.clone());
            suhina_ui::kit::theme::install(&cc.egui_ctx);
            let owner = native_window(cc);
            window.editors_embeddable = owner.is_some();
            window.app.set_editor_owner(owner);
            window.app.set_settings(&cc.egui_ctx, settings);
            let folder = window.themes.folder_text();
            window.app.set_themes(&cc.egui_ctx, theme_list, folder);
            window.app.set_keymap(keymap);
            if !animations {
                App::set_animations(&cc.egui_ctx, false);
            }
            window.share_about();
            #[cfg(target_os = "macos")]
            {
                window.mac_menu = mac_menu::install(&suhina_ui::settings::keymap::actions(), cc.egui_ctx.clone());
            }
            Ok(Box::new(window))
        }),
    );
    match result {
        Ok(()) => ExitCode::SUCCESS,
        Err(e) => {
            eprintln!("error: {e}");
            ExitCode::FAILURE
        }
    }
}

/// `Ok(None)` asks for the usage text.
fn parse_args(args: &[String]) -> Result<Option<Args>, String> {
    let mut parsed = Args { source: Source::Nothing, renderer: None, animations: true, new_instance: false };
    let mut it = args.iter();
    while let Some(arg) = it.next() {
        let source = match arg.as_str() {
            "--help" | "-h" => return Ok(None),
            "--demo" => Source::Demo,
            "--new-instance" => {
                parsed.new_instance = true;
                continue;
            }
            "--no-animations" => {
                parsed.animations = false;
                continue;
            }
            "--trace" => {
                let file = it.next().ok_or("--trace takes the file to write the log to")?;
                suhina_plugins::trace::start(std::path::Path::new(file));
                continue;
            }
            "--renderer" => {
                parsed.renderer = match it.next().map(String::as_str) {
                    Some("wgpu") => Some(eframe::Renderer::Wgpu),
                    Some("glow") => Some(eframe::Renderer::Glow),
                    other => return Err(format!("--renderer takes wgpu or glow, not {other:?}")),
                };
                continue;
            }
            flag if flag.starts_with('-') => return Err(format!("unknown argument: {flag}")),
            path => Source::Als(PathBuf::from(path)),
        };
        if parsed.source != Source::Nothing {
            return Err("give one project".into());
        }
        parsed.source = source;
    }
    Ok(Some(parsed))
}

struct Window {
    app: App,
    /// The build's endpoints: later starts' projects, and agents working on the open projects
    /// (ADR 0028); `None` with `--new-instance`.
    agents: Option<suhina_automation::AppEndpoints>,
    /// Shown over the first frames while Suhina starts; gone once it has faded out.
    splash: Option<Splash>,
    /// What starting does once the splash is on screen.
    start_up: Option<StartUp>,
    graphics_now: Graphics,
    /// The main window is a Win32 or X11 one, which plug-in editors can be embedded in.
    editors_embeddable: bool,
    themes: theme_files::ThemeFiles,
    /// The projects opened and saved lately, for the project dialog.
    recent: recent::Recent,
    /// The macOS menu bar, once installed (#650).
    #[cfg(target_os = "macos")]
    mac_menu: Option<mac_menu::MacMenu>,
}

/// The slow part of starting, done in the window once the splash shows rather than before the
/// window opens: the browser's library (its saved indexes), the sample map and the project
/// asked for.
struct StartUp {
    source: Source,
    output_choice: OutputChoice,
    /// Warnings from reading the settings, keymap and themes.
    warnings: Vec<String>,
}

impl eframe::App for Window {
    fn ui(&mut self, ui: &mut eframe::egui::Ui, _frame: &mut eframe::Frame) {
        if let Some(splash) = self.splash.as_mut()
            && splash.take_start(ui.ctx())
            && let Some(start_up) = self.start_up.take()
        {
            self.start(start_up);
            self.app.check_updates_at_start(ui.ctx());
        }
        if let Some(splash) = self.splash.as_mut()
            && !splash.started()
        {
            // The window is not ready yet: only the splash.
            let _shown = splash.show(ui);
            return;
        }
        self.serve_agents(ui.ctx());
        #[cfg(target_os = "macos")]
        if let Some(menu) = &self.mac_menu {
            for id in menu.take() {
                self.app.queue_action(&id);
            }
        }
        self.app.ui(ui);
        self.answer_agents();
        if let Some((path, how)) = self.app.take_open_request() {
            self.open(&path, how);
        }
        if let Some(new) = self.app.take_new_request() {
            self.new_project(new);
        }
        if let Some(offer) = self.app.take_recovery_request() {
            match Session::recover(&offer.backup, offer.file.as_deref()) {
                Ok((session, warnings)) => self.show_session(session, warnings, OpenAs::NewTab),
                Err(e) => self.app.set_status(Status::Error(e.message().to_owned())),
            }
        }
        if let Some(backup) = self.app.take_reveal_request()
            && let Err(e) = theme_files::reveal(&backup, true)
        {
            self.app.set_status(Status::Error(e));
        }
        if let Some(path) = self.app.take_saved_path() {
            self.remember(&path);
        }
        if let Some(request) = self.app.take_pick_request()
            && let Some(path) = pick(&request)
        {
            self.app.set_picked(request.field, path);
        }
        if let Some(path) = self.app.take_forget_request()
            && let Err(e) = self.recent.remove(&path)
        {
            self.app.set_status(Status::Error(e));
        }
        if let Some(folder) = self.app.take_lists_wanted() {
            self.app.set_project_lists(self.recent.existing(), recent::templates(&folder));
        }
        // Each project keeps its decoded audio in its session (ADR 0011).
        let _closed = self.app.take_closed();
        // Only the active project plays; one made active again gets an output.
        if let Some(id) = self.app.take_player_request() {
            let choice = self.app.projects().output_choice().clone();
            let opened = self.app.projects().get(id).map(|session| {
                suhina_output::open_chosen(session.project(), session.audio(), session.device_assets(), &choice, None)
            });
            match opened {
                Some(Ok(opened)) => {
                    self.app.attach_player(id, opened.player);
                    if let Some(notice) = opened.notice {
                        self.app.set_status(Status::Hint(notice));
                    }
                }
                Some(Err(e)) => self.app.set_status(Status::Error(format!("no audio output: {e}"))),
                None => {}
            }
            self.share_about();
        }
        if let Some(settings) = self.app.take_changed_settings()
            && let Err(e) = save_settings(&settings)
        {
            self.app.set_status(Status::Error(e));
        }
        if let Some(keymap) = self.app.take_changed_keymap()
            && let Err(e) = save_file(KEYMAP_FILE, &keymap.to_json())
        {
            self.app.set_status(Status::Error(e));
        }
        self.sync_themes(ui.ctx());
        if let Some(splash) = self.splash.as_mut()
            && !splash.show(ui)
        {
            self.splash = None;
        }
    }

    /// Quitting: backups that quitting calls for are written, and the journal says nothing is
    /// left to offer next time.
    fn on_exit(&mut self, _gl: Option<&eframe::glow::Context>) {
        let _finished_in_time = self.app.finish_backups();
        let _let_go_in_time = self.app.let_go_of_projects();
        // Restart now, after an update: the new build starts as this one closes.
        if self.app.restart_wanted()
            && let Err(e) = suhina_update::relaunch()
        {
            eprintln!("error: {e}");
        }
    }
}

impl Window {
    /// Before drawing (ADR 0028): opens the projects later starts of this build handed over,
    /// passes on what agents ask, and runs their work on the open projects, so this frame shows
    /// it.
    fn serve_agents(&mut self, ctx: &eframe::egui::Context) {
        let Some(agents) = self.agents.as_ref() else { return };
        agents.set_enabled(self.app.settings().agents);
        let include_project = self.app.settings().mcp_project_folder;
        let folders: Vec<PathBuf> = self.app.settings().mcp_write_folders.iter().map(PathBuf::from).collect();
        self.app.projects_mut().set_mcp_write_folders(include_project, &folders);
        let opens = agents.take_opens();
        for event in agents.take_events() {
            match event {
                suhina_automation::AppEvent::Ask { connection, client } => self.app.ask_agent(connection, client),
                suhina_automation::AppEvent::Gone { connection } => self.app.agent_gone(connection),
            }
        }
        if agents.run_jobs(self.app.projects_mut()) > 0 {
            self.app.note_outside_change();
        }
        self.app.set_agents(agents.connected());
        // A later start comes forward here, with its project if it had one.
        if let Some(files) = opens {
            ctx.send_viewport_cmd(eframe::egui::ViewportCommand::Minimized(false));
            ctx.send_viewport_cmd(eframe::egui::ViewportCommand::Focus);
            for path in files {
                self.open(&path, OpenAs::NewTab);
            }
        }
    }

    /// After drawing: the person's answers to agents and the agents they disconnected.
    fn answer_agents(&mut self) {
        let Some(agents) = self.agents.as_ref() else { return };
        for (connection, allow) in self.app.take_agent_answers() {
            agents.answer(connection, allow);
        }
        for connection in self.app.take_agent_disconnects() {
            agents.disconnect(connection);
        }
    }

    /// Starts the browser's library and the sample map, and opens the project asked for, or what
    /// Settings › General › At start says without one.
    fn start(&mut self, start_up: StartUp) {
        let StartUp { source, output_choice, mut warnings } = start_up;
        // The browser's library: browser.json and the folder indexes beside the settings (ADR 0012).
        let mut library = suhina_library::Library::new(eframe::storage_dir(app_id()));
        warnings.extend(library.take_warnings());
        if let Err(e) = library.start() {
            warnings.push(e);
        }
        self.app.set_file_library(library);
        // The sample map analyses the library's samples in the background (ADR 0013).
        self.app.projects_mut().start_sound_map();
        // What a crash left in the backups folder is offered (`docs/spec/backups.md`).
        self.app.offer_recovery();
        // Shown first, so what opening the project says comes after it.
        if let Some(warning) = warnings.first() {
            self.app.set_status(Status::Hint(warning.clone()));
        }
        match source {
            // What Settings › General › At start says; the last project is the newest recent one.
            Source::Nothing => self.app.open_at_start(self.recent.existing().into_iter().next()),
            Source::Demo => {
                self.app.projects_mut().close_preview_output();
                let session = Session::new(suhina_ui::demo::demo_project());
                let (session, opened) = with_audio(session, &output_choice);
                self.app.open_project(session, &opened, OpenAs::NewTab);
            }
            Source::Als(path) => self.open(&path, OpenAs::NewTab),
        }
    }

    /// Opens a Live Set in a new tab or in place of the active project: imports it, decodes its
    /// audio and starts playing it, stopped.
    fn open(&mut self, path: &Path, how: OpenAs) {
        match Session::open(path) {
            Ok((session, warnings)) => {
                self.show_session(session, warnings, how);
                self.remember(path);
            }
            Err(e) => self.app.set_status(Status::Error(e.message().to_owned())),
        }
    }

    /// Makes a new project, blank or from a template, and opens it in a new tab.
    fn new_project(&mut self, new: NewProject) {
        let made = match new {
            NewProject::Blank => Ok((Session::new(suhina_session::blank_project()), Vec::new())),
            NewProject::Template(path) => Session::from_template(&path),
        };
        match made {
            Ok((session, warnings)) => self.show_session(session, warnings, OpenAs::NewTab),
            Err(e) => self.app.set_status(Status::Error(e.message().to_owned())),
        }
    }

    /// Decodes a session's audio, starts it playing (stopped) and shows it.
    fn show_session(&mut self, mut session: Session, mut warnings: Vec<String>, how: OpenAs) {
        let folder = session.audio_folder().map_or_else(|| PathBuf::from("."), Path::to_path_buf);
        // Outside the project's folder only the person's own saved files and library folders
        // are read (`docs/spec/missing-files.md` › Project file boundaries).
        let source = session.source().map(Path::to_path_buf);
        let trusted = self.app.projects_mut().trusted_audio_for(source.as_deref());
        let (audio, audio_warnings) = suhina_media::load_project_audio_trusted(session.project(), &folder, &trusted);
        session.set_trusted(trusted);
        warnings.extend(audio_warnings);
        let waveforms = suhina_ui::kit::waveform::from_pool(&audio);
        session.set_audio(audio);
        let choice = self.app.projects().output_choice().clone();
        // A device may only be open once: the previews' own output closes first (ADR 0011).
        self.app.projects_mut().close_preview_output();
        let (session, output_warnings) = with_audio(session, &choice);
        warnings.extend(output_warnings);
        if self.app.open_project(session, &warnings, how).is_some() {
            self.app.set_waveforms(waveforms);
        }
        let _closed = self.app.take_closed();
        self.share_about();
    }

    /// Puts a project opened or saved first among the recent ones.
    fn remember(&mut self, path: &Path) {
        if let Err(e) = self.recent.add(path) {
            self.app.set_status(Status::Error(e));
        }
        self.app.record_project(path);
    }

    /// Does what Settings asked of the theme files (write, rename, read again, back up, export
    /// the template, bring in a dropped file, show the folder) and reads a theme file saved
    /// elsewhere. While a theme file is in use the folder is looked at every second.
    fn sync_themes(&mut self, ctx: &eframe::egui::Context) {
        let mut reload = false;
        for action in self.app.take_theme_actions() {
            let done = match action {
                FileAction::Save { id, theme } => self.themes.write(&id, &theme).map(|()| None),
                FileAction::Rename { from, to } => self.themes.rename(&from, &to).map(|()| None),
                FileAction::Reload => {
                    reload = true;
                    Ok(None)
                }
                FileAction::OpenFolder => self.themes.open_folder().map(|()| None),
                FileAction::Backup => self.themes.backup().map(|path| Some(saved(&path, "Backed up the themes to"))),
                FileAction::ExportTemplate => {
                    self.themes.export_template().map(|path| Some(saved(&path, "Saved the theme template to")))
                }
                FileAction::Import(path) => self.import_themes(ctx, &path).map(Some),
            };
            match done {
                Ok(Some(message)) => self.app.set_status(Status::Done(message)),
                Ok(None) => {}
                Err(e) => self.app.set_status(Status::Error(e)),
            }
        }
        let (entries, warnings) = if reload {
            let (entries, warnings) = self.themes.read_all();
            let count = entries.len();
            self.app.set_themes(ctx, entries, self.themes.folder_text());
            if warnings.is_empty() {
                self.app.set_status(Status::Done(format!("Read {count} theme files")));
            }
            (Vec::new(), warnings)
        } else {
            self.themes.changed()
        };
        for entry in entries {
            self.app.put_theme(ctx, entry);
        }
        if let Some(warning) = warnings.first() {
            self.app.set_status(Status::Error(warning.clone()));
        }
        if !suhina_ui::kit::theme::is_built_in(&self.app.settings().theme) {
            ctx.request_repaint_after(std::time::Duration::from_secs(1));
        }
    }

    /// Brings in a dropped theme file or zip and lists the themes again; a single new theme is
    /// put in use. Returns what to tell the user.
    fn import_themes(&mut self, ctx: &eframe::egui::Context, path: &Path) -> Result<String, String> {
        let (ids, warnings) = self.themes.import(path)?;
        let (entries, _) = self.themes.read_all();
        self.app.set_themes(ctx, entries, self.themes.folder_text());
        if let [id] = ids.as_slice() {
            self.app.use_theme(ctx, id);
        }
        let file = path.file_name().map_or_else(String::new, |n| n.to_string_lossy().into_owned());
        let added = match ids.len() {
            0 => format!("{file} has no themes that are not here already"),
            1 => format!("Added the theme from {file}"),
            n => format!("Added {n} themes from {file}"),
        };
        Ok(match warnings.first() {
            Some(warning) => format!("{added}; {warning}"),
            None => added,
        })
    }

    /// Tells the Settings page what is in use.
    fn share_about(&mut self) {
        self.app.set_about(About {
            graphics_now: Some(self.graphics_now),
            editors_embeddable: self.editors_embeddable,
            ..Default::default()
        });
    }
}

/// `"<what> <file>"`, and the file shown in the file manager.
/// Shows the system's file or folder dialog `request` asks for; `None` when it was cancelled.
/// It waits on the main thread, as macOS requires, while the window stays behind it.
fn pick(request: &suhina_ui::interaction::pick::PickRequest) -> Option<PathBuf> {
    let mut dialog = rfd::FileDialog::new().set_title(request.field.title());
    if let Some(start) = request.start.as_ref().filter(|s| s.is_dir()) {
        dialog = dialog.set_directory(start);
    }
    if request.field.folder() {
        dialog.pick_folder()
    } else if request.field == suhina_ui::interaction::pick::PickField::ExportFile {
        // The export writes a file that need not exist yet: a save dialog, not an open one.
        dialog.add_filter(request.field.filter_label(), request.field.extensions()).save_file()
    } else {
        dialog.add_filter(request.field.filter_label(), request.field.extensions()).pick_file()
    }
}

fn saved(path: &Path, what: &str) -> String {
    let _shown = theme_files::reveal(path, true);
    format!("{what} {}", path.display())
}

/// The app id whose folder holds the settings: the old one while only its folder exists
/// (copying it under the new name failed), so the settings stay readable.
fn app_id() -> &'static str {
    match (eframe::storage_dir(APP_ID), eframe::storage_dir(OLD_APP_ID)) {
        (Some(new), Some(old)) if !new.exists() && old.is_dir() => OLD_APP_ID,
        _ => APP_ID,
    }
}

/// Where projects are saved and templates kept until the settings say otherwise: `Suhina`
/// in the user's documents folder, or the folder made under the program's old name while only
/// that one exists (projects there stay where recent lists and links expect them).
fn default_folders() -> (PathBuf, PathBuf) {
    let base = documents_base();
    (base.join("Projects"), base.join("Templates"))
}

/// `Suhina` in the user's documents folder, or the folder made under the program's old name
/// while only that one exists; defaults are built beside it.
fn documents_base() -> PathBuf {
    let home = std::env::var_os("USERPROFILE").or_else(|| std::env::var_os("HOME")).map(PathBuf::from);
    let documents = home.map(|h| h.join("Documents")).unwrap_or_default();
    let (new, old) = (documents.join("Suhina"), documents.join(OLD_APP_ID));
    if !new.exists() && old.is_dir() { old } else { new }
}

/// Connects the output `choice` names to `session`, playing its audio; a device that is gone
/// falls back to the default with a warning, and without any output the project still opens.
/// Returns the session and warnings.
fn with_audio(mut session: Session, choice: &OutputChoice) -> (Session, Vec<String>) {
    match suhina_output::open_chosen(session.project(), session.audio(), session.device_assets(), choice, None) {
        Ok(opened) => {
            session.attach_player(opened.player);
            (session, opened.notice.into_iter().collect())
        }
        Err(e) => (session, vec![format!("no audio output: {e}")]),
    }
}

/// The main window's native handle, for plug-in editors to float above it (ADR 0006): a Win32
/// `HWND` or an X11 window id; `None` elsewhere (Wayland, macOS), where the plug-in host cannot
/// open editors yet.
/// On Linux the window opens as an X11 one whenever an X server is there (`DISPLAY`): the
/// desktop's own, or XWayland on a Wayland desktop, which every distribution ships. Plug-in
/// editors are X11 windows (the hosts open them with `DISPLAY` too), and only an X11 window
/// can own them or embed them (ADR 0006); a Wayland window could do neither. Without
/// `DISPLAY` winit chooses as usual (Wayland), and editors do not open: the host opens them
/// only as X11 windows, so they need an X server.
#[cfg(target_os = "linux")]
fn x11_first() -> Option<eframe::EventLoopBuilderHook> {
    use winit::platform::x11::EventLoopBuilderExtX11;
    let display = std::env::var("DISPLAY").is_ok_and(|d| !d.is_empty());
    display.then(|| {
        Box::new(|builder: &mut eframe::EventLoopBuilder<eframe::UserEvent>| {
            builder.with_x11();
        }) as eframe::EventLoopBuilderHook
    })
}

#[cfg(not(target_os = "linux"))]
fn x11_first() -> Option<eframe::EventLoopBuilderHook> {
    None
}

// An X11 window id is a `c_ulong`: 32 bits on Windows, 64 on Linux, where `u64::from` changes
// nothing.
#[allow(clippy::useless_conversion)]
fn native_window(cc: &eframe::CreationContext<'_>) -> Option<u64> {
    use raw_window_handle::{HasWindowHandle, RawWindowHandle};
    match cc.window_handle().ok()?.as_raw() {
        RawWindowHandle::Win32(handle) => u64::try_from(handle.hwnd.get()).ok(),
        RawWindowHandle::Xlib(handle) => Some(u64::from(handle.window)),
        RawWindowHandle::Xcb(handle) => Some(u64::from(handle.window.get())),
        _ => None,
    }
}

fn settings_path() -> Option<PathBuf> {
    eframe::storage_dir(app_id()).map(|dir| dir.join(SETTINGS_FILE))
}

/// The saved settings, or the defaults when there are none yet.
fn load_settings() -> (Settings, Vec<String>) {
    match settings_path().map(std::fs::read_to_string) {
        Some(Ok(text)) => Settings::from_text(&text),
        Some(Err(e)) if e.kind() != std::io::ErrorKind::NotFound => {
            (Settings::default(), vec![format!("could not read the settings: {e}")])
        }
        _ => (Settings::default(), Vec::new()),
    }
}

fn save_settings(settings: &Settings) -> Result<(), String> {
    save_file(SETTINGS_FILE, &settings.to_text())
}

/// The saved keymap over the default keys, or the defaults when there is none yet.
fn load_keymap() -> (Keymap, Vec<String>) {
    let path = eframe::storage_dir(app_id()).map(|dir| dir.join(KEYMAP_FILE));
    match path.map(std::fs::read_to_string) {
        Some(Ok(text)) => Keymap::from_json(&text),
        Some(Err(e)) if e.kind() != std::io::ErrorKind::NotFound => {
            (Keymap::default(), vec![format!("could not read the keymap: {e}")])
        }
        _ => (Keymap::default(), Vec::new()),
    }
}

/// Writes one of Suhina's files in its app-data folder.
fn save_file(name: &str, text: &str) -> Result<(), String> {
    let dir = eframe::storage_dir(app_id()).ok_or("there is no folder to keep settings in")?;
    std::fs::create_dir_all(&dir).map_err(|e| save_error::message(name, &e))?;
    std::fs::write(dir.join(name), text).map_err(|e| save_error::message(name, &e))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn parse(v: &[&str]) -> Result<Option<Args>, String> {
        parse_args(&v.iter().map(|s| s.to_string()).collect::<Vec<_>>())
    }

    #[test]
    fn one_project_or_none() {
        assert_eq!(parse(&[]).ok().flatten().map(|a| a.source), Some(Source::Nothing));
        assert_eq!(parse(&["--demo"]).ok().flatten().map(|a| a.source), Some(Source::Demo));
        assert_eq!(parse(&["set.als"]).ok().flatten().map(|a| a.source), Some(Source::Als("set.als".into())));
        assert!(matches!(parse(&["--help"]), Ok(None)));
        assert!(parse(&["--bogus"]).is_err());
        assert!(parse(&["a.als", "b.als"]).is_err());
    }

    #[test]
    fn renderer_and_animations_can_be_chosen() {
        let args = parse(&["--demo", "--renderer", "glow", "--no-animations"]).ok().flatten();
        assert_eq!(args.as_ref().map(|a| a.renderer), Some(Some(eframe::Renderer::Glow)));
        assert_eq!(args.map(|a| a.animations), Some(false));
        assert!(parse(&["--renderer", "metal"]).is_err());
        assert!(parse(&["--renderer"]).is_err());
    }

    #[test]
    fn a_new_instance_is_asked_for_by_its_flag() {
        assert_eq!(parse(&["set.als"]).ok().flatten().map(|a| a.new_instance), Some(false));
        let args = parse(&["--new-instance", "set.als"]).ok().flatten();
        assert_eq!(args.as_ref().map(|a| a.new_instance), Some(true));
        assert_eq!(args.map(|a| a.source), Some(Source::Als("set.als".into())));
    }
}
