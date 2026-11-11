// Copyright (C) 2026 The Suhina contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! The elements code draws, and the only way to an element's computed style (ADR 0031 ›
//! Amendment: the element registry).
//!
//! ```ignore
//! let el = KNOB.id("send").response(&response).attr(Checked, on).enter();
//! let boxed = el.paint(&painter, rect); // shadows, background, gradients, then border and outline
//! draw_contents(boxed.content);
//! let style = CLIP.color(clip.color).style(); // a drawn item
//! ```
//!
//! An entered element stays in the style tree (and its palette scope) until the value is
//! dropped. Its [`Entered::style`] is its own, worked out in the frame that enters it from its
//! parent's and its own identity, never the style of an element that encloses it, so a part that
//! forgets to enter its own element cannot paint as its parent. Kit's shared painters
//! (`mixer_controls`' level bars and pan field, `widgets::button_surface`) take the element their
//! caller entered. [`Entered::paint`] paints in CSS's order; what a region or container paints
//! after its contents (its border, its outline) comes when it is dropped.

use egui::{Color32, Painter, Rect, Response};

use super::native_css::HostState;

pub use super::native_css::Computed;

/// A state the app knows about an element, given to the sheet as an attribute (`[checked]`,
/// `[selected]`). An element's colour is [`Builder::color`].
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
}
pub use Attr::{Armed, Checked, Muted, Playing, Recording, Selected};

/// Every [`Attr`] with the host state that marks it.
const STATES: [(Attr, HostState); 6] = [
    (Checked, HostState::Checked),
    (Selected, HostState::Selected),
    (Playing, HostState::Playing),
    (Recording, HostState::Recording),
    (Armed, HostState::Armed),
    (Muted, HostState::Muted),
];

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
/// element's contents: the rectangle given, less the element's `padding`.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Boxed {
    pub content: Rect,
    pub radius: f32,
}

/// An element of the registry, by name and by its index in the registry (an interned id: the
/// style stack and tree compare and hash it, not the name).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct Name(&'static str, u16);

impl Name {
    pub const fn name(self) -> &'static str {
        self.0
    }
    /// The element's index in the registry, [`ALL`]'s too.
    pub const fn index(self) -> u16 {
        self.1
    }
    /// Whether the element is a region or a container: its background lies behind its controls.
    pub fn is_surface(self) -> bool {
        static SURFACES: std::sync::OnceLock<Vec<bool>> = std::sync::OnceLock::new();
        SURFACES
            .get_or_init(|| ALL.iter().map(|name| suhina_theme_package::elements::is_surface(name.0)).collect())
            .get(usize::from(self.1))
            .copied()
            .unwrap_or(false)
    }
    fn builder<'a>(self) -> Builder<'a> {
        Builder {
            name: self,
            id: None,
            response: None,
            states: 0,
            pointer: 0,
            color: None,
            attributes: Vec::new(),
            sheet: None,
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
    /// [`Name::response`] when there is a response, else the element with no pointer state.
    pub fn response_if(self, response: Option<&Response>) -> Builder<'_> {
        let builder = self.builder();
        match response {
            Some(response) => builder.response(response),
            None => builder,
        }
    }
    pub fn attr<'a>(self, attr: Attr, on: bool) -> Builder<'a> {
        self.builder().attr(attr, on)
    }
    /// The element's own colour (a track's, a clip's), as the sheet's `[color]`.
    pub fn color<'a>(self, color: Color32) -> Builder<'a> {
        self.builder().color(color)
    }
    /// The element with the attribute `name` set to `value` (`[kind="kick"]`).
    pub fn attribute<'a>(self, name: &'static str, value: &'a str) -> Builder<'a> {
        self.builder().attribute(name, value)
    }
    pub fn native<'a>(self, native: Native) -> Builder<'a> {
        self.builder().native(native)
    }
    pub fn pointer<'a>(self, hovered: bool, pressed: bool, disabled: bool) -> Builder<'a> {
        self.builder().pointer(hovered, pressed, disabled)
    }
    /// The element with the custom property `name` (`--track-colour`) for the sheet.
    pub fn variable<'a>(self, name: &'static str, value: String) -> Builder<'a> {
        self.builder().variable(name, value)
    }
    /// The style of a drawn item of this element, with nothing more said about it.
    pub fn style(self) -> Option<Computed> {
        self.builder().style()
    }
    pub fn enter(self) -> Entered {
        self.builder().enter()
    }
}

