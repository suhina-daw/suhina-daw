// Copyright (C) 2026 The Suhina contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! Native computed styles, Taffy layout and motion, consumed by native egui controls.
//!
//! The host supplies a bounded tree; themes never supply markup or executable code.
//! Everything resolves through `suhina-style`; there is no browser engine. A sheet that does
//! not parse keeps the last working one. This module is UI-thread-only.
use std::cell::RefCell;
use std::collections::HashMap;
use std::hash::{Hash, Hasher};
use std::sync::Arc;

use egui::{Color32, Rect, Vec2};
use suhina_style::{
    motion::{self, Animation, Frames, Transition},
    property::Property,
    value::Value,
};

use super::{skin, theme};
mod diagnostics;
mod geometry;
mod native;
mod paint;
mod shadows;
mod text;
mod workspace;
pub use diagnostics::{Notice, inspect};
pub use paint::{
    artwork_shadows, background, background_rounded, background_with, gradients_rounded, level_fill, level_glow,
    surface, thickness,
};
pub use text::paint_text;
pub use workspace::{Workspace, workspace};

const MAX_CONTEXTS: usize = 128;
const MAX_DEPTH: usize = 16;
const MAX_NODES: usize = 4096;

/// Styles exposed by native painters. Only properties a painter draws are here.
#[derive(Clone, Debug)]
pub struct Computed {
    pub color: Color32,
    pub background: Color32,
    pub border: Color32,
    pub font_size: f32,
    pub radius: f32,
    /// `border-width`; a control draws a border only when the sheet sets one.
    pub border_width: f32,
    /// `height` in pixels, when the sheet sets one: the thickness of a bar such as a volume field's.
    pub height: Option<f32>,
    /// `width` in pixels, when the sheet sets one: the size of a fader's cap or rail.
    pub width: Option<f32>,
    /// `text-align` where the sheet sets one: -1 left, 0 centre, 1 right.
    pub text_align: Option<i8>,
    pub opacity: f32,
    pub visible: bool,
    /// `accent-color`: a control's accent, such as a knob's arc.
    pub accent: Color32,
    pub authored: Authored,
    gradients: Arc<[paint::Gradient]>,
    shadows: Arc<[shadows::Shadow]>,
    text_shadows: Arc<[text::Shadow]>,
    transitions: Arc<[Transition]>,
    animations: Arc<[(Animation, Arc<Frames>)]>,
}
impl Computed {
    pub fn has_gradient(&self) -> bool {
        !self.gradients.is_empty()
    }

    /// The value a transition or keyframe animation interpolates for `property`.
    fn animated(&self, property: Property) -> Option<Value> {
        use suhina_style::value::{Color, Length};
        let color = |c: Color32| Value::Color(Color::Rgba(c.to_srgba_unmultiplied()));
        Some(match property {
            Property::Color => color(self.color),
            Property::BackgroundColor => color(self.background),
            Property::BorderColor => color(self.border),
            Property::Opacity => Value::Number(self.opacity),
            Property::FontSize => Value::Length(Length::Px(self.font_size)),
            Property::Radius => Value::Length(Length::Px(self.radius)),
            _ => return None,
        })
    }

