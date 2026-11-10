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
    sheet::Sheet,
    value::Value,
};

use super::{skin, theme};
mod diagnostics;
mod geometry;
mod native;

pub(super) use native::user_agent;
pub use native::user_agent_color;
mod paint;
mod shadows;
mod text;
mod workspace;
pub use diagnostics::{Notice, inspect};
pub use paint::background_with;
#[cfg(test)]
pub(in crate::kit) use paint::surface_with;
pub(in crate::kit) use paint::{artwork_shadows, background_rounded, gradients_rounded, level_fill, level_glow};
pub(in crate::kit) use paint::{box_begin, box_end, has_end};
pub use text::paint_text;
pub use workspace::{Workspace, splitter_between, workspace};

const MAX_CONTEXTS: usize = 128;
const MAX_DEPTH: usize = 16;
const MAX_NODES: usize = 4096;
/// Most element identities whose styles are kept, and most ids the app gives elements (`track-3`,
/// `device-compressor`) kept interned. Past the identities' limit their cache starts again empty;
/// a full id table starts again at the next frame, and until then a new id is not given.
const MAX_IDENTITIES: usize = 4096;
const MAX_IDS: usize = 8192;

/// A computed style: as the style engine gives it, and as painters read it.
#[derive(Clone)]
struct Styled {
    raw: Arc<suhina_style::cascade::ComputedStyle>,
    computed: Computed,
}

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
    /// Every custom property the element sees, resolved (`--device-series-1`).
    variables: Arc<std::collections::BTreeMap<String, String>>,
    gradients: Arc<[paint::Gradient]>,
    shadows: Arc<[shadows::Shadow]>,
    text_shadows: Arc<[text::Shadow]>,
    transitions: Arc<[Transition]>,
    animations: Arc<[(Animation, Arc<Frames>)]>,
    /// Everything the engine computed for the element, for the properties read rarely (the
    /// host's settings, the layout).
    resolved: Arc<suhina_style::cascade::ComputedStyle>,
}
impl Computed {
    /// The computed value of `property`, written or not (a host default, an initial value).
    pub fn value(&self, property: Property) -> Value {
        self.resolved.get(property)
    }

    /// Whether the element keeps what its children paint inside its box (`overflow: hidden` or
    /// `clip`; regions by default).
    pub fn hides_overflow(&self) -> bool {
        matches!(self.value(Property::Overflow), Value::Keyword("hidden" | "clip"))
    }

    /// The element's `padding` in points: top, right, bottom, left.
    pub fn padding(&self) -> [f32; 4] {
        [Property::PaddingTop, Property::PaddingRight, Property::PaddingBottom, Property::PaddingLeft].map(|side| {
            match self.value(side) {
                Value::Length(suhina_style::value::Length::Px(points)) if points.is_finite() => {
                    points.clamp(0.0, 512.0)
                }
                _ => 0.0,
            }
        })
    }

    /// The element's outline, when it has one: its width, colour and offset in points.
    pub fn outline(&self) -> Option<(f32, Color32, f32)> {
        use suhina_style::value::{Color, Length};
        if self.value(Property::OutlineStyle) != Value::Keyword("solid") {
            return None;
        }
        let width = match self.value(Property::OutlineWidth) {
            Value::Length(Length::Px(width)) if width.is_finite() && width > 0.0 => width.min(16.0),
            _ => return None,
        };
        let color = match self.value(Property::OutlineColor) {
            Value::Color(Color::Rgba([r, g, b, a])) => Color32::from_rgba_unmultiplied(r, g, b, a),
            _ => self.color,
        };
        let offset = match self.value(Property::OutlineOffset) {
            Value::Length(Length::Px(offset)) if offset.is_finite() => offset.clamp(-64.0, 64.0),
            _ => 0.0,
        };
        (color.a() > 0).then_some((width, color, offset))
    }

    /// The computed value of `property`, when a sheet wrote it for the element or one it inherits from.
    pub fn authored_value(&self, property: Property) -> Option<Value> {
        self.resolved.authored(property).then(|| self.resolved.get(property))
    }