/// An element being described before it is entered.
#[must_use]
pub struct Builder<'a> {
    name: Name,
    id: Option<&'a str>,
    response: Option<&'a Response>,
    /// The [`Attr`]s that are on, a bit each (`1 << attr as u8`).
    states: u8,
    /// Pointer states given without a response: hover 1, active 2, disabled 4.
    pointer: u16,
    /// A device panel's own sheet, and the context to wake when the engine was idle.
    sheet: Option<(egui::Context, &'a str)>,
    color: Option<Color32>,
    /// Attributes with a value the app gives the element besides its colour (`kind`).
    attributes: Vec<(&'static str, &'a str)>,
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
    /// The state `attr` is `on` or off; an off state sets nothing.
    pub fn attr(mut self, attr: Attr, on: bool) -> Self {
        let bit = 1 << attr as u8;
        if on {
            self.states |= bit;
        } else {
            self.states &= !bit;
        }
        self
    }
    /// The pointer's states for an element drawn without a response: `hovered` (`:hover`),
    /// `pressed` (`:active`), `disabled` (`:disabled`).
    pub fn pointer(mut self, hovered: bool, pressed: bool, disabled: bool) -> Self {
        self.pointer = u16::from(hovered) | u16::from(pressed) << 1 | u16::from(disabled) << 2;
        self
    }
    /// A device panel's own style sheet (`source`), which applies inside the panel only, over the
    /// application's theme ([`super::native_css`]).
    pub fn sheet(mut self, ctx: &egui::Context, source: Option<&'a str>) -> Self {
        self.sheet = source.map(|source| (ctx.clone(), source));
        self
    }
    /// The element's own colour, given to the sheet as `[color="#rrggbb"]` (or `#rrggbbaa`).
    pub fn color(mut self, color: Color32) -> Self {
        self.color = Some(color);
        self
    }
    /// The attribute `name` with `value`, for the sheet (`sample-point[kind="kick"]`).
    pub fn attribute(mut self, name: &'static str, value: &'a str) -> Self {
        self.attributes.push((name, value));
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
        let Self { name, id, response, states, pointer, color, attributes, native, variables, sheet } = self;
        let sheet_wake = sheet.clone();
        let scope = super::style_class::enter_inner(name, id, response, |node| {
            super::native_css::mark_node_bits(node, pointer);
            for (attr, state) in STATES {
                if states & (1 << attr as u8) != 0 {
                    super::native_css::mark_node(node, state);
                }
            }
            if let Some(color) = color {
                super::native_css::set_attribute(node, "color", super::native_css::hex(color));
            }
            for (attribute, value) in attributes {
                super::native_css::set_attribute(node, attribute, value.to_owned());
            }
            for (variable, value) in variables {
                super::native_css::set_variable(node, variable, value);
            }
            if let Some((ctx, source)) = sheet {
                super::native_css::device_sheet(Some(node), &ctx, Some(source));
            }
        });
        // A sheet asked for while the engine is idle starts it on the next frame.
        if scope.node.is_none()
            && let Some((ctx, source)) = sheet_wake
        {
            super::native_css::device_sheet(None, &ctx, Some(source));
        }
        Entered { scope, native, name, rest: std::cell::RefCell::new(None) }
    }

    /// The style of a drawn item: the element entered and left at once.
    pub fn style(self) -> Option<Computed> {
        self.enter().style()
    }
}

/// An element in scope.
#[must_use]
pub struct Entered {
    scope: super::style_class::Scope,
    native: Native,
    name: Name,
    /// What the element paints once its contents are ([`Entered::paint`]).
    rest: std::cell::RefCell<Option<Rest>>,
}

/// The part of an element's box painted after its contents: where, and how.
struct Rest {
    painter: Painter,
    rect: Rect,
    radius: f32,
    style: Computed,
    outer: Option<Rect>,
    control: bool,
}

