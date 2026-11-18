// Copyright (C) 2026 The Suhina contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! The elements code draws, and the only way to an element's computed style (ADR 0031 ›
//! Amendment: the element registry).
//!
//! ```ignore
//! let el = KNOB.id("send").response(&response).attr(Checked, on).enter();
//! let boxed = el.paint(&painter, rect); // shadows, background, gradients, inset shadows, border
//! draw_contents(boxed.content); // the box less its border and padding
//! drop(el); // clips what the contents painted (`overflow`), then paints the outline
//! let style = CLIP.color(clip.color).style(); // a drawn item
//! ```
//!
//! An entered element stays in the style tree (and its style scope) until the value is
//! dropped. Its [`Entered::style`] is its own, worked out in the frame that enters it from its
//! parent's and its own identity, never the style of an element that encloses it, so a part that
//! forgets to enter its own element cannot paint as its parent. Kit's shared painters
//! (`mixer_controls`' level bars and pan field, `widgets::button_surface`) take the element their
//! caller entered. [`Entered::paint`] paints in CSS's order; when the element is dropped, what its
//! contents painted is clipped to its padding box if it hides its overflow, and its outline is
//! painted over them.

use egui::{Color32, Painter, Rect, Response};
use suhina_style::property::Property;

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
    /// A clip or a track's stop button waiting for its launch.
    Queued,
}
pub use Attr::{Armed, Checked, Muted, Playing, Queued, Recording, Selected};

