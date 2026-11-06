// Copyright (C) 2026 The Suhina contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! The user's settings: what they are, their defaults, and the text file they are kept in
//! (`key = value` lines, so a person can read and fix it). The app decides where the file lives.

mod fields;
mod gradient_editor;
pub mod keymap;
pub mod plugin_manager;
mod settings_rows;
pub mod settings_view;
mod skin_editor;
pub mod themes;
pub mod track_sizes;
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

/// What opens when Suhina starts without a project given (Settings › Projects and backups › Startup project).
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

/// How the piano roll's keys are drawn when the theme does not set them.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum PianoKeyStyle {
    /// Light keys, the default.
    #[default]
    Bone,
    /// Keys between the two.
    Stone,
    /// Dark keys.
    Night,
}

impl PianoKeyStyle {
    pub const ALL: [Self; 3] = [Self::Bone, Self::Stone, Self::Night];

    /// The name the menu and the Settings row show.
    pub fn name(self) -> &'static str {
        match self {
            Self::Bone => "Bone",
            Self::Stone => "Stone",
            Self::Night => "Night",
        }
    }

    /// The value the settings file keeps.
    pub fn param(self) -> &'static str {
        match self {
            Self::Bone => "bone",
            Self::Stone => "stone",
            Self::Night => "night",
        }
    }

    pub fn parse(value: &str) -> Option<Self> {
        Self::ALL.into_iter().find(|style| style.param() == value)
    }
}

/// Which of the piano roll's keys show their names.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum PianoKeyNames {
    /// Every C, the default.
    #[default]
    OnlyC,
    /// The white keys.
    White,
    /// Every key.
    All,
}

impl PianoKeyNames {
    pub const ALL: [Self; 3] = [Self::OnlyC, Self::White, Self::All];

    /// The name the menu and the Settings row show.
    pub fn name(self) -> &'static str {
        match self {
            Self::OnlyC => "Only C",
            Self::White => "White keys",
            Self::All => "All keys",
        }
    }

    /// The value the settings file keeps.
    pub fn param(self) -> &'static str {
        match self {
            Self::OnlyC => "only_c",
            Self::White => "white",
            Self::All => "all",
        }
    }

    pub fn parse(value: &str) -> Option<Self> {
        Self::ALL.into_iter().find(|names| names.param() == value)
    }
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
/// The sizes the map's cluster labels can take, in percent: off, then smaller to full size.
pub const MAP_LABEL_SIZES: [u32; 4] = [0, 50, 75, 100];
pub const MAP_LABEL_SIZE: u32 = 100;
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

/// How the Settings window explains each setting: its name says what it does, and its one
/// sentence of description shows as the name's tooltip, as a ? beside the name to point at, or
/// under the name.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Descriptions {
    #[default]
    Tooltip,
    Icon,
    Shown,
}

