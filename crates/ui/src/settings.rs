// Copyright (C) 2026 The Suhina contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! The user's settings: what they are, their defaults, and the text file they are kept in
//! (`key = value` lines, so a person can read and fix it). The app decides where the file lives.

pub mod keymap;
pub mod plugin_manager;
mod settings_rows;
pub mod settings_view;
pub mod themes;
pub mod update_settings;
pub mod view_settings;
pub mod workflow;

/// Which side of the timeline the track headers are on.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Side {
    Left,
    #[default]
    Right,
}

/// What opening a project does while another is open.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum OnOpen {
    /// Ask each time, in the window.
    #[default]
    Ask,
    /// Open it in a new tab.
    NewTab,
    /// Close the active project and open it in its place.
    Replace,
}

/// What opens when Suhina starts without a project given (Settings › General › At start).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum AtStart {
    /// The start panel, with the recent projects and the templates (`start_panel`).
    #[default]
    Panel,
    /// A new blank project.
    Blank,
    /// A new project from the template `Settings::start_template` names.
    Template,
    /// The project opened or saved last.
    Last,
}

/// Where the playhead goes when the transport stops (`transport.stop`'s `return`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum StopReturn {
    /// Back to the play start marker, where it started.
    #[default]
    Marker,
    /// It stays where it stopped: the marker moves there.
    Here,
}

impl StopReturn {
    /// The value `transport.stop` takes for it.
    pub fn param(self) -> &'static str {
        match self {
            Self::Marker => "marker",
            Self::Here => "here",
        }
    }
}

/// What stays above the piano roll when its splitter is pulled to the top.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum PianoRollTop {
    /// The Arrangement view's ruler and the track whose clip is edited.
    #[default]
    Track,
    /// Nothing: the piano roll fills the area under the control bar.
    Window,
}

/// How large the piano roll's toolbar is drawn: the toolbar's height, every button's box and the
/// gaps and icon strokes inside it scale together, and the rest of the editor does not
/// (`docs/spec/ui-clip-editor.md` › Layout).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum ToolbarSize {
    /// 100 %.
    Normal,
    /// 125 %, the default.
    #[default]
    Large,
    /// 150 %.
    Larger,
}

impl ToolbarSize {
    pub const ALL: [Self; 3] = [Self::Normal, Self::Large, Self::Larger];

    /// The name the Settings row shows.
    pub fn name(self) -> &'static str {
        match self {
            Self::Normal => "Normal",
            Self::Large => "Large",
            Self::Larger => "Larger",
        }
    }

    /// The factor the toolbar is drawn at (1.0, 1.25 and 1.5).
    pub fn scale(self) -> f32 {
        match self {
            Self::Normal => 1.0,
            Self::Large => 1.25,
            Self::Larger => 1.5,
        }
    }

    /// The value the settings file keeps.
    pub fn param(self) -> &'static str {
        match self {
            Self::Normal => "normal",
            Self::Large => "large",
            Self::Larger => "larger",
        }
    }

    fn parse(text: &str) -> Option<Self> {
        Self::ALL.into_iter().find(|s| s.param() == text)
    }
}

/// Where Play and the play/stop key start the transport (`transport.play`'s `from`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum PlayFrom {
    /// From the play start marker.
    #[default]
    Marker,
    /// From the start of the clip selected in the Arrangement view; from the marker when none is.
    Clip,
}

impl PlayFrom {
    /// The value `transport.play` takes for it, and the settings file keeps.
    pub fn param(self) -> &'static str {
        match self {
            Self::Marker => "marker",
            Self::Clip => "clip",
        }
    }
}

/// How the detail area lays out a device chain.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum DeviceLayout {
    /// Devices side by side in a row under the view.
    #[default]
    Row,
    /// Devices one under another in a column beside the view.
    Stack,
}

/// How a track's volume and pan are shown: as the field they have always been (a bar for the
/// volume, a centre bar for the pan), or as a knob beside the value.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum MixerControls {
    #[default]
    Field,
    Knob,
}

/// Where a plug-in's editor opens.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum EditorPlacement {
    /// In a window of its own, owned by Suhina's: above it, and minimised with it.
    #[default]
    Window,
    /// Inside the device panel, as part of the device chain (Windows and X11; elsewhere the
    /// editor opens in a window).
    DeviceView,
}

