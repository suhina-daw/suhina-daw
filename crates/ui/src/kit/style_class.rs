// Copyright (C) 2026 The Suhina contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! Stable native CSS classes. A scoped palette lets existing painter code inherit styles.
use super::{skin, theme};
use std::cell::RefCell;

pub const CLASSES: &[&str] = &[
    "window",
    "workspace",
    "workspace-panel",
    "application-bar",
    "menu-bar",
    "project-tabs",
    "toolbar",
    "toolbar-module",
    "status-bar",
    "browser",
    "session",
    "meter-vertical",
    "meter-horizontal",
    "arrangement",
    "mixer",
    "track-header",
    "track-name",
    "clip",
    "clip-name",
    "clip-waveform",
    "clip-notes",
    "scene",
    "scene-column",
    "track-column",
    "slot",
    "detail",
    "clip-editor",
    "piano-roll",
    "device-panel",
    "device",
    "device-header",
    "slider",
    "slider-rail",
    "slider-fill",
    "slider-handle",
    "device-control",
    "device-section",
    "envelope",
    "waveform",
    "xy-control",
    "label",
    "separator",
    "settings",
    "dialog",
    "sample-map",
    "button",
    "fader",
    "fader-scale",
    "pan",
    "send",
    "add-track",
    "ruler",
    "loop-region",
    "playhead",
    "divider",
    "clip-gain",
    "clip-fade-handle",
    "piano-roll",
    "piano-grid",
    "piano-row",
    "piano-row-black",
    "piano-key",
    "piano-key-black",
    "piano-key-c",
    "piano-key-hover",
    "piano-key-black-hover",
    "piano-key-line",
    "piano-scale-mark",
    "piano-scale-strip",
    "piano-range",
    "piano-note",
    "velocity",
    "piano-tool",
    "midi-tools",
    "editor-tab",
    "start-page",
    "plugin-manager",
    "export",
    "clip-info",
    "fader-cap",
    "fader-rail",
    "knob",
    "meter",
    "meter-level",
    "value",
    "value-fill",
    "value-handle",
    "menu",
    "popup",
    "dropdown",
    "text-input",
    "browser-place",
    "browser-row",
    "browser-play",
    "browser-tag-dot",
    "browser-preview-wave",
    "track-number",
    "fold",
    "loop-line",
    "ruler-number",
    "ruler-tick",
    "settings-nav",
    "progress",
    "progress-fill",
    // Families: one class a group of parts shares, for a rule over all of them (`families`).
    "value-control",
    "track-button",
    "transport-button",
    "list-row",
    "timeline-part",
];

/// The families an element of `class` (named `name`) belongs to besides its own class, so one
/// rule reaches every part of a kind: `.value-control` every value a user drags (levels, pans,
/// faders, sends, knobs, sliders, a device's parameters), `.track-button` a track's own buttons,
/// `.transport-button` the transport's, `.list-row` the browser's rows and places,
/// `.timeline-part` the ruler, loop and play position.
pub fn families(class: &str, name: Option<&str>) -> &'static [&'static str] {
    match (class, name) {
        ("value" | "pan" | "fader" | "send" | "knob" | "slider" | "device-control" | "clip-gain", _) => {
            &["value-control"]
        }
        ("button", Some("solo" | "cue" | "arm" | "track-on" | "monitor" | "automation")) => &["track-button"],
        ("button", Some("play" | "stop" | "record" | "metronome-toggle")) => &["transport-button"],
        ("browser-row" | "browser-place", _) => &["list-row"],
        ("ruler" | "loop-region" | "loop-line" | "playhead", _) => &["timeline-part"],
        _ => &[],
    }
}
thread_local! {static STACK: RefCell<Vec<String>> = const {RefCell::new(Vec::new())};}