    fn set_animated(&mut self, property: Property, value: &Value) {
        use suhina_style::value::{Color, Length};
        let color = |v: &Value| match v {
            Value::Color(Color::Rgba([r, g, b, a])) => Some(Color32::from_rgba_unmultiplied(*r, *g, *b, *a)),
            _ => None,
        };
        let px = |v: &Value| match v {
            Value::Length(Length::Px(n)) if n.is_finite() => Some(*n),
            _ => None,
        };
        match property {
            Property::Color => {
                if let Some(c) = color(value) {
                    self.color = c;
                    self.authored.color = true;
                }
            }
            Property::BackgroundColor => {
                if let Some(c) = color(value) {
                    self.background = c;
                    self.authored.background = true;
                }
            }
            Property::BorderColor => {
                if let Some(c) = color(value) {
                    self.border = c;
                    self.authored.border = true;
                }
            }
            Property::Opacity => {
                if let Value::Number(n) = value
                    && n.is_finite()
                {
                    self.opacity = n.clamp(0.0, 1.0);
                }
            }
            Property::FontSize => {
                if let Some(n) = px(value) {
                    self.font_size = n.clamp(7.0, 96.0);
                    self.authored.font_size = true;
                }
            }
            Property::Radius => {
                if let Some(n) = px(value) {
                    self.radius = n.clamp(0.0, 128.0);
                    self.authored.radius = true;
                }
            }
            Property::TextShadow | Property::BoxShadow => {
                if let Value::Shadows(list) = value {
                    let foreground = self.color.to_srgba_unmultiplied();
                    let resolved: Vec<_> = list
                        .iter()
                        .take(suhina_style::paint::DRAWN_SHADOW_LAYERS)
                        .map(|shadow| shadow.resolve(self.font_size, foreground))
                        .collect();
                    if property == Property::TextShadow {
                        self.text_shadows = resolved.iter().map(text::Shadow::from_native).collect::<Vec<_>>().into();
                    } else {
                        self.shadows = resolved.iter().map(shadows::Shadow::from_native).collect::<Vec<_>>().into();
                    }
                }
            }
            _ => {}
        }
    }
}

/// Only authored properties override native defaults. An unrelated CSS rule must not
/// replace every label's native colour, size or corner radius with defaults.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Authored {
    pub color: bool,
    pub background: bool,
    pub border: bool,
    pub font_size: bool,
    pub radius: bool,
    pub accent: bool,
    pub border_width: bool,
}

/// One running transition of one property of one element.
#[derive(Clone, Debug)]
struct Running {
    property: Property,
    from: Value,
    to: Value,
    start: f64,
    transition: Transition,
}

/// Motion state of one element, kept while its key exists.
#[derive(Clone, Debug, Default)]
struct Motion {
    /// When its current animation list started, and the names that identify the list.
    started: f64,
    names: Vec<String>,
    running: Vec<Running>,
}