/// How the window is drawn. Takes effect the next time the app starts.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Graphics {
    /// wgpu: Vulkan, Metal or Direct3D 12.
    #[default]
    Gpu,
    /// OpenGL, through glow.
    OpenGl,
}

/// How samples with a known tempo are stretched when dropped (`clip.addFiles`' `warp`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum WarpMode {
    /// Keeps transients on the grid.
    #[default]
    Rhythmic,
    Tonal,
    Texture,
    Smooth,
    HighQuality,
    /// Speed and pitch change together.
    Repitch,
}

impl WarpMode {
    pub const ALL: [WarpMode; 6] =
        [Self::Rhythmic, Self::Tonal, Self::Texture, Self::Smooth, Self::HighQuality, Self::Repitch];

    /// The value `clip.addFiles` takes, and the settings file keeps.
    pub fn param(self) -> &'static str {
        match self {
            Self::Rhythmic => "rhythmic",
            Self::Tonal => "tonal",
            Self::Texture => "texture",
            Self::Smooth => "smooth",
            Self::HighQuality => "high-quality",
            Self::Repitch => "repitch",
        }
    }

    pub fn title(self) -> &'static str {
        match self {
            Self::Rhythmic => "Rhythmic",
            Self::Tonal => "Tonal",
            Self::Texture => "Texture",
            Self::Smooth => "Smooth",
            Self::HighQuality => "High quality",
            Self::Repitch => "Re-pitch",
        }
    }

    fn parse(text: &str) -> Option<Self> {
        Self::ALL.into_iter().find(|m| m.param() == text)
    }
}