impl Descriptions {
    pub fn param(self) -> &'static str {
        match self {
            Self::Tooltip => "tooltip",
            Self::Icon => "icon",
            Self::Shown => "shown",
        }
    }

    pub fn parse(text: &str) -> Option<Self> {
        [Self::Tooltip, Self::Icon, Self::Shown].into_iter().find(|d| d.param() == text)
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Settings {
    /// User-owned PNG beneath translucent surfaces; never captures the desktop.
    pub background_image: String,
    pub animations: bool,
    /// How the Settings window explains each setting (Help, beside Close): a tooltip on its name,
    /// a ? beside it, or the sentence under it.
    pub setting_descriptions: Descriptions,
    /// The Arrangement view's track headers: on a side of the user's choosing, or `None` where
    /// the theme puts them (on the right when it says nothing).
    pub track_headers: Option<Side>,
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
    /// Launching a clip in the Launcher view selects it, so the detail area shows it
    /// (`docs/spec/ui-session-view.md` › Launching and selecting).
    pub select_on_launch: bool,
    /// The Launcher mixer's height, the detail area's layout and double-clicks that make clips.
    pub view: crate::settings::view_settings::ViewSettings,
    /// Whether and where to check for updates (`docs/spec/updates.md`).
    pub updates: crate::settings::update_settings::UpdateSettings,
    /// What stays above the piano roll pulled to the top.
    pub piano_roll_top: PianoRollTop,
    /// How large the piano roll's toolbar is drawn.
    pub toolbar_size: ToolbarSize,
    /// How the piano roll's keys are drawn, and which of them are named.
    pub piano_key_style: PianoKeyStyle,
    pub piano_key_names: PianoKeyNames,
    /// A track's volume and pan as a field or as a knob (Settings › Launcher and mixer).
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
    /// Where themes are kept; empty for `themes` in Suhina's data folder. `SUHINA_THEMES` wins
    /// over it.
    pub themes_folder: String,
    /// Where Suhina keeps its settings, keys, recent projects and themes; empty for the default.
    /// Not written in the settings file, which is kept there: the app names it beside the default.
    pub data_folder: String,
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
    /// The size of the sample map's cluster labels, in percent of their full size; 0 hides them
    /// (Labels in the map's top bar, #864).
    pub map_label_size: u32,
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
    /// Last explicitly applied device per resolved audio host; None means its default.
    pub audio_devices: std::collections::BTreeMap<String, Option<String>>,
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
    /// What each audio device keeps (Settings › Audio, ADR 0017), by
    /// [`suhina_session::OutputChoice::device_key`]: the input chosen with it ("off", "default"
    /// or a name) and its default track input (`audio.setTrackInput`: "none", "3" or "1/2").
    /// Choosing the device again brings both back.
    pub audio_device_inputs: std::collections::BTreeMap<String, [String; 2]>,
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
            setting_descriptions: Descriptions::Tooltip,
            background_image: String::new(),
            track_headers: None,
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
            piano_key_style: PianoKeyStyle::default(),
            piano_key_names: PianoKeyNames::default(),
            mixer_controls: MixerControls::Field,
            plugin_editors: EditorPlacement::Window,
            theme: crate::kit::theme::BUILT_IN.into(),
            projects_folder: String::new(),
            templates_folder: String::new(),
            nam_models_folder: String::new(),
            themes_folder: String::new(),
            data_folder: String::new(),
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
            map_label_size: MAP_LABEL_SIZE,
            map_window: false,
            map_window_rect: None,
            waveform_db: false,
            audio_host: String::new(),
            audio_device: String::new(),
            audio_devices: Default::default(),
            audio_sample_rate: 0,
            audio_buffer: 0,
            audio_exclusive: false,
            audio_cue_pair: 0,
            audio_cue_volume: 0,
            audio_input: "off".into(),
            audio_device_inputs: Default::default(),
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

/// The longest audio device key (`host|device`) and input device name the settings keep for a
/// card; longer ones are not written, so what is written is read back.
const MAX_DEVICE_KEY: usize = 2048;
const MAX_DEVICE_NAME: usize = 1024;
/// The longest `audio_device_inputs` line read: room for every card at the longest names.
const MAX_DEVICE_INPUTS_LINE: usize = 262_144;

impl Settings {
    pub(crate) fn remember_output(&mut self, choice: &suhina_session::OutputChoice, default_host: Option<&str>) {
        let Some(host) = choice.host.as_deref().or(default_host) else { return };
        let key = host.trim().to_lowercase();
        if self.audio_devices.len() < 64 || self.audio_devices.contains_key(&key) {
            self.audio_devices.insert(key, choice.device.clone());
        }
    }

    pub(crate) fn remembered_device(&self, host: Option<&str>, default_host: Option<&str>) -> Option<Option<String>> {
        let key = host.or(default_host)?.trim().to_lowercase();
        let active = self.output_choice();
        if active.host.as_deref().or(default_host).is_some_and(|h| h.eq_ignore_ascii_case(&key)) {
            return Some(active.device);
        }
        self.audio_devices.get(&key).cloned()
    }

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

    /// The default track input of the audio device these settings choose: none while Input is
    /// off or when none was chosen for the device.
    pub fn track_input(&self) -> suhina_engine::model::InputChannels {
        let choice = self.output_choice();
        if choice.input == suhina_session::InputChoice::Off {
            return suhina_engine::model::InputChannels::None;
        }
        self.audio_device_inputs
            .get(&choice.device_key())
            .and_then(|[_, track]| suhina_session::audio::input_channels_from_text(track))
            .unwrap_or_default()
    }

    /// What every audio device keeps (`Projects::set_device_inputs`, at start).
    pub fn device_inputs(&self) -> std::collections::BTreeMap<String, suhina_session::DeviceInputs> {
        self.audio_device_inputs
            .iter()
            .map(|(key, [input, track])| {
                let kept = suhina_session::DeviceInputs {
                    input: suhina_session::InputChoice::from_text(input),
                    track_input: suhina_session::audio::input_channels_from_text(track).unwrap_or_default(),
                };
                (key.clone(), kept)
            })
            .collect()
    }

    /// Keeps what every audio device keeps as the session has it, after a change.
    pub(crate) fn keep_device_inputs(
        &mut self,
        inputs: &std::collections::BTreeMap<String, suhina_session::DeviceInputs>,
    ) {
        self.audio_device_inputs = inputs
            .iter()
            .filter(|(key, kept)| key.len() <= MAX_DEVICE_KEY && kept.input.as_text().len() <= MAX_DEVICE_NAME)
            .take(suhina_session::MAX_DEVICE_INPUTS)
            .map(|(key, kept)| {
                let texts =
                    [kept.input.as_text().to_string(), suhina_session::audio::input_channels_text(kept.track_input)];
                (key.clone(), texts)
            })
            .collect();
    }

    /// The input `device` was last chosen with, when it was chosen before.
    pub(crate) fn remembered_input(
        &self,
        device: &suhina_session::OutputChoice,
    ) -> Option<suhina_session::InputChoice> {
        self.audio_device_inputs
            .get(&device.device_key())
            .map(|[input, _]| suhina_session::InputChoice::from_text(input))
    }

    /// The setting the Default track input row changes: the one of the audio device chosen,
    /// "none" until one is chosen for it. `None` while Input is off.
    pub(crate) fn track_input_slot(&mut self) -> Option<&mut String> {
        let choice = self.output_choice();
        if choice.input == suhina_session::InputChoice::Off {
            return None;
        }
        let input = choice.input.as_text().to_string();
        let [kept_input, track] =
            self.audio_device_inputs.entry(choice.device_key()).or_insert_with(|| [input.clone(), "none".into()]);
        *kept_input = input;
        Some(track)
    }

    /// Keeps `input` as the default track input of the audio device chosen; nothing while
    /// Input is off.
    pub(crate) fn set_track_input(&mut self, input: suhina_engine::model::InputChannels) {
        if let Some(slot) = self.track_input_slot() {
            *slot = suhina_session::audio::input_channels_text(input);
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
        let lines = |fields: &[fields::Field]| -> String {
            fields.iter().map(|f| format!("{} = {}\n", f.key, (f.write)(self))).collect()
        };
        "# Suhina settings; changed from Options > Settings.\n".to_string()
            + &lines(fields::FIELDS)
            + &self.view.to_text()
            + &lines(fields::LATER)
            + &format!("mcp_write_folders = {}\n", serde_json::json!(self.mcp_write_folders))
            + &lines(fields::LAST)
            + &self.updates.to_text()
            + &format!("audio_devices = {}\n", serde_json::json!(self.audio_devices))
            + &format!("audio_device_inputs = {}\n", serde_json::json!(self.audio_device_inputs))
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
            if fields::field(key).is_some_and(|f| (f.read)(&mut settings, value)) {
                continue;
            }
            match (key, value) {
                // `play_start` since the selected clip became the default; the old key, which
                // every file got written with "marker" while that was the default, is left alone.
                ("play_from", "marker" | "clip") => {}
                ("mcp_write_folders", folders) => match serde_json::from_str::<Vec<String>>(folders) {
                    Ok(folders) => settings.mcp_write_folders = folders,
                    Err(_) => {
                        warnings.push(format!("settings line {}: expected a list of MCP write folders", number + 1))
                    }
                },
                ("audio_devices", value) if value.len() <= 32_768 => {
                    if let Ok(devices) =
                        serde_json::from_str::<std::collections::BTreeMap<String, Option<String>>>(value)
                    {
                        settings.audio_devices = devices
                            .into_iter()
                            .filter(|(host, device)| {
                                host.len() <= 256 && device.as_ref().is_none_or(|d| d.len() <= 1024)
                            })
                            .take(64)
                            .map(|(host, device)| (host.to_lowercase(), device))
                            .collect();
                    } else {
                        warnings.push(format!("settings line {}: invalid audio device history", number + 1));
                    }
                }
                // Retired: the piano roll's grid is no longer linked to the control bar.
                ("piano_roll_grid", "own" | "control_bar") => {}
                ("audio_device_inputs", value) if value.len() <= MAX_DEVICE_INPUTS_LINE => {
                    match serde_json::from_str::<std::collections::BTreeMap<String, [String; 2]>>(value) {
                        Ok(inputs) => {
                            let listed = inputs.len();
                            settings.audio_device_inputs = inputs
                                .into_iter()
                                .filter(|(key, [input, _])| {
                                    !key.trim().is_empty()
                                        && key.len() <= MAX_DEVICE_KEY
                                        && input.len() <= MAX_DEVICE_NAME
                                })
                                .filter_map(|(key, [input, track])| {
                                    let track = suhina_session::audio::input_channels_from_text(&track)?;
                                    let input = suhina_session::InputChoice::from_text(&input).as_text().to_string();
                                    Some((
                                        key.trim().to_lowercase(),
                                        [input, suhina_session::audio::input_channels_text(track)],
                                    ))
                                })
                                .take(suhina_session::MAX_DEVICE_INPUTS)
                                .collect();
                            if settings.audio_device_inputs.len() < listed {
                                warnings.push(format!(
                                    "settings line {}: an audio device's inputs that could not be read were left out",
                                    number + 1
                                ));
                            }
                        }
                        Err(_) => warnings.push(format!("settings line {}: invalid audio device inputs", number + 1)),
                    }
                }
                ("midi_input_off", port) if !port.is_empty() => {
                    if !settings.midi_inputs_off.iter().any(|p| p == port) {
                        settings.midi_inputs_off.push(port.to_string());
                    }
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

/// What a saved built-in theme that has since gone or changed its id is called now.
fn renamed_theme(id: &str) -> Option<String> {
    Some(match id {
        // Editor Dark's colours are Suhina Studio's Darker variant.
        "editor-dark" => "suhina-studio@darker".into(),
        "suhina-dark" | "suhina-studio-dark" | "dracula" | "solarized-dark" | "suhina-forest-amber" => {
            crate::kit::theme::BUILT_IN.into()
        }
        "suhina-studio-light" => "suhina-studio@light".into(),
        _ => format!("suhina-studio@{}", id.strip_prefix("suhina-dark@")?),
    })
}

/// Whether `id` can name a theme file: not empty and not a path.
pub fn is_theme_id(id: &str) -> bool {
    !id.is_empty() && !id.contains(['/', '\\', ':']) && id != "." && id != ".."
}

#[cfg(test)]
#[path = "settings_tests.rs"]
mod tests;