#[derive(Default)]
struct Engine {
    source: Option<Arc<str>>,
    palette: String,
    size: [u32; 2],
    dark: bool,
    contexts: HashMap<Vec<String>, Computed>,
    toolbar: Option<(String, Vec<Module>)>,
    workspace: Option<(String, Option<Workspace>)>,
    toolbar_layout: bool,
    collecting: bool,
    nodes: Vec<StyleNode>,
    parents: Vec<usize>,
    occurrences: HashMap<(u64, String), usize>,
    previous: Vec<StyleNode>,
    /// Resolved styles before motion, by element key.
    targets: HashMap<u64, Computed>,
    /// What painters read: the targets with transitions and animations applied.
    values: HashMap<u64, Computed>,
    motions: HashMap<u64, Motion>,
    native: Option<native::Paint>,
    motion: bool,
    styles_changed: bool,
    /// The user's own sheet (`user.css`), above every theme and device sheet.
    user: Option<(Arc<str>, Arc<suhina_style::sheet::Sheet>)>,
    /// The chosen theme variant's values, above the theme's sheet.
    variant: Option<(Arc<str>, Arc<suhina_style::sheet::Sheet>)>,
    /// Parsed device panel sheets by the hash of their text, bounded.
    device_sheets: HashMap<u64, Arc<suhina_style::sheet::Sheet>>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
struct StyleNode {
    key: u64,
    parent: Option<usize>,
    class: String,
    name: Option<String>,
    /// Pointer states (hover 1, active 2, disabled 4, focus 8) and the host's ([`mark`]).
    state: u16,
    /// Custom properties the host gives the element ([`set_variable`]).
    variables: Vec<(String, String)>,
    /// The device sheet this node is the root of (its hash), or 0.
    sheet: u64,
}
thread_local! {
    static ENGINE: RefCell<Engine> = RefCell::new(Engine::default());
    static USER_SHEET: RefCell<Option<Arc<str>>> = const { RefCell::new(None) };
    /// Whether a device panel asked for its sheet since the last frame began, so the engine
    /// runs even when the theme has no style sheet of its own.
    static DEVICE_WANTED: std::cell::Cell<bool> = const { std::cell::Cell::new(false) };
}

/// Most device panel sheets kept parsed at once.
const MAX_DEVICE_SHEETS: usize = 32;

/// Sets the user's own style sheet (`user.css`), which applies over every theme and device
/// sheet and stays when the theme changes. `None` removes it. Read at the next frame.
pub fn set_user_sheet(source: Option<Arc<str>>) {
    USER_SHEET.with(|sheet| *sheet.borrow_mut() = source);
}

/// Gives the device panel scope just entered its own style sheet: it applies only inside that
/// panel, above the application theme and below `user.css`. Unchanged text is parsed once; a
/// sheet that does not parse is ignored and the panel keeps the theme.
pub fn device_sheet(ctx: &egui::Context, source: Option<&str>) {
    let Some(source) = source.filter(|s| !s.trim().is_empty() && s.len() <= 64 * 1024) else { return };
    if !DEVICE_WANTED.with(|wanted| wanted.replace(true)) && !ENGINE.with(|engine| engine.borrow().collecting) {
        // The engine was idle: start it on the next frame.
        ctx.request_repaint();
    }
    let mut hash = std::hash::DefaultHasher::new();
    source.hash(&mut hash);
    let key = hash.finish().max(1);
    ENGINE.with(|engine| {
        let mut engine = engine.borrow_mut();
        if !engine.collecting {
            return;
        }
        if !engine.device_sheets.contains_key(&key) {
            let Ok(sheet) = suhina_style::sheet::Sheet::parse(source) else { return };
            if engine.device_sheets.len() >= MAX_DEVICE_SHEETS {
                engine.device_sheets.clear();
            }
            engine.device_sheets.insert(key, Arc::new(sheet));
        }
        if let Some(node) = engine.parents.last().copied().and_then(|at| engine.nodes.get_mut(at)) {
            node.sheet = key;
        }
    });
}

/// Finishes the style snapshot even on an early return from the native frame.
pub struct Frame(egui::Context, f64);
impl Drop for Frame {
    fn drop(&mut self) {
        let (changed, animating) = suhina_plugins::trace::timed("native CSS tree", || finish_tree(self.1));
        if animating {
            self.0.request_repaint_after(std::time::Duration::from_millis(16));
        } else if changed {
            self.0.request_repaint();
        }
    }
}

pub fn frame(ui: &egui::Ui) -> Frame {
    frame_with_motion(ui, true)
}

/// Disabling motion suppresses CSS transitions/keyframes as well as extra ticks.
pub fn frame_with_motion(ui: &egui::Ui, motion: bool) -> Frame {
    begin_frame(ui);
    ENGINE.with(|engine| {
        let mut engine = engine.borrow_mut();
        if engine.motion != motion {
            engine.motion = motion;
            engine.motions.clear();
            engine.styles_changed = true;
        }
        engine.collecting = engine.source.is_some();
        engine.nodes.clear();
        engine.parents.clear();
        engine.occurrences.clear();
    });
    Frame(ui.ctx().clone(), ui.input(|input| input.time))
}

/// Register an actual native scope. Named project roles retain identity when reordered.
pub fn enter(class: &str, name: Option<&str>, response: Option<&egui::Response>) -> bool {
    ENGINE.with(|engine| {
        let mut engine = engine.borrow_mut();
        if !engine.collecting || engine.nodes.len() >= MAX_NODES || engine.parents.len() >= MAX_DEPTH {
            return false;
        }
        if !super::style_class::CLASSES.contains(&class) {
            return false;
        }
        let name = name.filter(|name| {
            name.len() <= 96 && name.bytes().all(|b| b.is_ascii_alphanumeric() || matches!(b, b'-' | b'_' | b'.'))
        });
        let parent = engine.parents.last().copied();
        let parent_key = parent.and_then(|p| engine.nodes.get(p)).map_or(0, |n| n.key);
        let occurrence = engine.occurrences.entry((parent_key, class.into())).or_default();
        let index = *occurrence;
        *occurrence += 1;
        let mut hash = std::hash::DefaultHasher::new();
        (parent_key, class, name).hash(&mut hash);
        if let Some(response) = response {
            response.id.hash(&mut hash);
        } else if name.is_none() {
            index.hash(&mut hash);
        }
        let key = hash.finish();
        let state = response.map_or(0, |r| {
            u16::from(r.hovered())
                | u16::from(r.is_pointer_button_down_on()) << 1
                | u16::from(!r.enabled()) << 2
                | u16::from(r.has_focus()) << 3
        });
        let node = StyleNode {
            key,
            parent,
            class: class.into(),
            name: name.map(str::to_owned),
            state,
            variables: Vec::new(),
            sheet: 0,
        };
        let index = engine.nodes.len();
        engine.nodes.push(node);
        engine.parents.push(index);
        true
    })
}

/// A state of a control or a track that only the host knows, for selectors such as `:checked`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum HostState {
    /// A toggle that is on: a switched-on button, a soloed or armed track's button.
    Checked = 16,
    /// The selected track (its header, column or mixer strip).
    Selected = 32,
    /// Playing: the play button while the transport runs, a playing clip.
    Playing = 64,
    /// Recording: the record button while it records.
    Recording = 128,
    /// A track armed for recording.
    Armed = 256,
    /// A muted track.
    Muted = 512,
}