/// The browser panel's narrowest and widest kept in the settings, and its width at first. On
/// screen it is also never wider than [`BROWSER_SHARE`] of the window.
pub const BROWSER_WIDTHS: (u32, u32) = (200, 4000);
pub const BROWSER_WIDTH: u32 = 300;
/// The most of the window's width the browser panel takes.
pub const BROWSER_SHARE: f32 = 0.5;
/// The preview's level, in dB: its range and its default.
/// The longest sample the sample map shows by default, and the range it may be set to.
pub const MAP_MAX_SECONDS: u32 = 10;
/// The size of the sample map's points, in percent of their size at first: its range and default.
pub const MAP_DOT_SIZES: (u32, u32) = (25, 400);
pub const MAP_DOT_SIZE: u32 = 100;
pub const MAP_MAX_SECONDS_RANGE: (u32, u32) = (1, 60);
pub const PREVIEW_VOLUMES: (i32, i32) = (-48, 6);
/// The cue volumes a settings file may hold, in dB; the lowest is silence.
pub const CUE_VOLUMES: (i32, i32) = (-60, 12);
pub const PREVIEW_VOLUME: i32 = -6;
/// The size of the whole window, in percent: the smallest and largest, the default, and how much
/// one Zoom in or Zoom out changes it.
pub const ZOOMS: (u32, u32) = (50, 200);
pub const ZOOM: u32 = 100;
pub const ZOOM_STEP: u32 = 10;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Settings {
    pub animations: bool,
    pub track_headers: Side,
    pub graphics: Graphics,
    /// Audio clips moved over others stack (all play) instead of replacing them.
    pub sample_stacking: bool,
    /// MIDI clips moved over others stack instead of replacing them.
    pub midi_stacking: bool,
    pub on_open: OnOpen,
    pub at_start: AtStart,
    /// The template `AtStart::Template` starts from: a path, or a file name in the templates
    /// folder.
    pub start_template: String,
    /// Where Stop leaves the playhead.
    pub stop: StopReturn,
    /// Where Play starts.
    pub play_from: PlayFrom,
    pub device_layout: DeviceLayout,
    /// Launching a clip in the Session view selects it, so the detail area shows it
    /// (`docs/spec/ui-session-view.md` › Launching and selecting).
    pub select_on_launch: bool,
    /// The Session mixer's height, the detail area's layout and double-clicks that make clips.
    pub view: crate::settings::view_settings::ViewSettings,
    /// Whether and where to check for updates (`docs/spec/updates.md`).
    pub updates: crate::settings::update_settings::UpdateSettings,
    /// What stays above the piano roll pulled to the top.
    pub piano_roll_top: PianoRollTop,
    /// How large the piano roll's toolbar is drawn.
    pub toolbar_size: ToolbarSize,
    /// The control bar's grid value sets the piano roll's too; otherwise the piano roll keeps
    /// its own.
    pub piano_grid_follows: bool,
    /// A track's volume and pan as a field or as a knob (Settings › Session and mixer).
    pub mixer_controls: MixerControls,
    /// Where plug-in editors open.
    pub plugin_editors: EditorPlacement,
    /// The theme in use: [`crate::kit::theme::BUILT_IN`] or the name of a file in the themes folder,
    /// without `.json`.
    pub theme: String,
    /// Where new projects are saved and templates are kept; empty for the defaults the app gives
    /// (`App::set_default_folders`).
    pub projects_folder: String,
    pub templates_folder: String,
    /// Where the NAM devices' models and IRs list from
    /// ([docs/spec/nam-amp-and-cab.md]); empty for the defaults the app gives.
    pub nam_models_folder: String,
    pub irs_folder: String,
    /// The browser panel shows, on which side, and how wide (`docs/spec/browser.md`).
    pub browser_open: bool,
    pub browser_side: Side,
    pub browser_width: u32,
    /// Selecting a sample or MIDI file previews it.
    pub preview_autoplay: bool,
    /// The preview's level, in dB.
    pub preview_volume: i32,
    /// Several dropped files go one track each without Ctrl (and onto one track with it).
    pub drop_separate: bool,
    pub warp_mode: WarpMode,
    /// The sample map (`docs/spec/sample-map.md`): grid rather than cloud, preview on hover,
    /// and the longest sample it shows, in seconds.
    pub map_grid: bool,
    pub map_hover_preview: bool,
    pub map_max_seconds: u32,
    /// The sample map's point size, in percent (Size in the map's top bar).
    pub map_dot_size: u32,
    /// The sample map is in a window of its own (`docs/spec/sample-map.md` › Where the map is
    /// shown), and where that window was: its left, top, width and height in points, on the
    /// whole desktop, so a second screen counts.
    pub map_window: bool,
    pub map_window_rect: Option<[i32; 4]>,
    /// Waveforms in decibels, so quiet parts show; linear otherwise.
    pub waveform_db: bool,
    /// The audio output (Settings › Audio, `audio.setOutput`): host and device by name, empty
    /// for the default; sample rate and buffer size, 0 for the device's own.
    pub audio_host: String,
    pub audio_device: String,
    pub audio_sample_rate: u32,
    pub audio_buffer: u32,
    /// WASAPI exclusive mode; off, so other programs stay heard.
    pub audio_exclusive: bool,
    /// The output pair the cue plays on (`output.setCuePair`): 1 is channels 3 and 4; 0 plays
    /// it with the main output.
    pub audio_cue_pair: u16,
    /// The cue volume in dB (`output.setCueVolume`); the lowest of [`CUE_VOLUMES`] is silence.
    pub audio_cue_volume: i32,
    /// The audio input device (Settings › Audio, `audio.setOutput`'s `input`): "off" (the
    /// default), "default" or a device's name.
    pub audio_input: String,
    /// The MIDI input ports turned off, by name (Settings › MIDI, `midi.setInput`); every
    /// other port is on. One `midi_input_off` line each, since a port's name may hold commas.
    pub midi_inputs_off: Vec<String>,
    /// Selecting a track arms it, so a MIDI controller plays what the user looks at (Settings ›
    /// MIDI, #834); off until the person turns it on.
    pub auto_arm: bool,
    /// The size of the whole window, in percent (`ZOOMS`): everything is drawn this much larger
    /// or smaller.
    pub zoom: u32,
    /// When backups of changed projects are written, and whether old ones are deleted
    /// (`docs/spec/backups.md`).
    pub backup: suhina_session::backup::Interval,
    pub backup_clean_up: bool,
    /// Agents may connect over MCP and work on the open projects (ADR 0028); off until the person
    /// turns it on.
    pub agents: bool,
    /// Attached agents may write within the project folder, including subfolders.
    pub mcp_project_folder: bool,
    /// Additional write folders granted by the person, including subfolders.
    pub mcp_write_folders: Vec<String>,
}