/// Every [`Attr`] with the host state that marks it.
const STATES: [(Attr, HostState); 7] = [
    (Checked, HostState::Checked),
    (Selected, HostState::Selected),
    (Playing, HostState::Playing),
    (Recording, HostState::Recording),
    (Armed, HostState::Armed),
    (Muted, HostState::Muted),
    (Queued, HostState::Queued),
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
/// element's contents: the rectangle given, less the element's border and `padding`.
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
    /// The element described with nothing yet said about it.
    pub fn builder<'a>(self) -> Builder<'a> {
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
    /// The style of a drawn item of this element, with nothing more said about it.
    pub fn style(self) -> Option<Computed> {
        self.builder().style()
    }
    /// The [`Look`] of the drawn items of this element, with nothing more said about them.
    pub fn look(self) -> Look {
        self.builder().look()
    }
    /// [`Builder::probe`] of this element.
    pub fn probe(self) -> Look {
        self.builder().probe()
    }
    /// The [`Look`] of this element inside `parent`, neither of them left in the style tree: a
    /// part as it is in its control with a state off (`slider-fill` in a `slider` without
    /// `[checked]`), to compare the part in the lit control with.
    pub fn probe_in(self, parent: Builder<'_>) -> Look {
        let outer = parent.enter();
        let look = self.probe();
        let node = outer.scope.node;
        drop(outer);
        super::native_css::discard(node);
        look
    }
    /// [`Builder::enter_against_plain`] of this element.
    pub fn enter_against_plain(self, attr: Attr, on: bool) -> (Entered, Option<Look>) {
        self.builder().enter_against_plain(attr, on)
    }
    pub fn enter(self) -> Entered {
        self.builder().enter()
    }
}

/// An element being described before it is entered.
#[must_use]
#[derive(Clone)]
pub struct Builder<'a> {
    name: Name,
    id: Option<&'a str>,
    response: Option<&'a Response>,
    /// The [`Attr`]s that are on, a bit each (`1 << attr as u8`).
    states: u8,
    /// Pointer states given without a response: hover 1, active 2, disabled 4, focus 8.
    pointer: u16,
    /// A device panel's own sheet.
    sheet: Option<&'a str>,
    color: Option<Color32>,
    /// Attributes with a value the app gives the element besides its colour (`kind`).
    attributes: Vec<(&'static str, &'a str)>,
    native: Native,
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
        self.pointer = (self.pointer & 8) | u16::from(hovered) | u16::from(pressed) << 1 | u16::from(disabled) << 2;
        self
    }
    /// The element has the keyboard's focus (`:focus`), for one drawn without a response.
    pub fn focused(mut self, on: bool) -> Self {
        self.pointer = (self.pointer & !8) | u16::from(on) << 3;
        self
    }
    /// A device panel's own style sheet (`source`), which applies inside the panel only, over the
    /// application's theme ([`super::native_css`]).
    pub fn sheet(mut self, source: Option<&'a str>) -> Self {
        self.sheet = source;
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

    /// Puts the element in the style tree and the style scope until the value is dropped. The
    /// pointer states it was given are the pointer's: `.pointer(true, ..)` puts the pointer over
    /// it, so its ancestors are `:hover` too.
    pub fn enter(self) -> Entered {
        self.enter_as(false)
    }

    /// [`Builder::enter`]; `asked` says the pointer states are a question (what the element looks
    /// like under the pointer, for a lookup), not where the pointer is: the element's ancestors
    /// are not `:hover` for it.
    fn enter_as(self, asked: bool) -> Entered {
        let Self { name, id, response, states, pointer, color, attributes, native, sheet } = self;
        let scope = super::style_class::enter_inner(name, id, response, |node| {
            super::native_css::mark_node_bits(node, pointer, asked);
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
            if let Some(source) = sheet {
                super::native_css::device_sheet(node, source);
            }
        });
        Entered { scope, native, name, rest: std::cell::RefCell::new(None) }
    }

    /// The style of a drawn item: the element entered and left at once.
    pub fn style(self) -> Option<Computed> {
        self.enter_as(true).style()
    }

    /// The [`Look`] of an element that is not drawn: what a state's rule is compared with
    /// (`[checked]` against the plain item). It takes no place in the style tree, so no selector of
    /// the elements drawn after it sees it as a sibling. Like any element styled in the frame it is
    /// first drawn in, it is worked out without its siblings, so a rule that reaches a toggle only
    /// through a sibling combinator (`tool + tool`) is not in the comparison.
    ///
    /// It is its own chain's style, never the values the tree last painted for a drawn element
    /// with the same key (held for a CSS transition), so a state's rule is not on both sides of
    /// the comparison.
    pub fn probe(self) -> Look {
        let entered = self.enter_as(true);
        let node = entered.scope.node;
        let style = match node {
            Some(_) => super::native_css::probe_style(node),
            None => entered.style(),
        };
        drop(entered);
        super::native_css::discard(node);
        Look(style)
    }

    /// [`Builder::probe`] of the element as if drawn inside `ancestor` (a `clip` of the
    /// Arrangement, looked up from the piano roll), so a rule that reaches it only there
    /// (`arrangement clip[muted]`) is in its look. The ancestor is taken out of the style tree
    /// again with it.
    pub fn probe_in(self, ancestor: Name) -> Look {
        let around = ancestor.enter();
        let node = around.scope.node;
        let look = self.probe();
        drop(around);
        super::native_css::discard(node);
        look
    }

    /// The element entered with `attr` `on`, and while it is on, the [`Look`] of the very same
    /// element with only `attr` off: its id, response (so its `:hover`, `:active` and focus),
    /// pointer states, colour, attributes and other states kept. That is what
    /// [`Look::themed_unlike`] and the others compare a state's rule with, so only a rule for
    /// the state itself (`button[checked]`) counts as the state's colour, not one for the
    /// pointer's state (`button:hover`) or for every such element.
    pub fn enter_against_plain(self, attr: Attr, on: bool) -> (Entered, Option<Look>) {
        // Looked up first, so it is the last node when it is taken out again.
        let plain = on.then(|| self.clone().attr(attr, false).probe());
        (self.attr(attr, on).enter(), plain)
    }

    /// A toggle's element, entered with `attrs` while it is `on`, and while it is lit at all (`on`,
    /// or `fading` out), the two [`Sides`] its fade runs between: the same element with `attrs`
    /// on and with them off, everything else kept (its id, its response and so its `:hover`). The
    /// one not entered is looked up first and taken out again ([`Builder::probe`]), so it takes
    /// no place in the style tree. [`super::widgets::Toggle::of`] compares them.
    pub fn enter_toggle(self, attrs: &[Attr], on: bool, fading: bool) -> (Entered, Option<Sides>) {
        let with = |builder: Self, on: bool| attrs.iter().fold(builder, |builder, attr| builder.attr(*attr, on));
        if on {
            let plain = with(self.clone(), false).probe();
            let el = with(self, true).enter();
            let lit = el.look();
            (el, Some(Sides { lit, plain, entered_lit: true }))
        } else if fading {
            let lit = with(self.clone(), true).probe();
            let el = with(self, false).enter();
            let plain = el.look();
            (el, Some(Sides { lit, plain, entered_lit: false }))
        } else {
            (with(self, false).enter(), None)
        }
    }

    /// The element's [`Look`] at rest and under the pointer (`:hover`), for a fade from one to the
    /// other that is `amount` of the way (0 none, 1 all): looked up as comparisons
    /// ([`Builder::probe`]), the hovered one only while there is some hover to show (else the
    /// resting one again). The builder is given no response, which would say where the pointer is.
    pub fn hover_ends(self, amount: f32) -> (Look, Look) {
        let rest = self.clone().pointer(false, false, false).probe();
        if amount.is_finite() && amount > 0.0 {
            let hot = self.pointer(true, false, false).probe();
            (rest, hot)
        } else {
            (rest.clone(), rest)
        }
    }

    /// The [`Look`] of every drawn item of this kind: looked up once, outside the loop that draws
    /// them, so a line drawn a hundred times is one element of the style tree.
    pub fn look(self) -> Look {
        Look(self.style())
    }
}