/// The host states and the selector names they match.
pub(crate) const HOST_STATES: [(u16, &str); 6] =
    [(16, "checked"), (32, "selected"), (64, "playing"), (128, "recording"), (256, "armed"), (512, "muted")];

/// The name of the element now in scope (`play` for `#control-play`), when it has one.
pub fn current_name() -> Option<String> {
    ENGINE.with(|engine| {
        let engine = engine.borrow();
        engine.parents.last().and_then(|&at| engine.nodes.get(at)).and_then(|node| node.name.clone())
    })
}

/// Gives the element now in scope the custom property `name` (`--track-colour`) as `value`, which
/// it and its descendants see below every rule of the sheet.
pub fn set_variable(name: &str, value: String) {
    ENGINE.with(|engine| {
        let mut engine = engine.borrow_mut();
        if let Some(&at) = engine.parents.last()
            && let Some(node) = engine.nodes.get_mut(at)
            && node.variables.len() < 16
        {
            node.variables.push((name.to_owned(), value));
        }
    });
}

/// The variables Suhina sets for the elements in scope (`--track-colour`, `--clip-colour`, …),
/// the innermost last, for artwork's `var()`.
pub fn scope_variables() -> Vec<(String, String)> {
    ENGINE.with(|engine| {
        let engine = engine.borrow();
        engine
            .parents
            .iter()
            .filter_map(|&at| engine.nodes.get(at))
            .flat_map(|node| node.variables.iter().cloned())
            .collect()
    })
}

/// `color` as `#RRGGBB` or `#RRGGBBAA`, for a value given to the sheet.
pub fn hex(color: Color32) -> String {
    let [r, g, b, a] = color.to_srgba_unmultiplied();
    if a == 255 { format!("#{r:02x}{g:02x}{b:02x}") } else { format!("#{r:02x}{g:02x}{b:02x}{a:02x}") }
}

/// Marks the element now in scope with `state` when `on`, so the sheet's `:checked`,
/// `:selected` and the others match it from the next frame.
pub fn mark(state: HostState, on: bool) {
    if !on {
        return;
    }
    ENGINE.with(|engine| {
        let mut engine = engine.borrow_mut();
        if let Some(&at) = engine.parents.last()
            && let Some(node) = engine.nodes.get_mut(at)
        {
            node.state |= state as u16;
        }
    });
}

pub fn leave() {
    ENGINE.with(|engine| {
        engine.borrow_mut().parents.pop();
    });
}

/// Marks the element in scope as the pointer leaves it: `hovered` (`:hover`), `pressed`
/// (`:active`) or `disabled` (`:disabled`), for a style looked up before any widget exists.
pub fn mark_pointer(hovered: bool, pressed: bool, disabled: bool) {
    let bits = u16::from(hovered) | u16::from(pressed) << 1 | u16::from(disabled) << 2;
    if bits == 0 {
        return;
    }
    ENGINE.with(|engine| {
        let mut engine = engine.borrow_mut();
        if let Some(&at) = engine.parents.last()
            && let Some(node) = engine.nodes.get_mut(at)
        {
            node.state |= bits;
        }
    });
}