impl Default for Settings {
    fn default() -> Self {
        Self {
            animations: true,
            track_headers: Side::Right,
            graphics: Graphics::Gpu,
            sample_stacking: false,
            midi_stacking: false,
            on_open: OnOpen::Ask,
            at_start: AtStart::Panel,
            start_template: String::new(),
            stop: StopReturn::Marker,
            play_from: PlayFrom::Clip,
            device_layout: DeviceLayout::Row,
            select_on_launch: true,
            view: crate::settings::view_settings::ViewSettings::default(),
            updates: crate::settings::update_settings::UpdateSettings::default(),
            piano_roll_top: PianoRollTop::Track,
            toolbar_size: ToolbarSize::Large,
            piano_grid_follows: false,
            mixer_controls: MixerControls::Field,
            plugin_editors: EditorPlacement::Window,
            theme: crate::kit::theme::BUILT_IN.into(),
            projects_folder: String::new(),
            templates_folder: String::new(),
            nam_models_folder: String::new(),
            irs_folder: String::new(),
            browser_open: true,
            browser_side: Side::Left,
            browser_width: BROWSER_WIDTH,
            preview_autoplay: true,
            preview_volume: PREVIEW_VOLUME,
            drop_separate: false,
            warp_mode: WarpMode::Rhythmic,
            map_grid: false,
            map_hover_preview: true,
            map_max_seconds: MAP_MAX_SECONDS,
            map_dot_size: MAP_DOT_SIZE,
            map_window: false,
            map_window_rect: None,
            waveform_db: false,
            audio_host: String::new(),
            audio_device: String::new(),
            audio_sample_rate: 0,
            audio_buffer: 0,
            audio_exclusive: false,
            audio_cue_pair: 0,
            audio_cue_volume: 0,
            audio_input: "off".into(),
            midi_inputs_off: Vec::new(),
            zoom: ZOOM,
            backup: suhina_session::backup::Interval::default(),
            backup_clean_up: true,
            agents: false,
            mcp_project_folder: true,
            mcp_write_folders: Vec::new(),
            auto_arm: false,
        }
    }
}

impl Settings {
    /// The audio output these settings choose.
    pub fn output_choice(&self) -> suhina_session::OutputChoice {
        let text = |s: &str| (!s.trim().is_empty()).then(|| s.trim().to_string());
        suhina_session::OutputChoice {
            host: text(&self.audio_host),
            device: text(&self.audio_device),
            sample_rate: (self.audio_sample_rate > 0).then_some(self.audio_sample_rate),
            buffer_frames: (self.audio_buffer > 0).then_some(self.audio_buffer),
            exclusive: self.audio_exclusive,
            cue_pair: (self.audio_cue_pair > 0).then_some(self.audio_cue_pair),
            cue_gain: if self.audio_cue_volume <= CUE_VOLUMES.0 {
                0.0
            } else {
                10f64.powf(f64::from(self.audio_cue_volume) / 20.0) as f32
            },
            input: suhina_session::InputChoice::from_text(&self.audio_input),
        }
    }

    /// Keeps `choice` as the audio output.
    pub fn set_output_choice(&mut self, choice: &suhina_session::OutputChoice) {
        self.audio_host = choice.host.clone().unwrap_or_default();
        self.audio_device = choice.device.clone().unwrap_or_default();
        self.audio_sample_rate = choice.sample_rate.unwrap_or(0);
        self.audio_buffer = choice.buffer_frames.unwrap_or(0);
        self.audio_exclusive = choice.exclusive;
        self.audio_cue_pair = choice.cue_pair.unwrap_or(0);
        self.audio_cue_volume = if choice.cue_gain > 0.0 {
            ((20.0 * f64::from(choice.cue_gain).log10()).round() as i32).clamp(CUE_VOLUMES.0, CUE_VOLUMES.1)
        } else {
            CUE_VOLUMES.0
        };
        self.audio_input = choice.input.as_text().to_string();
    }