pub fn valid_selector(selector: &str) -> bool {
    let parts: Vec<_> = selector.split_whitespace().collect();
    !parts.is_empty()
        && parts.len() <= 4
        && parts.iter().all(|part| part.strip_prefix('.').is_some_and(|c| CLASSES.contains(&c)))
}
pub fn valid_property(key: &str, value: &str) -> bool {
    match key {
        "--suhina-track-artwork" => matches!(value, "on" | "off"),
        "--suhina-appearance" => matches!(value, "native" | "knob" | "field"),
        "--suhina-value-position" => {
            matches!(value, "right" | "left" | "top" | "bottom" | "inside" | "center" | "centre" | "none" | "hidden")
        }
        "--suhina-track-headers" => matches!(value, "left" | "right"),
        "--suhina-header-tint" => value.trim().parse::<u8>().is_ok_and(|v| v <= 100),
        "--suhina-param-columns" => value.trim().parse::<u8>().is_ok_and(|v| (1..=8).contains(&v)),
        "--suhina-value-decimals" => matches!(value.trim(), "0" | "1" | "2"),
        "--suhina-value-unit" => {
            let unit = value.trim();
            unit.len() >= 2
                && unit.starts_with(['"', '\''])
                && unit.ends_with(['"', '\''])
                && unit.chars().count() <= 10
        }
        "--suhina-gap" => px(value).is_some_and(|v| v <= 16),
        "--suhina-param-label" => matches!(value, "left" | "top" | "hidden"),
        "--suhina-param-row" => px(value).is_some_and(|v| (16..=160).contains(&v)),
        "--suhina-compact-below" => px(value).is_some_and(|v| v <= 200),
        "--suhina-track-height" => px(value).is_some_and(|v| (MIN_TRACK_HEIGHT..=MAX_TRACK_HEIGHT).contains(&v)),
        "--suhina-header-min-width" => value
            .strip_suffix("px")
            .and_then(|v| v.trim().parse::<u16>().ok())
            .is_some_and(|v| (160..=520).contains(&v)),
        "color" | "background-color" | "border-color" => theme::parse_hex(value).is_some(),
        "border-radius" => value.strip_suffix("px").and_then(|v| v.parse::<u8>().ok()).is_some_and(|v| v <= 32),
        "font-size" => {
            value.strip_suffix("px").and_then(|v| v.parse::<u8>().ok()).is_some_and(|v| (7..=32).contains(&v))
        }
        _ => false,
    }
}
fn matches(selector: &str, stack: &[String]) -> bool {
    let parts: Vec<_> = selector.split_whitespace().filter_map(|s| s.strip_prefix('.')).collect();
    // A part names the class in scope or one of its families.
    let is = |class: &str, part: &str| class == part || families(class, None).contains(&part);
    if !parts.last().copied().zip(stack.last()).is_some_and(|(part, class)| is(class, part)) {
        return false;
    }
    let mut at = 0;
    for part in parts {
        let Some(index) = stack.get(at..).and_then(|s| s.iter().position(|c| is(c, part))) else {
            return false;
        };
        at += index + 1;
    }
    true
}
pub fn property(key: &str) -> Option<String> {
    let skin = skin::current();
    STACK.with(|stack| {
        let stack = stack.borrow();
        // Native custom properties inherit from the nearest authored ancestor.
        // Ordinary paint properties inherit through the scoped palette instead.
        let first = if key.starts_with("--") { 1 } else { stack.len() };
        (first..=stack.len()).rev().find_map(|end| {
            skin.styles
                .iter()
                .filter(|(selector, _)| matches(selector, &stack[..end]))
                .filter_map(|(selector, properties)| {
                    properties.get(key).map(|value| (selector.split_whitespace().count(), value))
                })
                .max_by_key(|(specificity, _)| *specificity)
                .map(|(_, value)| value.clone())
        })
    })
}

