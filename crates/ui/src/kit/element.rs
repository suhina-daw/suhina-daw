// Copyright (C) 2026 The Suhina contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! The elements code draws, and the only way to an element's computed style (ADR 0031 ›
//! Amendment: the element registry).
//!
//! ```ignore
//! let el = KNOB.id("send").response(&response).attr(Checked, on).enter();
//! let boxed = el.paint(&painter, rect); // background, artwork shadows, gradients, border
//! draw_contents(boxed.content);
//! let style = CLIP.drawn(&lane).attr(Color, clip.color).style(); // a drawn item
//! ```
//!
//! An entered element stays in the style tree (and its palette scope) until the value is
//! dropped. Its [`Entered::style`] is its own, found by its node, never the style of an element
//! that encloses it, so a part that forgets to enter its own element cannot paint as its parent.

use egui::{Color32, Painter, Rect, Response};

pub use super::native_css::Computed;

/// What the app knows about an element, given to the sheet as an attribute (`[checked]`,
/// `[color="#ff9a3c"]`).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Attr {
    /// A toggle that is on: a switched-on button, the chosen tool, tab or row.
    Checked,
    /// The selected track, clip, note, device or browser row.
    Selected,
    /// Playing: play while the transport runs, a playing clip.
    Playing,
    /// Recording: record while it records, a clip being recorded.
    Recording,
    /// A track armed for recording.
    Armed,
    /// A muted track, a clip or note that is off, a device that is off.
    Muted,
    /// A colour of the element's own (a track's, a clip's), as `#rrggbb` or `#rrggbbaa`.
    Color,
}
pub use Attr::{Armed, Checked, Color, Muted, Playing, Recording, Selected};

/// The value an attribute takes: on or off for a state, a colour for [`Attr::Color`].
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Value {
    On(bool),
    Color(Color32),
}
impl From<bool> for Value {
    fn from(on: bool) -> Self {
        Self::On(on)
    }
}
impl From<Color32> for Value {
    fn from(color: Color32) -> Self {
        Self::Color(color)
    }
}

/// What a control paints where the sheet sets nothing: today's token values, until the
/// user-agent sheet styles every element (#1019).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Native {
    pub radius: f32,
    pub fill: Color32,
}
impl Default for Native {
    fn default() -> Self {
        Self { radius: 0.0, fill: Color32::TRANSPARENT }
    }
}

/// What [`Entered::paint`] painted: the radius it used, and the rectangle left for the
/// element's contents (the rectangle given, until `padding` exists).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Boxed {
    pub content: Rect,
    pub radius: f32,
}

/// An element of the registry, by name.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Name(&'static str);

impl Name {
    pub const fn name(self) -> &'static str {
        self.0
    }
    fn builder<'a>(self) -> Builder<'a> {
        Builder {
            name: self,
            id: None,
            response: None,
            attributes: [None; 7],
            native: Native::default(),
            variables: Vec::new(),
        }
    }
    /// The element with the id the app gives it (`play`, `device-compressor`); the registry
    /// says whether a theme can select it by that id.
    pub fn id(self, id: &str) -> Builder<'_> {
        self.builder().id(id)
    }
    /// The element drawn as `response`'s widget: its pointer and focus states, and its identity.
    pub fn response(self, response: &Response) -> Builder<'_> {
        self.builder().response(response)
    }
    pub fn attr<'a>(self, attr: Attr, value: impl Into<Value>) -> Builder<'a> {
        self.builder().attr(attr, value)
    }
    pub fn native<'a>(self, native: Native) -> Builder<'a> {
        self.builder().native(native)
    }
    pub fn enter(self) -> Entered {
        self.builder().enter()
    }
    /// A drawn item inside `parent` (a note in its lane, a tick on the ruler), styled by the
    /// parent and its own identity: build it, then take its [`Builder::style`].
    pub fn drawn<'a>(self, parent: &Entered) -> Builder<'a> {
        let _ = parent;
        self.builder()
    }
}

/// An element being described before it is entered.
#[must_use]
pub struct Builder<'a> {
    name: Name,
    id: Option<&'a str>,
    response: Option<&'a Response>,
    attributes: [Option<Value>; 7],
    native: Native,
    variables: Vec<(&'static str, String)>,
}