    /// Makes the window `steps` zoom steps larger (or smaller, when negative), onto the steps and
    /// within the limits. Returns whether the zoom changed.
    pub fn step_zoom(&mut self, steps: i32) -> bool {
        let step = i64::from(ZOOM_STEP);
        let zoom = i64::from(self.zoom);
        // From the step on the side it moves away from, so a hand-edited 115 goes to 120 or 110.
        let from = match steps.signum() {
            1 => zoom / step * step,
            -1 => (zoom + step - 1) / step * step,
            _ => return false,
        };
        let wanted = from.saturating_add(i64::from(steps).saturating_mul(step));
        let zoom = u32::try_from(wanted.clamp(i64::from(ZOOMS.0), i64::from(ZOOMS.1))).unwrap_or(ZOOM);
        let changed = zoom != self.zoom;
        self.zoom = zoom;
        changed
    }

    /// The zoom as the factor everything is drawn at (1.0 for 100 %).
    pub fn zoom_factor(&self) -> f32 {
        self.zoom.clamp(ZOOMS.0, ZOOMS.1) as f32 / 100.0
    }

    /// The settings as the lines of a settings file.
    pub fn to_text(&self) -> String {
        let headers = match self.track_headers {
            Side::Left => "left",
            Side::Right => "right",
        };
        let graphics = match self.graphics {
            Graphics::Gpu => "wgpu",
            Graphics::OpenGl => "glow",
        };
        let on = |b: bool| if b { "on" } else { "off" };
        let devices = match self.device_layout {
            DeviceLayout::Row => "row",
            DeviceLayout::Stack => "stack",
        };
        let on_open = match self.on_open {
            OnOpen::Ask => "ask",
            OnOpen::NewTab => "tab",
            OnOpen::Replace => "replace",
        };
        let at_start = match self.at_start {
            AtStart::Panel => "panel",
            AtStart::Blank => "blank",
            AtStart::Template => "template",
            AtStart::Last => "last",
        };
        let editors = match self.plugin_editors {
            EditorPlacement::Window => "window",
            EditorPlacement::DeviceView => "device",
        };
        format!(
            "# Suhina settings; changed from Options > Settings.\nanimations = {}\ntrack_headers = {headers}\nrenderer = {graphics}\nsample_stacking = {}\nmidi_stacking = {}\nopen_projects = {on_open}\nat_start = {at_start}\nstart_template = {}\nstop = {stop}\nplay_start = {play_from}\ndevice_chain = {devices}\nmixer_controls = {mixer}\nselect_on_launch = {select_on_launch}\nplugin_editors = {editors}\ntheme = {}\nprojects_folder = {}\ntemplates_folder = {}\nnam_models_folder = {}\nirs_folder = {}\nbrowser = {}\nbrowser_side = {browser_side}\nbrowser_width = {}\npreview_autoplay = {}\npreview_volume = {}\ndrop_files = {drop}\nwarp_mode = {}\nmap_layout = {map_layout}\nmap_hover_preview = {}\nmap_max_seconds = {}\nmap_dot_size = {}\nmap_window = {}\nmap_window_rect = {}\naudio_host = {}\naudio_device = {}\naudio_sample_rate = {}\naudio_buffer = {}\naudio_exclusive = {}\naudio_cue_pair = {}\naudio_cue_volume = {}\naudio_input = {}\nzoom = {}\nwaveform_scale = {waveform_scale}\npiano_roll_top = {piano_top}\npiano_roll_grid = {piano_grid}\npiano_roll_toolbar = {piano_toolbar}\nbackup = {}\nbackup_clean_up = {}\n",
            on(self.animations),
            on(self.sample_stacking),
            on(self.midi_stacking),
            self.start_template,
            self.theme,
            self.projects_folder,
            self.templates_folder,
            self.nam_models_folder,
            self.irs_folder,
            on(self.browser_open),
            self.browser_width,
            on(self.preview_autoplay),
            self.preview_volume,
            self.warp_mode.param(),
            on(self.map_hover_preview),
            self.map_max_seconds,
            self.map_dot_size,
            on(self.map_window),
            self.map_window_rect.map_or_else(|| "none".to_string(), |[x, y, w, h]| format!("{x}, {y}, {w}, {h}")),
            self.audio_host,
            self.audio_device,
            self.audio_sample_rate,
            self.audio_buffer,
            on(self.audio_exclusive),
            self.audio_cue_pair,
            self.audio_cue_volume,
            self.audio_input,
            self.zoom,
            self.backup.name(),
            on(self.backup_clean_up),
            map_layout = if self.map_grid { "grid" } else { "cloud" },
            waveform_scale = if self.waveform_db { "db" } else { "linear" },
            stop = self.stop.param(),
            select_on_launch = on(self.select_on_launch),
            play_from = self.play_from.param(),
            browser_side = match self.browser_side {
                Side::Left => "left",
                Side::Right => "right",
            },
            drop = if self.drop_separate { "separate" } else { "one_track" },
            piano_top = match self.piano_roll_top {
                PianoRollTop::Track => "track",
                PianoRollTop::Window => "window",
            },
            piano_grid = if self.piano_grid_follows { "control_bar" } else { "own" },
            mixer = match self.mixer_controls {
                MixerControls::Field => "field",
                MixerControls::Knob => "knob",
            },
            piano_toolbar = self.toolbar_size.param(),
        ) + &self.view.to_text()
            + &format!("agents = {}\n", if self.agents { "on" } else { "off" })
            + &format!("mcp_project_folder = {}\n", if self.mcp_project_folder { "on" } else { "off" })
            + &format!("mcp_write_folders = {}\n", serde_json::json!(self.mcp_write_folders))
            + &format!("auto_arm = {}\n", if self.auto_arm { "on" } else { "off" })
            + &self.updates.to_text()
            + &self.midi_inputs_off.iter().map(|port| format!("midi_input_off = {port}\n")).collect::<String>()
    }