impl Rest {
    fn paint(self) {
        super::native_css::box_end(&self.painter, self.rect, self.radius, &self.style, self.outer, self.control);
    }
}

impl Drop for Entered {
    fn drop(&mut self) {
        if let Some(rest) = self.rest.get_mut().take() {
            rest.paint();
        }
    }
}

impl Entered {
    /// The element's own computed style, when a sheet styles the window.
    pub fn style(&self) -> Option<Computed> {
        self.scope.style()
    }

    /// The element's box in `rect`, in CSS's order: its outer shadows, its background and
    /// gradients, then for a control its inset shadows and border; for a region or container
    /// those come when it leaves scope, after its contents, and so does any element's outline.
    /// Its [`Native`] fallbacks stand where the sheet sets none. Its contents go in
    /// [`Boxed::content`], the box less its `padding`.
    pub fn paint(&self, painter: &Painter, rect: Rect) -> Boxed {
        self.paint_with(painter, rect, self.native)
    }

    /// [`Entered::paint`] with `native` as the fallbacks, for a fill worked out once the element is
    /// in scope (its hover).
    pub fn paint_with(&self, painter: &Painter, rect: Rect, native: Native) -> Boxed {
        // A box painted before by this element is finished first.
        if let Some(rest) = self.rest.borrow_mut().take() {
            rest.paint();
        }
        let style = self.style();
        let control = !self.name.is_surface();
        let outer = super::native_css::outer_clip(self.scope.node);
        super::native_css::set_box(self.scope.node, rect, painter.clip_rect());
        let radius =
            super::native_css::box_begin(painter, rect, native.radius, native.fill, style.as_ref(), outer, control);
        let content = match &style {
            Some(style) => {
                let [top, right, bottom, left] = style.padding();
                let inner = Rect::from_min_max(rect.min + egui::vec2(left, top), rect.max - egui::vec2(right, bottom));
                if inner.is_positive() { inner } else { Rect::from_center_size(inner.center(), egui::Vec2::ZERO) }
            }
            None => rect,
        };
        if let Some(style) = style.filter(|style| super::native_css::has_end(style, control)) {
            *self.rest.borrow_mut() = Some(Rest { painter: painter.clone(), rect, radius, style, outer, control });
        }
        Boxed { content, radius }
    }

    /// Says where the element is on screen without painting it (a panel, the toolbar): the box
    /// that clips its children's outer shadows when it hides its overflow.
    pub fn occupies(&self, rect: Rect, clip: Rect) {
        super::native_css::set_box(self.scope.node, rect, clip);
    }

    /// The element's `color`, or `fallback` where the sheet sets none.
    pub fn color_or(&self, fallback: Color32) -> Color32 {
        self.style().filter(|s| s.authored.color).map_or(fallback, |s| s.color)
    }

    /// The element's computed `color`, the user-agent tier's included: for an element whose
    /// colour that tier gives (`knob-body`, `fader-cap`, `sample-point[kind]`), so a colour a
    /// theme gives an element around it does not reach it. `fallback` only where no sheet styles
    /// the window.
    pub fn computed_color_or(&self, fallback: Color32) -> Color32 {
        self.style().map_or(fallback, |s| s.color)
    }

    /// The element's `background-color`, or `fallback` where the sheet sets none.
    pub fn background_or(&self, fallback: Color32) -> Color32 {
        self.style().filter(|s| s.authored.background).map_or(fallback, |s| s.background)
    }

    /// The element's corner radius, or `fallback` where the sheet sets none.
    pub fn radius_or(&self, fallback: f32) -> f32 {
        self.style().filter(|s| s.authored.radius).map_or(fallback, |s| s.radius)
    }

    /// The element's `font-size`, or `fallback` where the sheet sets none.
    pub fn font_size_or(&self, fallback: f32) -> f32 {
        self.style().filter(|s| s.authored.font_size).map_or(fallback, |s| s.font_size)
    }

    /// The thickness the sheet gives the element (`height`), or `fallback`.
    pub fn thickness_or(&self, fallback: f32) -> f32 {
        self.style().and_then(|style| style.height).unwrap_or(fallback)
    }