/// Buttons a theme selects by id, as `#control-NAME`: the toolbar's and a track's.
pub const CONTROL_IDS: &[&str] = &[
    "play",
    "stop",
    "record",
    "browser-toggle",
    "metronome-toggle",
    "solo",
    "cue",
    "arm",
    "track-on",
    "automation",
    "monitor",
];

/// Level fields a theme selects by id (`#arrangement-volume`).
pub const LEVEL_FIELDS: &[&str] = &["arrangement-volume", "arrangement-pan", "mixer-volume", "mixer-pan"];

/// The element id a theme selects a scope by: documented controls and panels keep their
/// literal ids; others get one derived from their key.
fn element_id(node: &StyleNode) -> String {
    if node.class == "button"
        && let Some(name) = node.name.as_deref().filter(|name| CONTROL_IDS.contains(name))
    {
        return format!("control-{name}");
    }
    if (node.class == "workspace" && node.name.as_deref() == Some("workspace"))
        || (node.class == "workspace-panel"
            && node.name.as_deref().is_some_and(|name| ["panel-browser", "panel-main", "panel-detail"].contains(&name)))
    {
        return node.name.clone().unwrap_or_default();
    }
    // A device panel's control is selected by its id (`#cutoff`), as the panel sheet names it.
    if node.class == "device-control"
        && let Some(name) = &node.name
    {
        return name.clone();
    }
    if node.class == "divider" && node.name.as_deref() == Some("mixer-divider") {
        return "mixer-divider".to_owned();
    }
    // A device's panel by what it is: `#device-compressor`, `#device-plugin`.
    if node.class == "device"
        && let Some(name) = node.name.as_deref().filter(|name| name.starts_with("device-"))
    {
        return name.to_owned();
    }
    if node.class == "editor-tab"
        && let Some(name) = node.name.as_deref().filter(|name| name.starts_with("tab-"))
    {
        return name.to_owned();
    }
    if node.class == "piano-tool"
        && let Some(name) = node.name.as_deref().filter(|name| name.starts_with("piano-"))
    {
        return name.to_owned();
    }
    if node.class == "add-track"
        && let Some(name) = node.name.as_deref().filter(|name| name.starts_with("add-"))
    {
        return name.to_owned();
    }
    // A mixer send by its place: `#mixer-send-1` is the first.
    if node.class == "send"
        && let Some(name) = node.name.as_deref().filter(|name| name.starts_with("mixer-send-"))
    {
        return name.to_owned();
    }
    if node.class == "popup"
        && let Some(name) = node.name.as_deref().filter(|name| *name == "metronome-level")
    {
        return name.to_owned();
    }
    // The level fields of the track headers and the mixer, by the names Edit interface gives them.
    if matches!(node.class.as_str(), "value" | "pan" | "fader")
        && let Some(name) = node.name.as_deref().filter(|name| LEVEL_FIELDS.contains(name))
    {
        return name.to_owned();
    }
    if node.class == "toolbar-module" && node.name.as_deref().is_some_and(|name| skin::MODULES.contains(&name)) {
        node.name.clone().unwrap_or_default()
    } else {
        format!("node-{}", node.key)
    }
}