pub struct Scope {
    palette: theme::Palette,
    revision: u64,
    native: bool,
}
impl Drop for Scope {
    fn drop(&mut self) {
        if theme::palette_revision() == self.revision {
            theme::local_palette(self.palette);
        }
        if self.native {
            super::native_css::leave();
        }
        STACK.with(|s| {
            s.borrow_mut().pop();
        });
    }
}
pub fn enter(class: &str) -> Scope {
    enter_inner(class, None, None)
}
pub fn enter_named(class: &str, name: &str) -> Scope {
    enter_inner(class, Some(name), None)
}
pub fn enter_response(class: &str, response: &egui::Response) -> Scope {
    enter_inner(class, None, Some(response))
}
pub fn enter_named_response(class: &str, name: &str, response: &egui::Response) -> Scope {
    enter_inner(class, Some(name), Some(response))
}

/// A shared surface may be called inside a complete control scope or on its own.
pub fn control(class: &str, response: &egui::Response) -> Option<Scope> {
    (!STACK.with(|stack| stack.borrow().last().is_some_and(|current| current == class)))
        .then(|| enter_response(class, response))
}
fn enter_inner(class: &str, name: Option<&str>, response: Option<&egui::Response>) -> Scope {
    let native = super::native_css::enter(class, name, response);
    let saved = theme::palette();
    STACK.with(|s| s.borrow_mut().push(class.into()));
    let mut palette = saved;
    if let Some(color) = property("color").as_deref().and_then(theme::parse_hex) {
        set_foreground(&mut palette, color, class);
    }
    if let Some(color) = property("background-color").as_deref().and_then(theme::parse_hex) {
        set_background(&mut palette, color, class);
    }
    if let Some(color) = property("border-color").as_deref().and_then(theme::parse_hex) {
        palette.border = color;
        palette.border_strong = color;
    }
    if let Some(values) = STACK.with(|stack| super::native_css::scoped(&stack.borrow())) {
        if values.authored.color {
            set_foreground(&mut palette, values.color, class);
        }
        if values.authored.border {
            palette.border = values.border;
            palette.border_strong = values.border;
        }
        if values.authored.background {
            set_background(&mut palette, values.background, class);
        }
        // `accent-color` reaches every control drawn in the scope (a knob's arc, a lit button),
        // the parameter list of a built-in effect too.
        if values.authored.accent {
            palette.accent = values.accent;
        }
    }
    theme::local_palette(palette);
    Scope { palette: saved, revision: theme::palette_revision(), native }
}
/// Views and bars: their background is the surface behind their controls, not the controls' own.
const SURFACES: &[&str] = &[
    "window",
    "workspace",
    "workspace-panel",
    "application-bar",
    "menu-bar",
    "project-tabs",
    "toolbar",
    "status-bar",
    "browser",
    "session",
    "arrangement",
    "mixer",
    "detail",
    "clip-editor",
    "piano-roll",
    "settings",
    "dialog",
    "sample-map",
    // Containers of controls: a selected header's background never fills its buttons.
    "track-header",
    "track-column",
    "mixer-strip",
    "device",
    "device-panel",
    "midi-tools",
];

/// A background for `class`: a view's sets the surfaces inside it, and a control's its own fill
/// too. So a view's background never repaints the buttons drawn on it.
fn set_background(palette: &mut theme::Palette, color: egui::Color32, class: &str) {
    palette.window_background = color;
    palette.panel_background = color;
    if !SURFACES.contains(&class) {
        palette.control_background = color;
    }
}

/// Keeps the text of the element in scope readable on `background`: when the colour it inherited
/// (a bar's light text in a menu that opens over a light window) has too little contrast, its
/// text takes the ink that reads on `background`. Lasts until the scope ends. Returns that ink
/// when it changed the text, for widgets styled from the inherited colour.
pub(crate) fn readable_on(background: egui::Color32) -> Option<egui::Color32> {
    let inherited = computed().filter(|style| style.authored.color).map_or_else(theme::text_primary, |s| s.color);
    if theme::contrast(inherited, background) >= 3.0 {
        return None;
    }
    // Near black or near white, whichever reads on it: the palette's own text colours are the
    // inherited ones by now.
    let (dark, light) = (egui::Color32::from_rgb(0x1a, 0x1a, 0x1a), egui::Color32::from_rgb(0xf4, 0xf4, 0xf4));
    let ink = if theme::contrast(dark, background) >= theme::contrast(light, background) { dark } else { light };
    let mut palette = theme::palette();
    set_foreground(&mut palette, ink, "");
    theme::local_palette(palette);
    Some(ink)
}