impl<'a> Builder<'a> {
    pub fn id(mut self, id: &'a str) -> Self {
        self.id = Some(id);
        self
    }
    pub fn response(mut self, response: &'a Response) -> Self {
        self.response = Some(response);
        self
    }
    pub fn attr(mut self, attr: Attr, value: impl Into<Value>) -> Self {
        if let Some(slot) = self.attributes.get_mut(attr as usize) {
            *slot = Some(value.into());
        }
        self
    }
    /// The radius and fill [`Entered::paint`] uses where the sheet sets none.
    pub fn native(mut self, native: Native) -> Self {
        self.native = native;
        self
    }
    /// A custom property the element gives the sheet and its descendants (`--track-colour`).
    pub fn variable(mut self, name: &'static str, value: String) -> Self {
        self.variables.push((name, value));
        self
    }

    /// Puts the element in the style tree and the palette scope until the value is dropped.
    pub fn enter(self) -> Entered {
        let scope = super::style_class::enter_inner(self.name.0, self.id, self.response);
        if let Some(node) = scope.node {
            for (index, value) in self.attributes.iter().enumerate() {
                match (index, value) {
                    (index, Some(Value::On(true))) => {
                        if let Some(state) = host_state(index) {
                            super::native_css::mark_node(node, state);
                        }
                    }
                    (6, Some(Value::Color(color))) => {
                        super::native_css::set_attribute(node, "color", super::native_css::hex(*color));
                    }
                    _ => {}
                }
            }
            for (name, value) in self.variables {
                super::native_css::set_variable(name, value);
            }
        }
        Entered { scope, native: self.native }
    }

    /// The style of a drawn item: the element entered and left at once.
    pub fn style(self) -> Option<Computed> {
        self.enter().style()
    }
}

fn host_state(index: usize) -> Option<super::native_css::HostState> {
    use super::native_css::HostState;
    [
        HostState::Checked,
        HostState::Selected,
        HostState::Playing,
        HostState::Recording,
        HostState::Armed,
        HostState::Muted,
    ]
    .get(index)
    .copied()
}

/// An element in scope.
#[must_use]
pub struct Entered {
    scope: super::style_class::Scope,
    native: Native,
}

impl Entered {
    /// The element's own computed style, when a sheet styles the window.
    pub fn style(&self) -> Option<Computed> {
        self.scope.style()
    }

    /// The element's surface in `rect`: its shadows, background, gradients and border, with
    /// its [`Native`] fallbacks where the sheet sets none. Painted in the order controls have
    /// always painted (#1273 orders paint as CSS does).
    pub fn paint(&self, painter: &Painter, rect: Rect) -> Boxed {
        let radius = super::native_css::surface_with(painter, rect, self.native.radius, self.native.fill, self.style());
        Boxed { content: rect, radius }
    }

    /// [`Entered::paint`] with `native` as the fallbacks, for a fill worked out once the element is
    /// in scope (its hover).
    pub fn paint_with(&self, painter: &Painter, rect: Rect, native: Native) -> Boxed {
        let radius = super::native_css::surface_with(painter, rect, native.radius, native.fill, self.style());
        Boxed { content: rect, radius }
    }
}

/// The registry's element named `name`, for code that has a name at run time.
pub fn named(name: &str) -> Option<Name> {
    ALL.iter().copied().find(|element| element.0 == name)
}

macro_rules! names {
    ($($constant:ident = $name:literal;)*) => {
        $(
            #[doc = concat!("The `", $name, "` element.")]
            pub const $constant: Name = Name($name);
        )*
        /// Every element, as the registry lists them.
        pub const ALL: &[Name] = &[$($constant,)*];
    };
}