    /// Reads a settings file. Unknown keys and values are skipped with a warning and leave the
    /// default, so an old or hand-edited file never stops the app from starting.
    pub fn from_text(text: &str) -> (Self, Vec<String>) {
        let mut settings = Self::default();
        let mut warnings = Vec::new();
        for (number, line) in text.lines().enumerate() {
            let line = line.trim();
            if line.is_empty() || line.starts_with('#') {
                continue;
            }
            let Some((key, value)) = line.split_once('=') else {
                warnings.push(format!("settings line {}: expected key = value", number + 1));
                continue;
            };
            let (key, value) = (key.trim(), value.trim());
            if settings.view.parse(key, value) || settings.updates.parse(key, value) {
                continue;
            }
            match (key, value) {
                ("animations", "on" | "off") => settings.animations = value == "on",
                ("sample_stacking", "on" | "off") => settings.sample_stacking = value == "on",
                ("agents", "on" | "off") => settings.agents = value == "on",
                ("mcp_project_folder", "on" | "off") => settings.mcp_project_folder = value == "on",
                ("mcp_write_folders", folders) => match serde_json::from_str::<Vec<String>>(folders) {
                    Ok(folders) => settings.mcp_write_folders = folders,
                    Err(_) => {
                        warnings.push(format!("settings line {}: expected a list of MCP write folders", number + 1))
                    }
                },
                ("auto_arm", "on" | "off") => settings.auto_arm = value == "on",
                ("midi_stacking", "on" | "off") => settings.midi_stacking = value == "on",
                ("track_headers", "left") => settings.track_headers = Side::Left,
                ("track_headers", "right") => settings.track_headers = Side::Right,
                ("device_chain", "row") => settings.device_layout = DeviceLayout::Row,
                ("device_chain", "stack") => settings.device_layout = DeviceLayout::Stack,
                ("select_on_launch", "on" | "off") => settings.select_on_launch = value == "on",
                ("mixer_controls", "field") => settings.mixer_controls = MixerControls::Field,
                ("mixer_controls", "knob") => settings.mixer_controls = MixerControls::Knob,
                ("plugin_editors", "window") => settings.plugin_editors = EditorPlacement::Window,
                ("plugin_editors", "device") => settings.plugin_editors = EditorPlacement::DeviceView,
                ("projects_folder", folder) => settings.projects_folder = folder.to_string(),
                ("templates_folder", folder) => settings.templates_folder = folder.to_string(),
                ("nam_models_folder", folder) => settings.nam_models_folder = folder.to_string(),
                ("irs_folder", folder) => settings.irs_folder = folder.to_string(),
                ("open_projects", "ask") => settings.on_open = OnOpen::Ask,
                ("open_projects", "tab") => settings.on_open = OnOpen::NewTab,
                ("open_projects", "replace") => settings.on_open = OnOpen::Replace,
                // `dialog` is what test builds wrote before the start panel took the dialog's place.
                ("at_start", "panel" | "dialog") => settings.at_start = AtStart::Panel,
                ("at_start", "blank") => settings.at_start = AtStart::Blank,
                ("at_start", "template") => settings.at_start = AtStart::Template,
                ("at_start", "last") => settings.at_start = AtStart::Last,
                ("start_template", path) => settings.start_template = path.to_string(),
                ("stop", "marker") => settings.stop = StopReturn::Marker,
                ("stop", "here") => settings.stop = StopReturn::Here,
                // `play_start` since the selected clip became the default; the old key, which
                // every file got written with "marker" while that was the default, is left alone.
                ("play_start", "marker") => settings.play_from = PlayFrom::Marker,
                ("play_start", "clip") => settings.play_from = PlayFrom::Clip,
                ("play_from", "marker" | "clip") => {}
                ("renderer", "wgpu") => settings.graphics = Graphics::Gpu,
                ("renderer", "glow") => settings.graphics = Graphics::OpenGl,
                ("theme", id) if is_theme_id(id) => settings.theme = id.to_string(),
                ("browser", "on" | "off") => settings.browser_open = value == "on",
                ("browser_side", "left") => settings.browser_side = Side::Left,
                ("browser_side", "right") => settings.browser_side = Side::Right,
                ("browser_width", width) if width.parse::<u32>().is_ok() => {
                    settings.browser_width =
                        width.parse::<u32>().unwrap_or(BROWSER_WIDTH).clamp(BROWSER_WIDTHS.0, BROWSER_WIDTHS.1);
                }
                ("preview_autoplay", "on" | "off") => settings.preview_autoplay = value == "on",
                ("preview_volume", db) if db.parse::<i32>().is_ok() => {
                    settings.preview_volume =
                        db.parse::<i32>().unwrap_or(PREVIEW_VOLUME).clamp(PREVIEW_VOLUMES.0, PREVIEW_VOLUMES.1);
                }
                ("map_layout", "cloud" | "grid") => settings.map_grid = value == "grid",
                ("map_hover_preview", "on" | "off") => settings.map_hover_preview = value == "on",
                ("waveform_scale", "linear" | "db") => settings.waveform_db = value == "db",
                ("map_max_seconds", seconds) if seconds.parse::<u32>().is_ok() => {
                    settings.map_max_seconds = seconds
                        .parse::<u32>()
                        .unwrap_or(MAP_MAX_SECONDS)
                        .clamp(MAP_MAX_SECONDS_RANGE.0, MAP_MAX_SECONDS_RANGE.1);
                }
                ("map_dot_size", size) if size.parse::<u32>().is_ok() => {
                    settings.map_dot_size =
                        size.parse::<u32>().unwrap_or(MAP_DOT_SIZE).clamp(MAP_DOT_SIZES.0, MAP_DOT_SIZES.1);
                }
                ("map_window", "on" | "off") => settings.map_window = value == "on",
                ("map_window_rect", "none") => settings.map_window_rect = None,
                ("map_window_rect", rect) if parse_window_rect(rect).is_some() => {
                    settings.map_window_rect = parse_window_rect(rect);
                }
                ("zoom", percent) if percent.parse::<u32>().is_ok() => {
                    settings.zoom = percent.parse::<u32>().unwrap_or(ZOOM).clamp(ZOOMS.0, ZOOMS.1);
                }
                ("drop_files", "one_track") => settings.drop_separate = false,
                ("drop_files", "separate") => settings.drop_separate = true,
                ("warp_mode", mode) if WarpMode::parse(mode).is_some() => {
                    settings.warp_mode = WarpMode::parse(mode).unwrap_or_default();
                }
                ("audio_host", host) => settings.audio_host = host.to_string(),
                ("audio_device", device) => settings.audio_device = device.to_string(),
                ("audio_sample_rate", rate) if rate.parse::<u32>().is_ok_and(|r| r <= MAX_SAMPLE_RATE) => {
                    settings.audio_sample_rate = rate.parse().unwrap_or(0);
                }
                ("audio_buffer", frames)
                    if frames
                        .parse::<u32>()
                        .is_ok_and(|f| f == 0 || (MIN_BUFFER_FRAMES..=MAX_BUFFER_FRAMES).contains(&f)) =>
                {
                    settings.audio_buffer = frames.parse().unwrap_or(0);
                }
                ("audio_exclusive", "on" | "off") => settings.audio_exclusive = value == "on",
                ("audio_cue_pair", pair) if pair.parse::<u16>().is_ok_and(|p| p <= MAX_CUE_PAIR) => {
                    settings.audio_cue_pair = pair.parse().unwrap_or(0);
                }
                ("audio_cue_volume", db) if db.parse::<i32>().is_ok() => {
                    settings.audio_cue_volume = db.parse::<i32>().unwrap_or(0).clamp(CUE_VOLUMES.0, CUE_VOLUMES.1);
                }
                ("piano_roll_top", "track") => settings.piano_roll_top = PianoRollTop::Track,
                ("piano_roll_top", "window") => settings.piano_roll_top = PianoRollTop::Window,
                ("piano_roll_grid", "own" | "control_bar") => settings.piano_grid_follows = value == "control_bar",
                ("backup", when) if suhina_session::backup::Interval::parse(when).is_some() => {
                    settings.backup = suhina_session::backup::Interval::parse(when).unwrap_or_default();
                }
                ("backup_clean_up", "on" | "off") => settings.backup_clean_up = value == "on",
                ("audio_input", input) => {
                    settings.audio_input = suhina_session::InputChoice::from_text(input).as_text().to_string()
                }
                ("midi_input_off", port) if !port.is_empty() => {
                    if !settings.midi_inputs_off.iter().any(|p| p == port) {
                        settings.midi_inputs_off.push(port.to_string());
                    }
                }
                ("piano_roll_toolbar", size) if ToolbarSize::parse(size).is_some() => {
                    settings.toolbar_size = ToolbarSize::parse(size).unwrap_or_default();
                }
                _ => {
                    warnings.push(format!("settings line {}: {key} = {value} is not a known setting", number + 1));
                }
            }
        }
        (settings, warnings)
    }
}