fn set_foreground(palette: &mut theme::Palette, color: egui::Color32, class: &str) {
    palette.text_primary = color;
    palette.text_inverse = color;
    palette.text_secondary = color;
    palette.text_value = color;
    palette.text_hint = color;
    palette.browser_text_primary = color;
    palette.browser_text_secondary = color;
    palette.browser_text_value = color;
    palette.text_on_color_dark = color;
    palette.text_on_color_light = color;
    if class == "meter" {
        palette.meter_normal = color;
    }
}
pub fn foreground(fallback: egui::Color32) -> egui::Color32 {
    if let Some(style) = computed()
        && style.authored.color
    {
        return style.color;
    }
    property("color").as_deref().and_then(theme::parse_hex).unwrap_or(fallback)
}

/// Native widget presentation is portable theme data; absence preserves the user's preference.
/// The side the theme puts the Arrangement view's track headers on (`.arrangement {
/// --suhina-track-headers: left }`), when it says one: `Some(true)` for the left.
pub fn theme_track_headers_left() -> Option<bool> {
    let skin = skin::current();
    skin.styles.iter().filter(|(selector, _)| selector.as_str() == ".arrangement").find_map(|(_, properties)| {
        match properties.get("--suhina-track-headers").map(String::as_str) {
            Some("left") => Some(true),
            Some("right") => Some(false),
            _ => None,
        }
    })
}

/// The narrowest the theme lets the Arrangement view's header column be (`.arrangement {
/// --suhina-header-min-width: 330px }`): the width its placements were made for, which the
/// header's narrower layouts would break.
/// How strongly the theme lets the track's colour tint its Arrangement header (`.arrangement {
/// --suhina-header-tint: 0 }` for none, up to 100), over the user's header colour strength; a
/// see-through theme takes the native tint away with 0.
/// The least and most a theme's row height for tracks may be, in points.
const MIN_TRACK_HEIGHT: u16 = 20;
const MAX_TRACK_HEIGHT: u16 = 400;

fn px(value: &str) -> Option<u16> {
    value.trim().strip_suffix("px").and_then(|v| v.trim().parse::<u16>().ok())
}

/// The height a theme gives an Arrangement track that has none of its own (`.arrangement {
/// --suhina-track-height: 96px }`), in place of Settings › Arrangement's default size.
pub fn theme_track_height() -> Option<f32> {
    let skin = skin::current();
    let value = skin.styles.get(".arrangement")?.get("--suhina-track-height")?;
    px(value).filter(|v| (MIN_TRACK_HEIGHT..=MAX_TRACK_HEIGHT).contains(v)).map(f32::from)
}

/// How a device panel's generated parameter list is laid out (`.device { --suhina-param-columns:
/// 3; --suhina-param-row: 48px }`): parameters side by side in that many columns, each that tall.
pub fn theme_param_grid() -> (usize, Option<f32>) {
    let skin = skin::current();
    let Some(device) = skin.styles.get(".device") else { return (1, None) };
    let columns =
        device.get("--suhina-param-columns").and_then(|v| v.trim().parse::<u8>().ok()).map_or(1, |v| v.clamp(1, 8));
    let row = device.get("--suhina-param-row").and_then(|v| px(v)).filter(|v| (16..=160).contains(v)).map(f32::from);
    (usize::from(columns), row)
}

/// Where a device panel's generated parameters show their names (`.device { --suhina-param-label:
/// top }`): `left`, `top` or `hidden`, over what each device says.
pub fn theme_param_label() -> Option<&'static str> {
    let skin = skin::current();
    match skin.styles.get(".device")?.get("--suhina-param-label")?.trim() {
        "left" => Some("left"),
        "top" => Some("top"),
        "hidden" => Some("hidden"),
        _ => None,
    }
}