names! {
    WINDOW = "window";
    APPLICATION_BAR = "application-bar";
    MENU_BAR = "menu-bar";
    MENU = "menu";
    PROJECT_TABS = "project-tabs";
    TOOLBAR = "toolbar";
    TOOLBAR_MODULE = "toolbar-module";
    STATUS_BAR = "status-bar";
    WORKSPACE = "workspace";
    WORKSPACE_PANEL = "workspace-panel";
    DIVIDER = "divider";
    BROWSER = "browser";
    BROWSER_PLACE = "browser-place";
    BROWSER_ROW = "browser-row";
    BROWSER_PLAY = "browser-play";
    BROWSER_TAG_DOT = "browser-tag-dot";
    BROWSER_PREVIEW_WAVE = "browser-preview-wave";
    SESSION = "session";
    TRACK_COLUMN = "track-column";
    SCENE_COLUMN = "scene-column";
    SCENE = "scene";
    SLOT = "slot";
    ARRANGEMENT = "arrangement";
    TRACK_HEADER = "track-header";
    TRACK_NAME = "track-name";
    TRACK_NUMBER = "track-number";
    FOLD = "fold";
    ADD_TRACK = "add-track";
    CLIP = "clip";
    CLIP_NAME = "clip-name";
    CLIP_WAVEFORM = "clip-waveform";
    CLIP_NOTES = "clip-notes";
    CLIP_GAIN = "clip-gain";
    CLIP_FADE_HANDLE = "clip-fade-handle";
    RULER = "ruler";
    RULER_NUMBER = "ruler-number";
    RULER_TICK = "ruler-tick";
    LOOP_REGION = "loop-region";
    LOOP_LINE = "loop-line";
    PLAYHEAD = "playhead";
    MIXER = "mixer";
    DETAIL = "detail";
    CLIP_EDITOR = "clip-editor";
    EDITOR_TAB = "editor-tab";
    PIANO_ROLL = "piano-roll";
    PIANO_GRID = "piano-grid";
    PIANO_ROW = "piano-row";
    PIANO_ROW_BLACK = "piano-row-black";
    PIANO_KEY = "piano-key";
    PIANO_KEY_BLACK = "piano-key-black";
    PIANO_KEY_C = "piano-key-c";
    PIANO_KEY_HOVER = "piano-key-hover";
    PIANO_KEY_BLACK_HOVER = "piano-key-black-hover";
    PIANO_KEY_LINE = "piano-key-line";
    PIANO_SCALE_MARK = "piano-scale-mark";
    PIANO_SCALE_STRIP = "piano-scale-strip";
    PIANO_RANGE = "piano-range";
    PIANO_CLIP_STRIP = "piano-clip-strip";
    PIANO_TIME_SELECTION = "piano-time-selection";
    PIANO_INSERT_MARK = "piano-insert-mark";
    PIANO_NOTE = "piano-note";
    VELOCITY = "velocity";
    PIANO_TOOL = "piano-tool";
    MIDI_TOOLS = "midi-tools";
    CLIP_INFO = "clip-info";
    SAMPLE_MAP = "sample-map";
    DEVICE_PANEL = "device-panel";
    DEVICE = "device";
    DEVICE_HEADER = "device-header";
    DEVICE_SECTION = "device-section";
    DEVICE_CONTROL = "device-control";
    XY_CONTROL = "xy-control";
    ENVELOPE = "envelope";
    WAVEFORM = "waveform";
    SLIDER = "slider";
    SLIDER_RAIL = "slider-rail";
    SLIDER_FILL = "slider-fill";
    SLIDER_HANDLE = "slider-handle";
    BUTTON = "button";
    VALUE = "value";
    VALUE_FILL = "value-fill";
    VALUE_HANDLE = "value-handle";
    PAN = "pan";
    FADER = "fader";
    FADER_CAP = "fader-cap";
    FADER_RAIL = "fader-rail";
    FADER_SCALE = "fader-scale";
    SEND = "send";
    KNOB = "knob";
    METER = "meter";
    METER_LEVEL = "meter-level";
    METER_VERTICAL = "meter-vertical";
    METER_HORIZONTAL = "meter-horizontal";
    DROPDOWN = "dropdown";
    TEXT_INPUT = "text-input";
    LABEL = "label";
    SEPARATOR = "separator";
    POPUP = "popup";
    PROGRESS = "progress";
    PROGRESS_FILL = "progress-fill";
    SETTINGS = "settings";
    SETTINGS_NAV = "settings-nav";
    DIALOG = "dialog";
    START_PAGE = "start-page";
    PLUGIN_MANAGER = "plugin-manager";
    EXPORT = "export";
    THEME_SWATCH = "theme-swatch";
}

#[cfg(test)]
#[path = "element_tests.rs"]
mod tests;