/// A toggle's two looks, lit and plain ([`Builder::enter_toggle`]).
#[derive(Clone, Debug)]
pub struct Sides {
    /// The element with its state on.
    pub lit: Look,
    /// The same element with its state off.
    pub plain: Look,
    /// Whether the element entered is the lit one: it is on, not fading out.
    pub entered_lit: bool,
}

/// What a sheet says about a kind of drawn item (a grid line, a slot's mark): its computed colours,
/// the base sheet's included, or for a reader that asks for one, a fallback colour where no rule of
/// a theme reaches the item (#1417).
#[derive(Clone, Debug, Default)]
pub struct Look(Option<Computed>);

/// Which of an element's colours a [`Look`] reads where a theme's rule gives it.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Paint {
    /// Its text `color`, inherited from a sheet's rule around it too.
    Color,
    /// Its own `color`: one a rule on the element itself sets, even when it equals the inherited
    /// one. A line or a mark does not take the text colour a view's rule gives everything in it.
    OwnColor,
    /// Its `background-color`.
    Background,
    /// Its `border-color`.
    Border,
    /// Its `accent-color`.
    Accent,
}

impl Paint {
    /// The colour `style` has for this paint, where a theme's rule gives it (the base sheet's does
    /// not count).
    fn themed(self, style: &Computed) -> Option<Color32> {
        let (set, color) = match self {
            Self::Color => (style.authored.color, style.color),
            Self::OwnColor => (style.declared(Property::Color), style.color),
            Self::Background => (style.authored.background, style.background),
            Self::Border => (style.authored.border, style.border),
            Self::Accent => (style.authored.accent, style.accent),
        };
        set.then_some(color)
    }
}

impl Look {
    /// The item's computed `color`, the base sheet's where no theme gives one; transparent where
    /// no sheet styles the window.
    pub fn color(&self) -> Color32 {
        self.computed_color_or(Color32::TRANSPARENT)
    }

    /// The item's computed `background-color` ([`Look::color`]).
    pub fn background(&self) -> Color32 {
        self.computed_background_or(Color32::TRANSPARENT)
    }

    /// The item's computed `border-color` ([`Look::color`]).
    pub fn border(&self) -> Color32 {
        self.0.as_ref().map_or(Color32::TRANSPARENT, Computed::border)
    }

    /// The item's computed `accent-color` ([`Look::color`]).
    pub fn accent(&self) -> Color32 {
        self.0.as_ref().map_or(Color32::TRANSPARENT, Computed::accent)
    }

    /// The item's computed `color`, the base and user-agent sheets' included: for an element whose
    /// colour those sheets give (`knob-body`, `fader-cap`, `sample-point[kind]`), so a colour a
    /// theme gives an element around it does not reach it. `fallback` only where no sheet styles
    /// the window.
    pub fn computed_color_or(&self, fallback: Color32) -> Color32 {
        self.0.as_ref().map_or(fallback, Computed::color)
    }

    /// The item's computed `background-color`, the base and user-agent sheets' included (often
    /// through a variable): a background a theme gives an element around it does not reach it,
    /// while a variable the theme sets and its own rule for the element do. `fallback` only where
    /// no sheet styles the window.
    pub fn computed_background_or(&self, fallback: Color32) -> Color32 {
        self.0.as_ref().map_or(fallback, Computed::background)
    }

    /// The item's colour `paint`, where a theme's rule gives it one.
    pub fn themed(&self, paint: Paint) -> Option<Color32> {
        self.0.as_ref().and_then(|style| paint.themed(style))
    }

    /// The item's own `color` ([`Paint::OwnColor`]), else `token`.
    pub fn own_color_or(&self, token: Color32) -> Color32 {
        self.themed(Paint::OwnColor).unwrap_or(token)
    }

    /// The item's own `color` ([`Paint::OwnColor`]), where a theme's rule on it gives one.
    pub fn own_color(&self) -> Option<Color32> {
        self.themed(Paint::OwnColor)
    }

    /// The item's text colour, inherited from a sheet's rule around it too; else `token`.
    pub fn color_or(&self, token: Color32) -> Color32 {
        self.themed(Paint::Color).unwrap_or(token)
    }

    /// The item's `font-size`, where the sheet sets one; else `fallback`.
    pub fn font_size_or(&self, fallback: f32) -> f32 {
        self.0.as_ref().filter(|s| s.authored.font_size).map_or(fallback, |s| s.font_size)
    }