    /// The keyword a sheet gives `property` (`appearance: knob`), when it gives one.
    pub fn keyword(&self, property: Property) -> Option<&'static str> {
        match self.authored_value(property) {
            Some(Value::Keyword(word)) if !word.is_empty() => Some(word),
            _ => None,
        }
    }

    /// The length in points a sheet gives `property`, when it gives one (not `auto`).
    pub fn points(&self, property: Property) -> Option<f32> {
        match self.authored_value(property) {
            Some(Value::Length(suhina_style::value::Length::Px(points))) if points.is_finite() => Some(points),
            _ => None,
        }
    }

    /// The integer a sheet gives `property`, when it gives one.
    pub fn integer(&self, property: Property) -> Option<i32> {
        match self.authored_value(property) {
            Some(Value::Integer(n)) => Some(n),
            _ => None,
        }
    }

    /// The text a sheet gives a setting read as text (`value-unit`, `liquid`), when it gives one.
    pub fn text(&self, property: Property) -> Option<String> {
        match self.authored_value(property) {
            Some(Value::Text(text)) if !text.is_empty() => Some(text),
            _ => None,
        }
    }

    /// The custom property `name` (with its `--`) as the element sees it.
    pub fn variable(&self, name: &str) -> Option<&str> {
        self.variables.get(name).map(String::as_str)
    }

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
    /// The theme's swatches by colour.
    swatches: HashMap<String, Computed>,
    /// Styles worked out in the frame that needs them, by the identity of an element and of every
    /// element it is in (its chain, [`chain_of`]), as `Resolver::leaf_style` keeps a leaf's.
    identities: HashMap<u64, Styled>,
    /// `html` and `body` under it: the parent of the tree's outermost elements.
    body: Option<Arc<suhina_style::cascade::ComputedStyle>>,
    /// The ids the app gives elements, interned: a node holds an index.
    ids: Vec<Arc<str>>,
    id_index: HashMap<Arc<str>, u32>,
    /// For each node of this frame: its chain, once its style was looked up (0 before).
    chains: Vec<u64>,
    /// For each node of this frame: the box it takes on screen, once it painted or gave one.
    boxes: Vec<Option<Rect>>,
    /// For each node of this frame: the style it was painted with, to compare with the full tree's
    /// at the frame's end.
    served: Vec<Option<Arc<suhina_style::cascade::ComputedStyle>>>,
    /// For each node of this frame: whether it was served a style other than its chain's (held
    /// for a transition, or inherited from a parent that was), which its children inherit.
    held: Vec<bool>,
    /// The chain each element key had when the whole tree was last resolved: its resolved style
    /// still holds while its chain is the same.
    resolved: HashMap<u64, u64>,
    /// Whether this frame hit a limit of the tree, which the trace was told of.
    limited: bool,
    toolbar: Option<(String, Vec<Module>)>,
    workspace: Option<(String, Option<Workspace>)>,
    /// Whether the browser and the detail area follow their splitters, by the window's size and
    /// which panels are open: a splitter drag changes only the preferred sizes, which lays the
    /// workspace out again without probing the flags anew.
    workspace_follows: Option<(String, (bool, bool))>,
    toolbar_layout: bool,
    collecting: bool,
    nodes: Vec<StyleNode>,
    parents: Vec<usize>,
    occurrences: HashMap<(u64, u16), usize>,
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
    /// Styles asked for this frame by elements the tree had no room for, which get none.
    unstyled: usize,
    /// Styles worked out afresh this frame: not found among the identities (and swatches).
    fresh: usize,
    /// Whether the frame drawn last asked for another one.
    asked: bool,
}

/// What the style engine did in the frame drawn last, for budget tests (`suhina-theme-render`):
/// counts that later work may only lower.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Budget {
    /// Style nodes the frame entered.
    pub nodes: usize,
    /// Element identities whose styles are kept.
    pub identities: usize,
    /// Styles asked for by elements past the tree's limits, which got none.
    pub unstyled: usize,
    /// Styles worked out afresh in the frame (and theme swatches).
    pub fresh: usize,
    /// Whether the style engine asked for another frame: a style it painted differed from the
    /// whole tree's, or something moves.
    pub asked: bool,
}

/// The counts of the frame drawn last; read them after the frame, before the next begins.
pub fn budget() -> Budget {
    ENGINE.with(|engine| {
        let engine = engine.borrow();
        Budget {
            nodes: engine.nodes.len(),
            identities: engine.identities.len(),
            unstyled: engine.unstyled,
            fresh: engine.fresh,
            asked: engine.asked,
        }
    })
}