/// Applies running transitions and keyframe animations to `target` at time `now`; returns
/// the painted values and whether anything is still moving.
fn animate(motion: &mut Motion, target: &Computed, now: f64) -> (Computed, bool) {
    let mut value = target.clone();
    let mut moving = false;
    if !target.animations.is_empty() {
        let names: Vec<String> = target.animations.iter().map(|(a, _)| a.name.clone()).collect();
        if motion.names != names {
            motion.names = names;
            motion.started = now;
        }
        let elapsed = (now - motion.started) as f32;
        for (animation, frames) in target.animations.iter() {
            moving |= motion::running(animation, elapsed);
            let Some(progress) = motion::progress(animation, elapsed) else { continue };
            let base = |property: Property| target.animated(property).unwrap_or(Value::Keyword(""));
            for (property, sampled) in motion::sample(frames, progress, animation.easing, base) {
                value.set_animated(property, &sampled);
            }
        }
    } else {
        motion.names.clear();
    }
    motion.running.retain(|running| {
        let duration = f64::from(running.transition.duration.max(f32::EPSILON));
        let t = ((now - running.start) / duration) as f32;
        if t >= 1.0 {
            return false;
        }
        let eased = running.transition.easing.apply(t.max(0.0));
        let sampled = motion::sample(
            &vec![
                (0.0, vec![(running.property, running.from.clone())]),
                (1.0, vec![(running.property, running.to.clone())]),
            ],
            eased,
            motion::Easing::Linear,
            |_| running.to.clone(),
        );
        for (property, sampled) in sampled {
            value.set_animated(property, &sampled);
        }
        moving = true;
        true
    });
    (value, moving)
}

/// Starts transitions for the animatable properties that changed from `old` to `new`, from the
/// value currently painted.
fn start_transitions(motion: &mut Motion, old: &Computed, new: &Computed, painted: &Computed, now: f64) {
    for property in [
        Property::Color,
        Property::BackgroundColor,
        Property::BorderColor,
        Property::Opacity,
        Property::FontSize,
        Property::Radius,
    ] {
        let (Some(before), Some(after)) = (old.animated(property), new.animated(property)) else { continue };
        if before == after {
            continue;
        }
        let Some(transition) = motion::find(&new.transitions, property) else { continue };
        let from = painted.animated(property).unwrap_or(before);
        motion.running.retain(|running| running.property != property);
        motion.running.push(Running {
            property,
            from,
            to: after,
            start: now + f64::from(transition.delay),
            transition,
        });
    }
}

fn finish_tree(now: f64) -> (bool, bool) {
    ENGINE.with(|engine| {
        let mut engine = engine.borrow_mut();
        engine.collecting = false;
        let changed = engine.nodes != engine.previous;
        let moving_before = !engine.motions.values().all(|m| m.running.is_empty() && m.names.is_empty());
        if engine.source.is_none()
            || engine.native.is_none()
            || (!changed && !engine.styles_changed && !moving_before && !engine.targets.is_empty())
        {
            return (false, false);
        }
        if changed || engine.styles_changed || engine.targets.is_empty() {
            let Some(resolved) = suhina_plugins::trace::timed("style paint resolve", || native::retained(&engine))
            else {
                // A tree the engine cannot resolve keeps the last painted values.
                engine.previous = engine.nodes.clone();
                engine.styles_changed = false;
                return (false, false);
            };
            let targets: HashMap<u64, Computed> = resolved.into_iter().collect();
            if engine.motion {
                let Engine { motions, values, targets: old, .. } = &mut *engine;
                for (key, new) in &targets {
                    if let (Some(old), Some(painted)) = (old.get(key), values.get(key)) {
                        start_transitions(motions.entry(*key).or_default(), old, new, painted, now);
                    }
                }
            }
            engine.motions.retain(|key, _| targets.contains_key(key));
            engine.targets = targets;
            engine.previous = engine.nodes.clone();
        }
        let styles_changed = std::mem::take(&mut engine.styles_changed);
        let mut moving = false;
        let mut values = HashMap::with_capacity(engine.targets.len());
        let motion_enabled = engine.motion;
        let Engine { targets, motions, .. } = &mut *engine;
        for (key, target) in targets.iter() {
            if motion_enabled
                && (!target.animations.is_empty() || motions.get(key).is_some_and(|m| !m.running.is_empty()))
            {
                let (value, still) = animate(motions.entry(*key).or_default(), target, now);
                moving |= still;
                values.insert(*key, value);
            } else {
                values.insert(*key, target.clone());
            }
        }
        if !moving {
            engine.motions.retain(|_, m| !m.running.is_empty() || !m.names.is_empty());
        }
        engine.values = values;
        // Publish the final sample too: once motion stops, the preceding frame still
        // painted its penultimate values.
        (changed || styles_changed || moving || moving_before, moving)
    })
}