    /// The item's `background-color`, where the sheet sets one; else `token`.
    pub fn background_or(&self, token: Color32) -> Color32 {
        self.themed(Paint::Background).unwrap_or(token)
    }

    /// The item's `border-color`, where the sheet sets one; else `token`: for a line the element
    /// draws along its edge (a column's separator, a selected slot's frame).
    pub fn border_or(&self, token: Color32) -> Color32 {
        self.themed(Paint::Border).unwrap_or(token)
    }

    /// The item's `accent-color`, where the sheet sets one; else `token`: a control's accent, such
    /// as a dial's pointer or the line under a field that has the keys.
    pub fn accent_or(&self, token: Color32) -> Color32 {
        self.themed(Paint::Accent).unwrap_or(token)
    }

    /// The item's computed style, when a sheet styles the window.
    pub fn style(&self) -> Option<&Computed> {
        self.0.as_ref()
    }

    /// The item's colour `paint` in a state (`[checked]`, `[queued]`) where it differs from
    /// `plain`, the same item without the state; else `token`, the state's own colour. A rule for
    /// every `button` then does not hide which one is on: only a rule for the state does.
    pub fn unlike(&self, plain: &Look, paint: Paint, token: Color32) -> Color32 {
        self.themed_unlike(plain, paint).unwrap_or(token)
    }

    /// [`Look::unlike`] with no token: `None` where the state has no colour `paint` of its own.
    pub fn themed_unlike(&self, plain: &Look, paint: Paint) -> Option<Color32> {
        self.themed(paint).filter(|own| Some(*own) != plain.themed(paint))
    }

    /// Whether the item has a background image (`background-image`) that `plain` does not have:
    /// a state's own image, not the one a rule for every such item gives.
    pub fn image_unlike(&self, plain: &Look) -> bool {
        match (&self.0, &plain.0) {
            (Some(own), Some(plain)) => own.has_gradient() && !own.same_image(plain),
            (Some(own), None) => own.has_gradient(),
            _ => false,
        }
    }
}

/// An element in scope.
#[must_use]
pub struct Entered {
    scope: super::style_class::Scope,
    native: Native,
    name: Name,
    /// What the element does once its contents are painted ([`Entered::paint`]).
    rest: std::cell::RefCell<Option<Rest>>,
}

/// What an element does once its contents are painted: clip them, then paint its outline.
struct Rest {
    painter: Painter,
    rect: Rect,
    radius: f32,
    style: Computed,
    outer: Option<Rect>,
    /// The first shape of its contents in the painter's layer, and the padding box they are kept
    /// in, when it hides its overflow.
    clip: Option<(egui::layers::ShapeIdx, Rect)>,
}

impl Rest {
    fn paint(self) {
        if let Some((from, to)) = self.clip {
            clip_contents(&self.painter, from, to);
        }
        super::native_css::box_end(&self.painter, self.rect, self.radius, &self.style, self.outer);
    }
}

/// Keeps every shape painted in `painter`'s layer since `from` inside `to` (`overflow: hidden`):
/// whichever painter or `Ui` painted it, so a child cannot paint past its parent's padding box.
/// Menus, popups and tooltips are other layers, which the clip leaves alone, as CSS's top layer.
fn clip_contents(painter: &Painter, from: egui::layers::ShapeIdx, to: Rect) {
    painter.ctx().graphics_mut(|layers| {
        let list = layers.entry(painter.layer_id());
        for index in from.0..list.next_idx().0 {
            list.mutate_shape(egui::layers::ShapeIdx(index), |shape| {
                #[cfg(debug_assertions)]
                count_cut(shape, to);
                shape.clip_rect = shape.clip_rect.intersect(to);
            });
        }
    });
}

#[cfg(debug_assertions)]
thread_local! {
    static CUTS: std::cell::Cell<usize> = const { std::cell::Cell::new(0) };
}

/// Counts a shape whose visible part an element's `overflow` clip cuts ([`overflow_cuts`]).
#[cfg(debug_assertions)]
fn count_cut(shape: &egui::epaint::ClippedShape, to: Rect) {
    // Shapes that show nothing (a widget's transparent frame, an empty text) are not cut.
    let shows = match &shape.shape {
        egui::Shape::Mesh(mesh) => mesh.vertices.iter().any(|vertex| vertex.color.a() > 0),
        egui::Shape::Text(text) => !text.galley.text().trim().is_empty(),
        egui::Shape::Rect(rect) => rect.fill.a() > 0 || (rect.stroke.width > 0.0 && rect.stroke.color.a() > 0),
        egui::Shape::Noop => false,
        _ => true,
    };
    let seen = shape.shape.visual_bounding_rect().intersect(shape.clip_rect);
    if shows && seen.is_positive() && !to.expand(0.01).contains_rect(seen) {
        CUTS.set(CUTS.get() + 1);
    }
}