#[derive(Clone, Debug, PartialEq, Eq)]
struct StyleNode {
    key: u64,
    parent: Option<usize>,
    element: super::element::Name,
    /// The id the app gave it, interned in [`Engine::ids`].
    id: Option<u32>,
    /// Pointer states (hover 1, active 2, disabled 4, focus 8) and the host's ([`mark_node`]).
    state: u16,
    /// Custom properties the host gives the element ([`set_variable`]).
    variables: Vec<(String, String)>,
    /// Attributes with a value the host gives the element (`color`), beside the states in
    /// `state` that are attributes without one.
    attributes: Vec<(String, String)>,
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

/// Gives the device panel element at `node` its own style sheet: it applies only inside that
/// panel, above the application theme and below `user.css`. Unchanged text is parsed once; a
/// sheet that does not parse is ignored and the panel keeps the theme. Given before the panel's
/// style is looked up ([`super::element::Builder::sheet`]).
pub(super) fn device_sheet(node: Option<usize>, ctx: &egui::Context, source: Option<&str>) {
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
        if let Some(node) = node.and_then(|at| engine.nodes.get_mut(at)) {
            node.sheet = key;
        }
    });
}

/// Finishes the style snapshot even on an early return from the native frame.
pub struct Frame(egui::Context, f64);
impl Drop for Frame {
    fn drop(&mut self) {
        let (changed, animating) = suhina_plugins::trace::timed("native CSS tree", || finish_tree(self.1));
        ENGINE.with(|engine| engine.borrow_mut().asked = changed || animating);
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
        if engine.ids.len() >= MAX_IDS {
            // No node holds an index now: the table starts again between frames, so the elements'
            // keys change with the ids and their styles are worked out again.
            engine.ids.clear();
            engine.id_index.clear();
        }
        engine.nodes.clear();
        engine.parents.clear();
        engine.occurrences.clear();
        engine.chains.clear();
        engine.boxes.clear();
        engine.served.clear();
        engine.held.clear();
        engine.unstyled = 0;
        engine.fresh = 0;
        engine.limited = false;
    });
    Frame(ui.ctx().clone(), ui.input(|input| input.time))
}

/// Register an actual native scope. Named project roles retain identity when reordered. Returns
/// the node's index in this frame's tree, or `None` when the engine is not collecting it.
pub(super) fn enter(
    element: super::element::Name,
    name: Option<&str>,
    response: Option<&egui::Response>,
) -> Option<usize> {
    ENGINE.with(|engine| {
        let mut engine = engine.borrow_mut();
        if !engine.collecting {
            return None;
        }
        if engine.nodes.len() >= MAX_NODES || engine.parents.len() >= MAX_DEPTH {
            limit(&mut engine, "the style tree is full (4096 elements, 16 deep): the rest draw without styles");
            return None;
        }
        let name = name.filter(|name| {
            name.len() <= 96 && name.bytes().all(|b| b.is_ascii_alphanumeric() || matches!(b, b'-' | b'_' | b'.'))
        });
        let id = name.and_then(|name| intern(&mut engine, name));
        let parent = engine.parents.last().copied();
        let parent_key = parent.and_then(|p| engine.nodes.get(p)).map_or(0, |n| n.key);
        let occurrence = engine.occurrences.entry((parent_key, element.index())).or_default();
        let index = *occurrence;
        *occurrence += 1;
        let mut hash = std::hash::DefaultHasher::new();
        (parent_key, element.index(), id).hash(&mut hash);
        if let Some(response) = response {
            response.id.hash(&mut hash);
        } else if id.is_none() {
            index.hash(&mut hash);
        }
        let key = hash.finish();
        let state = response.map_or(0, |r| {
            u16::from(r.hovered())
                | u16::from(r.is_pointer_button_down_on()) << 1
                | u16::from(!r.enabled()) << 2
                | u16::from(r.has_focus()) << 3
        });
        let node =
            StyleNode { key, parent, element, id, state, variables: Vec::new(), attributes: Vec::new(), sheet: 0 };
        let index = engine.nodes.len();
        engine.nodes.push(node);
        engine.chains.push(0);
        engine.boxes.push(None);
        engine.served.push(None);
        engine.held.push(false);
        engine.parents.push(index);
        Some(index)
    })
}