/// Refresh only when the authored CSS, palette or viewport changes.
pub fn begin_frame(ui: &egui::Ui) {
    let user_present = USER_SHEET.with(|sheet| sheet.borrow().is_some());
    let device_wanted = DEVICE_WANTED.with(|wanted| wanted.replace(false));
    // Without a theme sheet the engine still runs for user.css and device panel sheets.
    let source = skin::current().css.clone().or_else(|| (user_present || device_wanted).then(|| Arc::from("")));
    if source.is_none() {
        ENGINE.with(|engine| {
            if engine.borrow().source.is_some() {
                *engine.borrow_mut() = Engine::default();
            }
        });
        return;
    }
    let size = ui.ctx().input(|i| i.content_rect().size());
    let size = [size.x.clamp(1.0, 16384.0) as u32, size.y.clamp(1.0, 16384.0) as u32];
    let palette = theme::palette();
    let mut variables = String::from(":root {");
    for (key, _, color) in palette.tokens() {
        let variable = key.replace('.', "-");
        // Only referenced palette variables affect this stylesheet. Resetting all
        // style/layout caches for an unrelated opacity tick rebuilt every control's
        // style during every drag frame.
        // Escaped identifiers are conservatively treated as depending on all tokens.
        if key != "text.primary"
            && !source.as_deref().is_some_and(|css| css.contains('\\') || css.contains(&format!("--{variable}")))
        {
            continue;
        }
        variables.push_str(&format!("--{variable}:{};", theme::hex(color)));
    }
    variables.push_str("}\n");
    let dark = theme::contrast(palette.window_background, Color32::WHITE)
        > theme::contrast(palette.window_background, Color32::BLACK);
    let user = USER_SHEET.with(|sheet| sheet.borrow().clone());
    let variant = skin::current().variant.clone();
    ENGINE.with(|engine| {
        let mut engine = engine.borrow_mut();
        if engine.variant.as_ref().map(|(text, _)| text) != variant.as_ref() {
            engine.variant = variant
                .and_then(|text| Some((Arc::clone(&text), Arc::new(suhina_style::sheet::Sheet::parse(&text).ok()?))));
            engine.styles_changed = true;
            engine.contexts.clear();
            engine.toolbar = None;
            engine.workspace = None;
        }
        if engine.user.as_ref().map(|(text, _)| text) != user.as_ref() {
            // A user sheet that does not parse keeps the last working one.
            match user {
                Some(text) => {
                    if let Ok(sheet) = suhina_style::sheet::Sheet::parse(&text) {
                        engine.user = Some((text, Arc::new(sheet)));
                    }
                }
                None => engine.user = None,
            }
            engine.styles_changed = true;
            engine.contexts.clear();
            engine.toolbar = None;
            engine.workspace = None;
        }
        if engine.source != source {
            let native = suhina_plugins::trace::timed("style paint parse", || {
                source.as_deref().and_then(|source| native::Paint::new(source, &variables))
            });
            match native {
                Some(native) => {
                    let motion = engine.motion;
                    let user = engine.user.take();
                    let variant = engine.variant.take();
                    let device_sheets = std::mem::take(&mut engine.device_sheets);
                    *engine = Engine {
                        source,
                        palette: variables,
                        size,
                        dark,
                        native: Some(native),
                        motion,
                        user,
                        variant,
                        device_sheets,
                        ..Default::default()
                    };
                }
                None => {
                    // A sheet that does not parse (a half-saved file) keeps the last
                    // working styles; its error is shown by the theme editor.
                    engine.source = source;
                }
            }
        } else if engine.size != size || engine.dark != dark || engine.palette != variables {
            // Palette drags and native resizing do not change the scope tree or
            // authored CSS. Keep the last computed snapshot and resolve the new inputs
            // once at the end of this pass.
            engine.palette = variables;
            let palette = engine.palette.clone();
            if let Some(paint) = engine.native.as_mut() {
                let _ = paint.palette(&palette);
            }
            engine.size = size;
            engine.dark = dark;
            engine.styles_changed = true;
            engine.contexts.clear();
            engine.toolbar = None;
            engine.workspace = None;
        }
    });
}