/// The height under which an Arrangement header keeps Suhina's own layout instead of the theme's
/// placements (`.arrangement { --suhina-compact-below: 48px }`), so a low track does not overlap.
pub fn theme_compact_below() -> Option<f32> {
    let skin = skin::current();
    px(skin.styles.get(".arrangement")?.get("--suhina-compact-below")?).filter(|v| *v <= 200).map(f32::from)
}

pub fn theme_header_tint() -> Option<u8> {
    let skin = skin::current();
    skin.styles.get(".arrangement")?.get("--suhina-header-tint")?.trim().parse::<u8>().ok().map(|v| v.min(100))
}

pub fn theme_header_min_width() -> Option<f32> {
    let skin = skin::current();
    skin.styles.get(".arrangement")?.get("--suhina-header-min-width")?.strip_suffix("px")?.trim().parse::<f32>().ok()
}

pub fn knob_preference(fallback: bool) -> bool {
    match property("--suhina-appearance").as_deref() {
        Some("knob") => true,
        Some("field") => false,
        _ => fallback,
    }
}
/// Whether an element of `class` encloses what is drawn now (`pan` around a pan's knob).
pub fn within(class: &str) -> bool {
    STACK.with(|stack| stack.borrow().iter().any(|c| c == class))
}
/// The view being drawn, for artwork of one view: `mixer` (a strip, in any view), `session` or
/// `arrangement`.
pub fn view() -> Option<&'static str> {
    STACK.with(|stack| {
        let stack = stack.borrow();
        ["mixer", "session", "arrangement"].into_iter().find(|view| stack.iter().any(|class| class == view))
    })
}

pub fn computed() -> Option<super::native_css::Computed> {
    STACK.with(|stack| super::native_css::scoped(&stack.borrow()))
}
pub fn radius(fallback: f32) -> f32 {
    if let Some(values) = STACK.with(|stack| super::native_css::scoped(&stack.borrow()))
        && values.authored.radius
    {
        return values.radius;
    }
    property("border-radius")
        .and_then(|v| v.strip_suffix("px").and_then(|n| n.parse::<u8>().ok()))
        .map_or(fallback, f32::from)
}
pub fn font_size(fallback: f32) -> f32 {
    if let Some(values) = STACK.with(|stack| super::native_css::scoped(&stack.borrow()))
        && values.authored.font_size
    {
        return values.font_size;
    }
    property("font-size")
        .and_then(|v| v.strip_suffix("px").and_then(|n| n.parse::<u8>().ok()))
        .map_or(fallback, f32::from)
}

/// Background override for surfaces whose fallback colour is supplied by their caller.
pub fn background(fallback: egui::Color32) -> egui::Color32 {
    if let Some(values) = STACK.with(|stack| super::native_css::scoped(&stack.borrow()))
        && values.authored.background
    {
        return values.background;
    }
    property("background-color").as_deref().and_then(theme::parse_hex).unwrap_or(fallback)
}