/// `name` as an index into [`Engine::ids`].
fn intern(engine: &mut Engine, name: &str) -> Option<u32> {
    if let Some(id) = engine.id_index.get(name) {
        return Some(*id);
    }
    if engine.ids.len() >= MAX_IDS {
        // The nodes entered this frame hold indices into the table, so it starts again only when
        // the next frame begins (`frame_with_motion`); until then the element has no id.
        limit(engine, "the style engine's ids are full: they start again next frame");
        return None;
    }
    let id = u32::try_from(engine.ids.len()).ok()?;
    let name: Arc<str> = Arc::from(name);
    engine.ids.push(Arc::clone(&name));
    engine.id_index.insert(name, id);
    Some(id)
}

/// Tells the trace, once a frame, that the engine hit one of its limits.
fn limit(engine: &mut Engine, what: &str) {
    if !std::mem::replace(&mut engine.limited, true) {
        suhina_plugins::trace::line(format_args!("native CSS: {what}"));
    }
}

/// A state of a control or a track that only the host knows, for selectors such as `[checked]`.
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

/// Gives the element of this frame's tree at `node` the custom property `name` (`--track-colour`)
/// as `value`, which it and its descendants see below every rule of the sheet.
pub(super) fn set_variable(node: usize, name: &str, value: String) {
    ENGINE.with(|engine| {
        if let Some(node) = engine.borrow_mut().nodes.get_mut(node)
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

/// Marks the element of this frame's tree at `node` with `state`, so the sheet's `[checked]` and
/// the others match it: given before its style is looked up, in this frame.
pub(super) fn mark_node(node: usize, state: HostState) {
    ENGINE.with(|engine| {
        if let Some(node) = engine.borrow_mut().nodes.get_mut(node) {
            node.state |= state as u16;
        }
    });
}

/// The name the app gave the element of this frame's tree at `node` (`play` for `#control-play`).
pub(super) fn node_name(node: usize) -> Option<String> {
    ENGINE.with(|engine| {
        let engine = engine.borrow();
        engine
            .nodes
            .get(node)
            .and_then(|node| node.id)
            .and_then(|id| engine.ids.get(id as usize))
            .map(|id| id.to_string())
    })
}

/// The box the element at `node` takes on screen this frame (`rect`, as far as `clip` shows it),
/// for the elements inside it that it clips (`overflow`).
pub(super) fn set_box(node: Option<usize>, rect: Rect, clip: Rect) {
    let Some(node) = node else { return };
    ENGINE.with(|engine| {
        if let Some(slot) = engine.borrow_mut().boxes.get_mut(node) {
            *slot = Some(rect.intersect(clip));
        }
    });
}

/// The box of the nearest element around `node` that hides its overflow and whose box is known:
/// what clips the element's outer shadows and outline. `None` leaves the painter's own clip.
pub(super) fn outer_clip(node: Option<usize>) -> Option<Rect> {
    let node = node?;
    ENGINE.with(|engine| {
        let engine = engine.borrow();
        let mut at = engine.nodes.get(node)?.parent;
        while let Some(parent) = at {
            let hides = engine
                .served
                .get(parent)
                .cloned()
                .flatten()
                .is_some_and(|style| matches!(style.get(Property::Overflow), Value::Keyword("hidden" | "clip")));
            if hides && let Some(Some(rect)) = engine.boxes.get(parent) {
                return Some(*rect);
            }
            at = engine.nodes.get(parent)?.parent;
        }
        None
    })
}

/// Gives the element of this frame's tree at `node` the pointer states `bits` (hover 1, active 2,
/// disabled 4).
pub(super) fn mark_node_bits(node: usize, bits: u16) {
    if bits == 0 {
        return;
    }
    ENGINE.with(|engine| {
        if let Some(node) = engine.borrow_mut().nodes.get_mut(node) {
            node.state |= bits;
        }
    });
}

/// Gives the element of this frame's tree at `node` the attribute `name` with `value`. A
/// duplicate name is dropped, and so is an attribute past the style tree's limit
/// ([`suhina_style::selector::MAX_ATTRIBUTES`]) less the one each host state adds when the tree is
/// built (`native::retained`): an element over the limit would match no rule at all.
pub(super) fn set_attribute(node: usize, name: &str, value: String) {
    ENGINE.with(|engine| {
        if let Some(node) = engine.borrow_mut().nodes.get_mut(node)
            && node.attributes.len() < suhina_style::selector::MAX_ATTRIBUTES - HOST_STATES.len()
            && !node.attributes.iter().any(|(own, _)| own == name)
        {
            node.attributes.push((name.to_owned(), value));
        }
    });
}

pub fn leave() {
    ENGINE.with(|engine| {
        engine.borrow_mut().parents.pop();
    });
}

/// The id a theme selects a scope by: the registry's ids keep the name the app gave
/// (`#control-play`, `#device-compressor`); others get one derived from their key.
fn element_id(engine: &Engine, node: &StyleNode) -> String {
    suhina_theme_package::elements::element_id(node.element.name(), name_of(engine, node))
        .unwrap_or_else(|| format!("node-{}", node.key))
}

/// The id the app gave `node`.
fn name_of<'a>(engine: &'a Engine, node: &StyleNode) -> Option<&'a str> {
    node.id.and_then(|id| engine.ids.get(id as usize)).map(|id| id.as_ref())
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
        let mut stale = false;
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
            // What the frame painted from its own chains, against the whole tree's: they differ only
            // where siblings (`+`, `~`, `:nth-child`) or a hover reaching an ancestor matter, which
            // the next frame paints.
            stale = engine.nodes.iter().zip(&engine.served).any(|(node, served)| {
                served.as_ref().is_some_and(|served| {
                    targets
                        .get(&node.key)
                        .is_some_and(|target| !Arc::ptr_eq(&target.resolved, served) && *target.resolved != **served)
                })
            });
            let Engine { nodes, chains, resolved, .. } = &mut *engine;
            resolved.clear();
            for (node, chain) in nodes.iter().zip(chains.iter()).filter(|(_, chain)| **chain != 0) {
                resolved.insert(node.key, *chain);
            }
            engine.targets = targets;
            engine.previous = engine.nodes.clone();
        }
        std::mem::take(&mut engine.styles_changed);
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
        // painted its penultimate values. A changed tree whose styles the frame worked out itself
        // needs no other frame (#1273).
        (stale || moving || moving_before, moving)
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
        let needed = format!("--{variable}");
        if key != "text.primary"
            && !native::PART_COLOURS.contains(&needed)
            && !source.as_deref().is_some_and(|css| css.contains('\\') || css.contains(&needed))
        {
            continue;
        }
        variables.push_str(&format!("--{variable}:{};", theme::hex(color)));
    }
    variables.push_str("}\n");
    let user = USER_SHEET.with(|sheet| sheet.borrow().clone());
    let variant = skin::current().variant.clone();
    ENGINE.with(|engine| {
        let mut engine = engine.borrow_mut();
        if engine.variant.as_ref().map(|(text, _)| text) != variant.as_ref() {
            engine.variant = variant
                .and_then(|text| Some((Arc::clone(&text), Arc::new(suhina_style::sheet::Sheet::parse(&text).ok()?))));
            engine.styles_changed = true;
            engine.reset_styles();
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
            engine.reset_styles();
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
        } else if engine.size != size || engine.palette != variables {
            // Palette drags and native resizing do not change the scope tree or
            // authored CSS. Keep the last computed snapshot and resolve the new inputs
            // once at the end of this pass.
            engine.palette = variables;
            let palette = engine.palette.clone();
            if let Some(paint) = engine.native.as_mut() {
                let _ = paint.palette(&palette);
            }
            engine.size = size;
            engine.styles_changed = true;
            engine.reset_styles();
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

impl Engine {
    /// Forgets every style worked out, after the sheets, the palette or the window's size changed.
    fn reset_styles(&mut self) {
        self.identities.clear();
        self.swatches.clear();
        self.body = None;
        self.resolved.clear();
        self.toolbar = None;
        self.workspace = None;
        self.workspace_follows = None;
    }
}

/// The style a stack of elements named from the root gives the innermost, for tests.
#[cfg(test)]
pub(super) fn scoped(stack: &[String]) -> Option<Computed> {
    ENGINE.with(|engine| {
        let engine = engine.borrow();
        engine.source.as_ref()?;
        native::scoped(&engine, stack)
    })
}

/// The style of the element at `node` in this frame's tree, worked out now if this frame has not
/// yet (its own: never an enclosing element's). `None` without a sheet, or for an element the tree
/// had no room for.
pub(super) fn style_of(node: Option<usize>) -> Option<Computed> {
    ENGINE.with(|engine| {
        let mut engine = engine.borrow_mut();
        engine.source.as_ref()?;
        if !same_source(engine.source.as_ref()) {
            return None;
        }
        let Some(node) = node else {
            if engine.collecting {
                engine.unstyled += 1;
            }
            return None;
        };
        style_at(&mut engine, node).map(|styled| styled.computed)
    })
}

/// The identity of `node` and of every element it is in: what its style depends on, apart from
/// its siblings. Its parent's style is looked up first, if it was not yet.
fn chain_of(engine: &mut Engine, at: usize) -> Option<u64> {
    if let Some(chain) = engine.chains.get(at).copied().filter(|chain| *chain != 0) {
        return Some(chain);
    }
    // The parent's chain holds its device sheet, and every other sheet above it.
    let parent_chain = match engine.nodes.get(at)?.parent {
        Some(parent) => chain_of(engine, parent)?,
        None => 0,
    };
    let node = engine.nodes.get(at)?;
    let mut hash = std::hash::DefaultHasher::new();
    (parent_chain, node.element.index(), name_of(engine, node), node.state, node.sheet).hash(&mut hash);
    node.attributes.hash(&mut hash);
    node.variables.hash(&mut hash);
    let chain = hash.finish().max(1);
    *engine.chains.get_mut(at)? = chain;
    Some(chain)
}

/// The style of `node`, and what it was painted with recorded: the whole tree's as last resolved
/// when its chain is the same as then (that one knows its siblings and its transitions), else
/// the one worked out for its chain now.
fn style_at(engine: &mut Engine, at: usize) -> Option<Styled> {
    let chain = chain_of(engine, at)?;
    let node = engine.nodes.get(at)?;
    let key = node.key;
    // A parent served a style its chain does not give (held for a transition) passes that style
    // on: what the element inherits from it is not its chain's, so it is neither looked up among
    // the identities nor kept there.
    let parent_held = node.parent.is_some_and(|parent| engine.held.get(parent).copied().unwrap_or(false));
    let resolved = engine.resolved.get(&key) == Some(&chain);
    let previous =
        engine.values.get(&key).map(|values| Styled { raw: Arc::clone(&values.resolved), computed: values.clone() });
    let (styled, held) = match previous.clone().filter(|_| resolved) {
        Some(styled) => (styled, false),
        None => {
            let fresh = match engine.identities.get(&chain).filter(|_| !parent_held) {
                Some(styled) => styled.clone(),
                None => {
                    let styled = native::fresh(engine, at)?;
                    engine.fresh += 1;
                    if !parent_held {
                        if engine.identities.len() >= MAX_IDENTITIES {
                            limit(engine, "the style engine's identities are full: they start again");
                            engine.identities.clear();
                        }
                        engine.identities.insert(chain, styled.clone());
                    }
                    styled
                }
            };
            // An element with transitions keeps its painted values for now: the end of the frame
            // starts the transition from them.
            match previous {
                Some(previous) if engine.motion && !fresh.computed.transitions.is_empty() => {
                    let held = parent_held || (!Arc::ptr_eq(&previous.raw, &fresh.raw) && *previous.raw != *fresh.raw);
                    (previous, held)
                }
                _ => (fresh, parent_held),
            }
        }
    };
    if let Some(served) = engine.served.get_mut(at) {
        *served = Some(Arc::clone(&styled.raw));
    }
    if let Some(slot) = engine.held.get_mut(at) {
        *slot = held;
    }
    Some(styled)
}

/// A theme's swatch of `color`, drawn as the theme in use styles a `theme-swatch` element with
/// that `color` attribute (`docs/spec/theme-skins.md` › Swatches); `None` without a theme sheet.
pub fn theme_swatch(color: Color32) -> Option<Computed> {
    let key = ["theme-swatch".to_owned(), hex(color)];
    ENGINE.with(|engine| {
        let mut engine = engine.borrow_mut();
        engine.source.as_ref()?;
        if !same_source(engine.source.as_ref()) {
            return None;
        }
        if let Some(values) = engine.swatches.get(&key[1]) {
            return Some(values.clone());
        }
        let values = native::theme_swatch(&engine, &key[1])?;
        engine.fresh += 1;
        // Past the cache's limit a swatch is worked out each time, so that every swatch in a
        // gallery is styled alike.
        if engine.swatches.len() < MAX_CONTEXTS {
            engine.swatches.insert(key[1].clone(), values.clone());
        } else {
            limit(&mut engine, "the theme swatches are full: the rest are worked out each time");
        }
        Some(values)
    })
}

/// The sheets settings are read from: the theme's, its variant's and the user's, as given (the
/// key, compared by pointer, and held so that no other text can take its address) and parsed.
struct SettingsSheets {
    sources: (Arc<str>, Option<Arc<str>>, Option<Arc<str>>),
    parsed: Option<(Sheet, Option<Sheet>, Option<Sheet>)>,
    /// The settings looked up by element path.
    found: HashMap<Vec<&'static str>, Option<Computed>>,
}

/// Whether `a` and `b` are the same text, by pointer.
fn same_arc(a: Option<&Arc<str>>, b: Option<&Arc<str>>) -> bool {
    match (a, b) {
        (Some(a), Some(b)) => Arc::ptr_eq(a, b),
        (None, None) => true,
        _ => false,
    }
}

/// The style the theme's sheets give the elements named by `path` (from `window` inward), for the
/// settings Suhina reads outside a frame: the Arrangement view's track height, a device's
/// parameter labels. Worked out once per set of sheets and path, which are known by their
/// pointers: asked for by every track header in every frame, it hashes no sheet's text. `None`
/// without a sheet.
pub fn settings_of(path: &[super::element::Name]) -> Option<Computed> {
    thread_local! {
        static CACHE: RefCell<Option<SettingsSheets>> = const { RefCell::new(None) };
    }
    let skin = skin::current();
    let css = skin.css.clone()?;
    let user = USER_SHEET.with(|sheet| sheet.borrow().clone());
    let key: Vec<&'static str> = path.iter().map(|name| name.name()).collect();
    CACHE.with(|cache| {
        let mut cache = cache.borrow_mut();
        let current = cache.as_ref().is_some_and(|cache| {
            let (theme, variant, own) = &cache.sources;
            Arc::ptr_eq(theme, &css)
                && same_arc(variant.as_ref(), skin.variant.as_ref())
                && same_arc(own.as_ref(), user.as_ref())
        });
        if !current {
            let parsed = Sheet::parse(&css).ok().map(|sheet| {
                let parse = |text: &Option<Arc<str>>| text.as_deref().and_then(|text| Sheet::parse(text).ok());
                (sheet, parse(&skin.variant), parse(&user))
            });
            *cache = Some(SettingsSheets {
                sources: (Arc::clone(&css), skin.variant.clone(), user.clone()),
                parsed,
                found: HashMap::new(),
            });
        }
        let cache = cache.as_mut()?;
        if let Some(found) = cache.found.get(&key) {
            return found.clone();
        }
        let (sheet, variant, user) = cache.parsed.as_ref()?;
        let found = native::settings_of(sheet, variant.as_ref(), user.as_ref(), &key);
        if cache.found.len() < MAX_CONTEXTS {
            cache.found.insert(key, found.clone());
        }
        found
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
            "toolbar {{ display:flex; width:{}px; height:{}px; gap:4px; align-items:center; }} toolbar-module {{ flex-shrink:0; height:{}px; }}",
            size.x.clamp(1.0, 16384.0),
            size.y.clamp(1.0, 16384.0),
            size.y.clamp(24.0, 16384.0)
        );
        for (id, width) in order {
            defaults.push_str(&format!("#{id} {{ width:{}px; }}", width.clamp(1.0, 4096.0)));
        }
        let mut elements = vec![native::element("toolbar", Some("toolbar"), None, Some(1))];
        for (id, _) in order {
            elements.push(native::element("toolbar-module", Some(id), None, Some(2)));
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