/// Whether the engine's sheet is the active theme's (an empty one stands in for none).
fn same_source(engine: Option<&Arc<str>>) -> bool {
    let current = skin::current().css.clone();
    match (engine, current.as_ref()) {
        (Some(a), Some(b)) => a == b,
        (Some(a), None) => a.is_empty(),
        _ => false,
    }
}

/// Compute inherited colours and native typography for the active class ancestry.
/// Sibling/object selectors require the host's complete retained component tree.
pub fn scoped(stack: &[String]) -> Option<Computed> {
    if stack.is_empty()
        || stack.len() > MAX_DEPTH
        || stack.iter().any(|s| !super::style_class::CLASSES.contains(&s.as_str()))
    {
        return None;
    }
    ENGINE.with(|engine| {
        let mut engine = engine.borrow_mut();
        engine.source.as_ref()?;
        if !same_source(engine.source.as_ref()) {
            return None;
        }
        if let Some(values) =
            engine.parents.last().and_then(|at| engine.nodes.get(*at)).and_then(|node| engine.values.get(&node.key))
        {
            return Some(values.clone());
        }
        if let Some(values) = engine.contexts.get(stack) {
            return Some(values.clone());
        }
        if engine.contexts.len() >= MAX_CONTEXTS {
            return None;
        }
        let values = native::scoped(&engine, stack)?;
        engine.contexts.insert(stack.to_vec(), values.clone());
        Some(values)
    })
}

/// One real toolbar child. Drawing and input use this same Taffy rectangle.
#[derive(Clone, Debug)]
pub struct Module {
    pub name: String,
    pub rect: Rect,
    pub style: Computed,
}

pub fn toolbar_layout_requested() -> bool {
    ENGINE.with(|engine| engine.borrow().toolbar_layout)
}

/// Flex/Grid layout for actual ordered, visible module ids. No theme-supplied markup.
pub fn toolbar(order: &[(String, f32)], size: Vec2) -> Option<Vec<Module>> {
    if order.len() > skin::MODULES.len()
        || order.iter().any(|(id, _)| !skin::MODULES.contains(&id.as_str()))
        || !size.is_finite()
    {
        return None;
    }
    ENGINE.with(|engine| {
        let mut engine = engine.borrow_mut();
        engine.source.as_ref()?;
        if !same_source(engine.source.as_ref()) {
            return None;
        }
        let key = format!("{order:?}:{size:?}");
        if let Some((previous, modules)) = &engine.toolbar
            && previous == &key
        {
            return Some(modules.clone());
        }
        let mut defaults = format!(
            ".toolbar {{ display:flex; width:{}px; height:{}px; gap:4px; align-items:center; }} .toolbar-module {{ flex-shrink:0; height:{}px; }}",
            size.x.clamp(1.0, 16384.0),
            size.y.clamp(1.0, 16384.0),
            size.y.clamp(24.0, 16384.0)
        );
        for (id, width) in order {
            defaults.push_str(&format!("#{id} {{ width:{}px; }}", width.clamp(1.0, 4096.0)));
        }
        let mut elements = vec![native::element("toolbar", Some("toolbar"), None, Some(1))];
        for (id, _) in order {
            let mut module = native::element("toolbar-module", Some(id), None, Some(2));
            module.classes.push(id.clone());
            elements.push(module);
        }
        let laid = native::layout(&engine, elements, &defaults, [size.x, size.y])?;
        engine.toolbar_layout = laid.requested;
        let mut modules = Vec::new();
        for ((name, _), (rect, style)) in order.iter().zip(laid.items.into_iter().skip(1)) {
            let Some(rect) = rect else { continue };
            if style.visible {
                modules.push(Module { name: name.clone(), rect, style });
            }
        }
        engine.toolbar = Some((key, modules.clone()));
        Some(modules)
    })
}

#[cfg(test)]
#[path = "native_css_tests.rs"]
mod tests;