/// Applies authored values to ordinary egui widgets as well as native painters.
/// The guard reborrows the same Ui: widget ids and layout are unchanged, and early
/// returns restore the caller's style without unsafe pointers or a new child Ui.
pub struct UiStyle<'a> {
    ui: &'a mut egui::Ui,
    previous: Option<std::sync::Arc<egui::Style>>,
    previous_clip: Option<egui::Rect>,
}
impl UiStyle<'_> {
    /// Bound both native painting and interaction to CSS geometry, restoring the
    /// caller's clipping on return without creating a different widget identity.
    pub fn clipped_to(mut self, rect: egui::Rect) -> Self {
        let previous = self.ui.clip_rect();
        self.previous_clip = Some(previous);
        self.ui.set_clip_rect(previous.intersect(rect));
        self
    }
}
impl std::ops::Deref for UiStyle<'_> {
    type Target = egui::Ui;
    fn deref(&self) -> &Self::Target {
        &*self.ui
    }
}
impl std::ops::DerefMut for UiStyle<'_> {
    fn deref_mut(&mut self) -> &mut Self::Target {
        &mut *self.ui
    }
}
impl Drop for UiStyle<'_> {
    fn drop(&mut self) {
        if let Some(previous) = self.previous_clip.take() {
            self.ui.set_clip_rect(previous);
        }
        if let Some(previous) = self.previous.take() {
            self.ui.set_style(previous);
        }
    }
}
pub fn ui(ui: &mut egui::Ui) -> UiStyle<'_> {
    let values = computed();
    let color = values
        .as_ref()
        .filter(|style| style.authored.color)
        .map(|style| style.color)
        .or_else(|| property("color").as_deref().and_then(theme::parse_hex));
    let font = values
        .as_ref()
        .filter(|style| style.authored.font_size)
        .map(|style| style.font_size)
        .or_else(|| property("font-size").map(|_| font_size(theme::TEXT)));
    let radius = values
        .as_ref()
        .filter(|style| style.authored.radius)
        .map(|style| style.radius)
        .or_else(|| property("border-radius").map(|_| self::radius(0.0)));
    // A border the sheet gives (a width, 0 for none) is the ordinary widgets' outline too, in
    // every state: without it they keep egui's own.
    let border = values
        .as_ref()
        .filter(|style| style.authored.border_width)
        .map(|style| egui::Stroke::new(style.border_width, style.border));
    // A panel's or view's background is the surface behind its widgets, not their fill: a
    // device panel's colour would otherwise hide its sliders' rails.
    let container = STACK.with(|stack| {
        stack.borrow().last().is_some_and(|class| {
            matches!(
                class.as_str(),
                "device"
                    | "device-panel"
                    | "settings"
                    | "dialog"
                    | "browser"
                    | "sample-map"
                    | "start-page"
                    | "plugin-manager"
                    | "popup"
                    | "piano-roll"
                    | "detail"
                    | "clip-editor"
                    | "midi-tools"
                    | "clip-info"
                    | "arrangement"
                    | "session"
                    | "mixer"
                    | "window"
                    | "workspace"
            )
        })
    });
    let fill = values.as_ref().filter(|style| style.authored.background && !container).map(|style| style.background);
    // egui's own widgets in the scope (Settings, dialogs, the start page, the browser) take the
    // sheet's generic control rules: `.button`, `.button:hover`, `.button:active`,
    // `.button:disabled` for buttons, check boxes and drop-downs, `.button:checked` for a
    // chosen row, and `.text-input` for text fields.
    let control = |hovered: bool, pressed: bool, disabled: bool| {
        let _button = enter("button");
        super::native_css::mark_pointer(hovered, pressed, disabled);
        computed().filter(|c| c.authored.background || c.authored.border || c.authored.color || c.authored.radius)
    };
    let controls = [
        control(false, false, false),
        control(true, false, false),
        control(true, true, false),
        control(false, false, true),
    ];
    // `.button:checked`: a chosen item in a list or a menu (egui's selection).
    let chosen = {
        let _button = enter("button");
        super::native_css::mark(super::native_css::HostState::Checked, true);
        computed().filter(|c| c.authored.background || c.authored.color)
    };
    let input = {
        let _input = enter("text-input");
        computed().filter(|c| c.authored.background || c.authored.border || c.authored.color)
    };
    let has_controls = controls.iter().any(Option::is_some) || input.is_some() || chosen.is_some();
    let previous = if color.is_some()
        || font.is_some()
        || radius.is_some()
        || border.is_some()
        || fill.is_some()
        || has_controls
    {
        let previous = ui.style().clone();
        let style = ui.style_mut();
        let [idle, hovered, pressed, disabled] = &controls;
        for (widget, own) in [
            (&mut style.visuals.widgets.inactive, idle.as_ref()),
            (&mut style.visuals.widgets.hovered, hovered.as_ref()),
            (&mut style.visuals.widgets.active, pressed.as_ref()),
            (&mut style.visuals.widgets.open, pressed.as_ref()),
            (&mut style.visuals.widgets.noninteractive, disabled.as_ref()),
        ] {
            let Some(own) = own else { continue };
            if own.authored.background {
                widget.bg_fill = own.background;
                widget.weak_bg_fill = own.background;
            }
            if own.authored.border || own.authored.border_width {
                widget.bg_stroke = egui::Stroke::new(own.border_width, own.border);
            }
            if own.authored.color {
                widget.fg_stroke.color = own.color;
            }
            if own.authored.radius {
                widget.corner_radius = egui::CornerRadius::same(own.radius.clamp(0.0, 128.0) as u8);
            }
        }
        if let Some(chosen) = &chosen {
            if chosen.authored.background {
                style.visuals.selection.bg_fill = chosen.background;
            }
            if chosen.authored.color {
                style.visuals.selection.stroke.color = chosen.color;
            }
        }
        if let Some(input) = &input {
            if input.authored.background {
                style.visuals.extreme_bg_color = input.background;
                style.visuals.text_edit_bg_color = Some(input.background);
            }
            if input.authored.color {
                style.visuals.widgets.inactive.fg_stroke.color = input.color;
            }
        }
        if let Some(stroke) = border {
            for widget in [
                &mut style.visuals.widgets.noninteractive,
                &mut style.visuals.widgets.inactive,
                &mut style.visuals.widgets.hovered,
                &mut style.visuals.widgets.active,
                &mut style.visuals.widgets.open,
            ] {
                widget.bg_stroke = stroke;
            }
            style.visuals.selection.stroke.width = style.visuals.selection.stroke.width.min(stroke.width.max(1.0));
        }
        if let Some(fill) = fill {
            for widget in [&mut style.visuals.widgets.inactive, &mut style.visuals.widgets.open] {
                widget.bg_fill = fill;
                widget.weak_bg_fill = fill;
            }
        }
        if let Some(color) = color {
            style.visuals.override_text_color = Some(color);
        }
        if let Some(size) = font {
            for font in style.text_styles.values_mut() {
                font.size = size;
            }
        }
        if let Some(radius) = radius {
            let radius = egui::CornerRadius::same(radius.clamp(0.0, 128.0) as u8);
            for widget in [
                &mut style.visuals.widgets.noninteractive,
                &mut style.visuals.widgets.inactive,
                &mut style.visuals.widgets.hovered,
                &mut style.visuals.widgets.active,
                &mut style.visuals.widgets.open,
            ] {
                widget.corner_radius = radius;
            }
        }
        Some(previous)
    } else {
        None
    };
    UiStyle { ui, previous, previous_clip: None }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn native_custom_properties_inherit_and_nearest_scope_overrides() {
        let mut theme = theme::Theme::default();
        assert!(
            super::super::theme_css::apply(
                &mut theme,
                ".track-column { --suhina-track-artwork: on; } .track-column .clip { --suhina-track-artwork: off; }"
            )
            .is_empty()
        );
        skin::configure(&theme.skin);
        {
            let _column = enter("track-column");
            assert_eq!(property("--suhina-track-artwork").as_deref(), Some("on"));
            {
                let _slot = enter("slot");
                assert_eq!(property("--suhina-track-artwork").as_deref(), Some("on"));
                let _clip = enter("clip");
                assert_eq!(property("--suhina-track-artwork").as_deref(), Some("off"));
            }
            assert_eq!(property("--suhina-track-artwork").as_deref(), Some("on"));
        }
        assert!(property("--suhina-track-artwork").is_none());
        skin::configure(&skin::Skin::default());
    }

    #[test]
    fn portable_pan_appearance_survives_json_css_and_does_not_leak_out_of_its_scope() {
        for (value, forced) in [("knob", Some(true)), ("field", Some(false)), ("native", None)] {
            let mut source = theme::Theme::default();
            assert!(
                super::super::theme_css::apply(
                    &mut source,
                    &format!(".mixer .pan {{ --suhina-appearance: {value}; }}")
                )
                .is_empty()
            );
            let (restored, warnings) = theme::Theme::from_json(&source.to_json());
            assert!(warnings.is_empty(), "{warnings:?}");
            let mut css = theme::Theme::default();
            assert!(super::super::theme_css::apply(&mut css, &super::super::theme_css::export(&restored)).is_empty());
            assert_eq!(source, css);
            skin::configure(&restored.skin);
            let _mixer = enter("mixer");
            {
                let _pan = enter("pan");
                for preference in [false, true] {
                    assert_eq!(knob_preference(preference), forced.unwrap_or(preference));
                }
            }
            assert!(!knob_preference(false));
            assert!(knob_preference(true));
        }
        skin::configure(&skin::Skin::default());
    }
    #[test]
    fn a_live_theme_change_survives_all_enclosing_style_scopes() {
        let original = theme::palette();
        let mut skin = skin::Skin::default();
        skin.styles.insert(".browser".into(), [("color".into(), "#123456".into())].into());
        skin::configure(&skin);
        let mut changed = original;
        changed.text_primary = egui::Color32::from_rgb(40, 50, 60);
        changed.grid_major = egui::Color32::from_rgba_unmultiplied(80, 90, 100, 120);
        let ctx = egui::Context::default();
        {
            let _window = enter("window");
            let _browser = enter("browser");
            assert_eq!(theme::text_primary(), theme::parse_hex("#123456").unwrap());
            theme::set_palette(&ctx, changed);
        }
        assert_eq!(theme::palette(), changed);
        theme::set_palette(&ctx, original);
        skin::configure(&skin::Skin::default());
    }
    #[test]
    fn ordinary_widget_style_preserves_ui_identity_and_restores_caller() {
        let mut skin = skin::Skin::default();
        skin.styles.insert(
            ".settings".into(),
            [("color".into(), "#123456".into()), ("font-size".into(), "19px".into())].into(),
        );
        skin::configure(&skin);
        let context = egui::Context::default();
        context
            .run_ui(egui::RawInput::default(), |ui| {
                let original = ui.style().clone();
                let original_clip = ui.clip_rect();
                let id = ui.id();
                {
                    let _scope = enter("settings");
                    let limited = egui::Rect::from_min_size(ui.max_rect().min, egui::Vec2::splat(16.0));
                    let styled = self::ui(ui).clipped_to(limited);
                    assert_eq!(styled.id(), id);
                    assert_eq!(styled.clip_rect(), original_clip.intersect(limited));
                    assert_eq!(styled.visuals().override_text_color, theme::parse_hex("#123456"));
                    assert!(styled.style().text_styles.values().all(|font| font.size == 19.0));
                }
                assert!(std::sync::Arc::ptr_eq(ui.style(), &original));
                assert_eq!(ui.clip_rect(), original_clip);
            })
            .textures_delta
            .clear();
        skin::configure(&skin::Skin::default());
    }
    #[test]
    fn descendant_styles_are_scoped_and_restore_palette() {
        let mut s = skin::Skin::default();
        s.styles.insert(".browser".into(), [("color".into(), "#123456".into())].into());
        s.styles.insert(".browser .button".into(), [("color".into(), "#ABCDEF".into())].into());
        skin::configure(&s);
        let original = theme::palette();
        {
            let _browser = enter("browser");
            assert_eq!(theme::text_primary(), theme::parse_hex("#123456").unwrap());
            {
                let _button = enter("button");
                assert_eq!(theme::text_primary(), theme::parse_hex("#ABCDEF").unwrap());
            }
            assert_eq!(theme::text_primary(), theme::parse_hex("#123456").unwrap());
        }
        assert_eq!(theme::palette(), original);
        skin::configure(&skin::Skin::default());
    }
}