    /// The element's shadows, gradients and inset shadows in `rect`, with `radius` where the sheet
    /// gives none: for a surface whose fill its caller paints. Whether it painted anything.
    pub fn paint_background(&self, painter: &Painter, rect: Rect, radius: f32) -> bool {
        self.occupies(rect, painter.clip_rect());
        self.style().is_some_and(|style| super::native_css::background_rounded(painter, rect, radius, style))
    }

    /// The element's gradients and inset shadows without its outer shadows: for a surface whose
    /// artwork casts them itself ([`Entered::artwork_shadows`]).
    pub fn paint_gradients(&self, painter: &Painter, rect: Rect, radius: f32) -> bool {
        self.style().is_some_and(|style| super::native_css::gradients_rounded(painter, rect, radius, style))
    }

    /// The element's shadows cast by artwork: drawn into `slot`, a place reserved under the
    /// artwork, as tinted copies of its `mesh`. Whether there were any.
    pub fn artwork_shadows(&self, painter: &Painter, slot: egui::layers::ShapeIdx, mesh: &egui::Mesh) -> bool {
        self.style().is_some_and(|style| super::native_css::artwork_shadows(painter, slot, mesh, &style))
    }

    /// A level's fill (`meter-level`, `value-fill`) over the whole `scale`, which the caller clips
    /// to the measured part. Whether the sheet gave one.
    pub fn level_fill(&self, painter: &Painter, scale: Rect) -> bool {
        self.style().is_some_and(|style| super::native_css::level_fill(painter, scale, &style))
    }

    /// A level's outer shadows around the part that is lit.
    pub fn level_glow(&self, painter: &Painter, lit: Rect) {
        if let Some(style) = self.style() {
            super::native_css::level_glow(painter, lit, &style);
        }
    }

    /// Keeps the element's text readable on `background` while it is in scope (see
    /// [`super::style_class::readable_on`]); the ink, when it changed the text.
    pub fn readable_on(&self, background: Color32) -> Option<Color32> {
        super::style_class::readable_on(self.style(), background)
    }

    /// egui's own widgets in `ui` styled as this element: its colour, font size, radius, border and
    /// fill, and the sheet's `button` and `text-input` rules for the widgets inside it.
    pub fn ui<'u>(&self, ui: &'u mut egui::Ui) -> super::style_class::UiStyle<'u> {
        super::style_class::ui_for(ui, self.style(), self.is_surface())
    }

    /// Whether the element is a region or a container: its background lies behind its controls.
    pub fn is_surface(&self) -> bool {
        self.name.is_surface()
    }

    /// The id the app gave the element (`play`, `device-compressor`), when it gave one.
    pub fn id(&self) -> Option<String> {
        self.scope.node.and_then(super::native_css::node_name)
    }

    /// The element's registry name.
    pub fn name(&self) -> Name {
        self.name
    }
}

/// The registry's element named `name`, for code that has a name at run time.
pub fn named(name: &str) -> Option<Name> {
    static BY_NAME: std::sync::OnceLock<std::collections::HashMap<&'static str, Name>> = std::sync::OnceLock::new();
    BY_NAME.get_or_init(|| ALL.iter().map(|name| (name.0, *name)).collect()).get(name).copied()
}

/// The registry's element named `name`, for tests that name elements as strings.
#[cfg(test)]
pub(crate) fn test_el(name: &str) -> Name {
    named(name).unwrap_or_else(|| panic!("no element {name}"))
}

macro_rules! names {
    ($($constant:ident = $name:literal;)*) => {
        /// The elements' indices, in the registry's order.
        #[allow(non_camel_case_types, clippy::upper_case_acronyms)]
        #[repr(u16)]
        enum Index { $($constant,)* }
        $(
            #[doc = concat!("The `", $name, "` element.")]
            pub const $constant: Name = Name($name, Index::$constant as u16);
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
    SAMPLE_POINT = "sample-point";
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
    KNOB_BODY = "knob-body";
    KNOB_POINTER = "knob-pointer";
    ICON = "icon";
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