use suhina_session::audio::{MAX_BUFFER_FRAMES, MAX_CUE_PAIR, MAX_SAMPLE_RATE, MIN_BUFFER_FRAMES};

/// Reads a window's place, `left, top, width, height` in points. A position far off any screen
/// or a size outside [`WINDOW_SIZES`] is refused, so a broken file cannot open a window nowhere.
fn parse_window_rect(text: &str) -> Option<[i32; 4]> {
    let parts: Vec<i32> = text.split(',').map(|p| p.trim().parse::<i32>()).collect::<Result<_, _>>().ok()?;
    let [x, y, w, h] = <[i32; 4]>::try_from(parts).ok()?;
    let on_desktop = |v: i32| (-WINDOW_POSITION..=WINDOW_POSITION).contains(&v);
    let sized = |v: i32| (WINDOW_SIZES.0..=WINDOW_SIZES.1).contains(&v);
    (on_desktop(x) && on_desktop(y) && sized(w) && sized(h)).then_some([x, y, w, h])
}

/// The smallest and largest a window of its own may be, in points, and how far from the origin
/// of the desktop its corner may be.
pub const WINDOW_SIZES: (i32, i32) = (200, 10_000);
const WINDOW_POSITION: i32 = 30_000;

/// Whether `id` can name a theme file: not empty and not a path.
pub fn is_theme_id(id: &str) -> bool {
    !id.is_empty() && !id.contains(['/', '\\', ':']) && id != "." && id != ".."
}

#[cfg(test)]
#[path = "settings_tests.rs"]
mod tests;