/// How many shapes an element's `overflow` clip has cut on this thread since the last call, which
/// starts the count again: for tests that check a view paints only inside the regions it is in.
/// Always 0 in a release build.
#[doc(hidden)]
pub fn overflow_cuts() -> usize {
    #[cfg(debug_assertions)]
    return CUTS.replace(0);
    #[cfg(not(debug_assertions))]
    0
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
    /// gradients, its inset shadows and its border. Its contents go in [`Boxed::content`], the box
    /// less its border and `padding`. When it leaves scope, what its contents painted in the
    /// painter's layer is clipped to its padding box if it hides its overflow, and its outline is
    /// painted over them. Its [`Native`] fallbacks stand where the sheet sets none.
    pub fn paint(&self, painter: &Painter, rect: Rect) -> Boxed {
        self.paint_with(painter, rect, self.native)
    }

    /// [`Entered::paint`] with `native` as the fallbacks, for a fill worked out once the element is
    /// in scope (its hover).
    pub fn paint_with(&self, painter: &Painter, rect: Rect, native: Native) -> Boxed {
        self.finish();
        let style = self.style();
        let outer = super::native_css::outer_clip(self.scope.node);
        let radius = super::native_css::box_begin(painter, rect, native.radius, native.fill, style.as_ref(), outer);
        let content = self.contain(painter, rect, style, radius, outer);
        Boxed { content, radius }
    }

    /// [`Entered::paint_with`] with `native.fill` in place of the sheet's `background-color`, and
    /// `image` (0 to 1) of its background image: for a toggle whose caller worked out its fill
    /// from the element already ([`super::widgets::Toggle`]), so neither the colour nor the image
    /// of a rule for every such control covers the lit state's fill.
    pub fn paint_over(&self, painter: &Painter, rect: Rect, native: Native, image: f32) -> Boxed {
        self.finish();
        let style = self.style();
        let mut own = style.clone();
        if let Some(own) = own.as_mut() {
            own.authored.background = false;
            own.image = image.clamp(0.0, 1.0);
        }
        let outer = super::native_css::outer_clip(self.scope.node);
        let radius = super::native_css::box_begin(painter, rect, native.radius, native.fill, own.as_ref(), outer);
        let content = self.contain(painter, rect, style, radius, outer);
        Boxed { content, radius }
    }

    /// The element's border in `rect`, for a bar whose background its caller paints with the
    /// window's corners (and [`Entered::paint_background`] its gradients and inset shadows): then,
    /// as after [`Entered::paint`], its contents go in the content box returned, are clipped to
    /// its padding box when it hides its overflow, and its outline comes when it leaves scope. The
    /// border follows the sheet's `border-radius`, as [`Entered::paint_background`]'s gradients and
    /// shadows do, and is square where the sheet sets none (#1487).
    pub fn paint_border(&self, painter: &Painter, rect: Rect) -> Rect {
        self.finish();
        let style = self.style();
        let radius = style
            .as_ref()
            .filter(|style| style.authored.radius)
            .map_or(0.0, |style| style.radius.min(rect.width().min(rect.height()) / 2.0).max(0.0));
        if let Some(style) = &style {
            super::native_css::border(painter, rect, radius, style);
        }
        let outer = super::native_css::outer_clip(self.scope.node);
        self.contain(painter, rect, style, radius, outer)
    }

    /// The element's content box in `rect`; and from here on, until it is dropped, its box
    /// clips what is painted in `painter`'s layer when it hides its overflow.
    fn contain(
        &self,
        painter: &Painter,
        rect: Rect,
        style: Option<Computed>,
        radius: f32,
        outer: Option<Rect>,
    ) -> Rect {
        let Some(style) = style else {
            super::native_css::set_box(self.scope.node, rect, painter.clip_rect());
            return rect;
        };
        let [top, right, bottom, left] = style.border_sides;
        let padding_box = shrink(rect, [top, right, bottom, left]);
        super::native_css::set_box(self.scope.node, padding_box, painter.clip_rect());
        let content = shrink(rect, style.insets_in(rect.width()));
        let clip = style.hides_overflow().then(|| {
            let from = painter.ctx().graphics_mut(|layers| layers.entry(painter.layer_id()).next_idx());
            (from, padding_box)
        });
        if clip.is_some() || style.outline().is_some() {
            *self.rest.borrow_mut() = Some(Rest { painter: painter.clone(), rect, radius, style, outer, clip });
        }
        content
    }

    /// Finishes a box this element painted before, if it did: a box at a time.
    fn finish(&self) {
        let rest = self.rest.borrow_mut().take();
        if let Some(rest) = rest {
            rest.paint();
        }
    }

    /// Closes the box painted last before the element leaves scope: what its contents painted so
    /// far is clipped, if it hides its overflow, and its outline is painted. Paint after this is
    /// not clipped by it: for an element whose scope holds something outside its box (the detail
    /// area's devices column, over the view above it).
    pub fn close_box(&self) {
        self.finish();
    }

    /// Where input may reach inside `rect`, the element's box: its padding box when it hides its
    /// overflow (what its contents paint outside it is cut, and takes no click either), else
    /// everywhere.
    pub fn input_clip(&self, rect: Rect) -> Rect {
        match self.style() {
            Some(style) if style.hides_overflow() => shrink(rect, style.border_sides),
            _ => Rect::EVERYTHING,
        }
    }

    /// The element's content box in `rect`: `rect` less its border and `padding`, for an element
    /// painted in part (a header scrolled partly out of view) whose contents follow its whole box.
    pub fn content_in(&self, rect: Rect) -> Rect {
        shrink(rect, self.insets_in(rect.width()))
    }

    /// The room the element's border and `padding` take inside its box (top, right, bottom,
    /// left): what a box sized from its contents adds around them. Zero where no sheet styles it.
    pub fn insets(&self) -> [f32; 4] {
        self.style().map_or([0.0; 4], |style| style.insets())
    }

    /// [`Entered::insets`] for a box `width` wide, which a padding in percent is taken of
    /// (`toolbar`, `workspace`: [`super::native_css::Computed::padding_in`]).
    pub fn insets_in(&self, width: f32) -> [f32; 4] {
        self.style().map_or([0.0; 4], |style| style.insets_in(width))
    }

    /// The element's node in this frame's style tree, while it is entered.
    pub(crate) fn node(&self) -> Option<usize> {
        self.scope.node
    }

    /// [`Look::color`] of the element.
    pub fn color(&self) -> Color32 {
        self.look().color()
    }

    /// [`Look::background`] of the element.
    pub fn background(&self) -> Color32 {
        self.look().background()
    }

    /// [`Look::border`] of the element.
    pub fn border(&self) -> Color32 {
        self.look().border()
    }

    /// [`Look::accent`] of the element.
    pub fn accent(&self) -> Color32 {
        self.look().accent()
    }

    /// [`Look::color_or`] of the element.
    pub fn color_or(&self, fallback: Color32) -> Color32 {
        self.look().color_or(fallback)
    }

    /// [`Look::own_color_or`] of the element: a colour the app gives by state (an error, the shown
    /// tab) where no rule on the element gives one.
    pub fn own_color_or(&self, fallback: Color32) -> Color32 {
        self.look().own_color_or(fallback)
    }

    /// [`Look::computed_color_or`] of the element.
    pub fn computed_color_or(&self, fallback: Color32) -> Color32 {
        self.look().computed_color_or(fallback)
    }

    /// [`Look::computed_background_or`] of the element.
    pub fn computed_background_or(&self, fallback: Color32) -> Color32 {
        self.look().computed_background_or(fallback)
    }

    /// [`Look::background_or`] of the element.
    pub fn background_or(&self, fallback: Color32) -> Color32 {
        self.look().background_or(fallback)
    }

    /// [`Look::border_or`] of the element.
    pub fn border_or(&self, fallback: Color32) -> Color32 {
        self.look().border_or(fallback)
    }

    /// The [`Look`] of the element, for the colours of what it draws inside itself.
    pub fn look(&self) -> Look {
        Look(self.style())
    }

    /// The element's [`Look`] as if the pointer were over it (`hovered`) or not, whatever it is
    /// now: what a fade between its two states starts and ends at.
    pub fn look_hovered(&self, hovered: bool) -> Look {
        Look(super::native_css::probe_hovered(self.scope.node, hovered))
    }

    /// A colour of the element `amount` (0 to 1) of the way from its resting style to its hovered
    /// one (`:hover`), `read` out of each ([`Look::background`], [`Look::color`]): the fade an
    /// animated hover draws. The element's own style is one end where it is in that state, so a
    /// control at rest, or fully hovered, looks the other end up only while it fades.
    pub fn hover_fade(&self, amount: f32, read: impl Fn(&Look) -> Color32) -> Color32 {
        let amount = if amount.is_finite() { amount.clamp(0.0, 1.0) } else { 0.0 };
        let now = super::native_css::node_hovered(self.scope.node);
        let end = |hovered: bool| if hovered == now { self.look() } else { self.look_hovered(hovered) };
        if amount <= 0.0 {
            return read(&end(false));
        }
        if amount >= 1.0 {
            return read(&end(true));
        }
        super::motion::mix(read(&end(false)), read(&end(true)), amount)
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
        super::native_css::set_box(self.scope.node, rect, painter.clip_rect());
        self.style().is_some_and(|style| super::native_css::background_rounded(painter, rect, radius, style))
    }

    /// The element's gradients and inset shadows without its outer shadows: for a surface whose
    /// artwork casts them itself ([`Entered::artwork_shadows`]).
    pub fn paint_gradients(&self, painter: &Painter, rect: Rect, radius: f32) -> bool {
        self.paint_gradients_with(painter, rect, radius, 1.0)
    }

    /// [`Entered::paint_gradients`] with `image` (0 to 1) of its gradients ([`Entered::paint_over`]).
    pub fn paint_gradients_with(&self, painter: &Painter, rect: Rect, radius: f32, image: f32) -> bool {
        self.style().is_some_and(|mut style| {
            style.image = image.clamp(0.0, 1.0);
            super::native_css::gradients_rounded(painter, rect, radius, style)
        })
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

/// `rect` less `insets` (top, right, bottom, left); an empty rectangle at its centre where they
/// leave no room.
fn shrink(rect: Rect, insets: [f32; 4]) -> Rect {
    let inner = super::native_css::row::inset(rect, insets);
    if inner.is_positive() { inner } else { Rect::from_center_size(inner.center(), egui::Vec2::ZERO) }
}

/// A line between a view's rows or parts: `separator[kind="row"]`'s `color`.
pub(crate) fn row_line() -> Color32 {
    SEPARATOR.attribute("kind", "row").look().color()
}

/// Dim words (a device's kind, an empty area's note, a heading's rule): `label[kind="hint"]`'s
/// `color`. Its own, not inherited: a colour a theme gives a whole view keeps them dimmer than the
/// text beside them.
pub(crate) fn hint_ink() -> Color32 {
    LABEL.attribute("kind", "hint").look().color()
}

/// A control's name over it, a figure's name: `label[kind="caption"]`'s `color`. Its own, as a
/// hint's: a colour a theme gives the whole view keeps it apart from the values.
pub(crate) fn caption_ink() -> Color32 {
    LABEL.attribute("kind", "caption").look().color()
}

/// A panel's title, such as a clip's name or a dialog's: `label[kind="title"]`'s `color`.
pub(crate) fn title_ink() -> Color32 {
    LABEL.attribute("kind", "title").look().color()
}

/// A figure or a value shown as words, not a field: `label[kind="value"]`'s `color`.
pub(crate) fn value_ink() -> Color32 {
    LABEL.attribute("kind", "value").look().color()
}

/// Words or a mark in the accent (a file found, a key being set, the toolbar editor's grid):
/// `label[kind="accent"]`'s `color`.
pub(crate) fn accent_ink() -> Color32 {
    LABEL.attribute("kind", "accent").look().color()
}

/// Words that say something went wrong: `label[kind="error"]`'s `color`.
pub(crate) fn error_ink() -> Color32 {
    LABEL.attribute("kind", "error").look().color()
}

/// A name or a line of text: `label`'s `color`.
pub(crate) fn name_ink() -> Color32 {
    LABEL.look().color()
}

/// The fill the app's bars, Settings, dialogs and menus paint for themselves, of `kind`
/// (`window`, `panel`, `panel-hover`, `control`, `control-hover`, `control-active`): the
/// `background-color` of `app-fill[kind]`, read where it is painted, so a region's variables reach
/// it.
pub(crate) fn app_fill(kind: &str) -> Color32 {
    APP_FILL.attribute("kind", kind).look().background()
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
    PROJECT_TAB = "project-tab";
    TOOLBAR = "toolbar";
    TOOLBAR_MODULE = "toolbar-module";
    GROUP = "group";
    FLEXIBLE_SPACE = "flexible-space";
    BAR_ITEM = "bar-item";
    STATUS_BAR = "status-bar";
    NOTICE_BAR = "notice-bar";
    WORKSPACE_PANEL = "workspace-panel";
    DIVIDER = "divider";
    BROWSER = "browser";
    BROWSER_PLACE = "browser-place";
    BROWSER_ROW = "browser-row";
    BROWSER_PLAY = "browser-play";
    BROWSER_ICON = "browser-icon";
    BROWSER_TAG_DOT = "browser-tag-dot";
    BROWSER_PREVIEW_WAVE = "browser-preview-wave";
    BROWSER_FILL = "browser-fill";
    SESSION = "session";
    TRACK_COLUMN = "track-column";
    SCENE_COLUMN = "scene-column";
    SCENE = "scene";
    SLOT = "slot";
    MAPPING_OVERLAY = "mapping-overlay";
    SLOT_RECORD = "slot-record";
    SLOT_MARK = "slot-mark";
    DROP_TARGET = "drop-target";
    ARRANGEMENT = "arrangement";
    TRACK_HEADER = "track-header";
    TRACK_NAME = "track-name";
    TRACK_NUMBER = "track-number";
    TRACK_COLOUR = "track-colour";
    FOLD = "fold";
    ADD_TRACK = "add-track";
    TRACK_LANE = "track-lane";
    CLIP = "clip";
    CLIP_NAME = "clip-name";
    CLIP_WAVEFORM = "clip-waveform";
    CLIP_NOTES = "clip-notes";
    CLIP_ENVELOPE = "clip-envelope";
    CLIP_GAIN = "clip-gain";
    CLIP_FADE_HANDLE = "clip-fade-handle";
    CLIP_FADE = "clip-fade";
    TAKE_HEADER = "take-header";
    RULER = "ruler";
    RULER_NUMBER = "ruler-number";
    RULER_TICK = "ruler-tick";
    TEMPO_MARK = "tempo-mark";
    GRID_LINE = "grid-line";
    TIME_SELECTION = "time-selection";
    MARQUEE = "marquee";
    START_MARKER = "start-marker";
    LOOP_REGION = "loop-region";
    LOOP_LINE = "loop-line";
    AUTOMATION_HEADER = "automation-header";
    AUTOMATION_LANE = "automation-lane";
    AUTOMATION_LINE = "automation-line";
    AUTOMATION_POINT = "automation-point";
    AUTOMATION_PIN = "automation-pin";
    MARKER = "marker";
    ARRANGEMENT_TOOL = "arrangement-tool";
    PLAYHEAD = "playhead";
    MIXER = "mixer";
    DEVICE_LIST = "device-list";
    MIXER_TOGGLES = "mixer-toggles";
    DEVICE_ROW = "device-row";
    DETAIL = "detail";
    DETAIL_FILL = "detail-fill";
    CLIP_EDITOR = "clip-editor";
    EDITOR_TAB = "editor-tab";
    PIANO_ROLL = "piano-roll";
    PIANO_ROLL_TOOLBAR = "piano-roll-toolbar";
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
    PIANO_SLICE = "piano-slice";
    PIANO_TOOL = "piano-tool";
    MIDI_TOOLS = "midi-tools";
    TAB_ROW = "tab-row";
    CLIP_INFO = "clip-info";
    SAMPLE_MAP = "sample-map";
    SAMPLE_POINT = "sample-point";
    DEVICE_PANEL = "device-panel";
    DEVICE = "device";
    DEVICE_CANVAS = "device-canvas";
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
    TOGGLE = "toggle";
    VALUE = "value";
    VALUE_FILL = "value-fill";
    VALUE_HANDLE = "value-handle";
    PAN = "pan";
    PAN_CENTRE = "pan-centre";
    FADER = "fader";
    FADER_CAP = "fader-cap";
    FADER_RAIL = "fader-rail";
    FADER_SCALE = "fader-scale";
    SEND = "send";
    KNOB = "knob";
    KNOB_BODY = "knob-body";
    KNOB_POINTER = "knob-pointer";
    KNOB_RING = "knob-ring";
    ICON = "icon";
    METER = "meter";
    METER_LEVEL = "meter-level";
    METER_CLIP = "meter-clip";
    METER_VERTICAL = "meter-vertical";
    METER_HORIZONTAL = "meter-horizontal";
    DROPDOWN = "dropdown";
    DROPDOWN_ARROW = "dropdown-arrow";
    TEXT_INPUT = "text-input";
    SCROLL_BAR = "scroll-bar";
    LABEL = "label";
    SEPARATOR = "separator";
    POPUP = "popup";
    TOOLTIP = "tooltip";
    PROGRESS = "progress";
    PROGRESS_FILL = "progress-fill";
    SETTINGS = "settings";
    SETTINGS_NAV = "settings-nav";
    DIALOG = "dialog";
    DIALOG_TITLE = "dialog-title";
    COLOUR_SWATCH = "colour-swatch";
    LIST_ROW = "list-row";
    START_PAGE = "start-page";
    PLUGIN_MANAGER = "plugin-manager";
    EXPORT = "export";
    THEME_CARD = "theme-card";
    THEME_SWATCH = "theme-swatch";
    APP_FILL = "app-fill";
    WIDGET = "widget";
}

#[cfg(test)]
#[path = "element_tests.rs"]
mod tests;
