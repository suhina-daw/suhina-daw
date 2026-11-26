// Copyright (C) 2026 The Suhina contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! Small drawing helpers and the controls the views share: text that fits a cell, the value
//! field drag, the knob.

use std::sync::Arc;

use egui::text::{LayoutJob, TextWrapping};
use egui::{
    Align2, Color32, FontId, Galley, Id, Painter, Pos2, Rect, Response, Role, Sense, Stroke, Ui, Vec2, WidgetInfo,
};
use serde_json::Value;

use crate::kit::element::{
    Armed, BUTTON, Checked, DROPDOWN, DROPDOWN_ARROW, Entered, KNOB, Look, Paint, SLIDER, SLIDER_FILL, SLIDER_HANDLE,
    SLIDER_RAIL, Sides, TEXT_INPUT, TOGGLE, VALUE,
};
use crate::kit::{motion, theme};

/// The arrow of a row that opens: pointing right while closed, down while open.
pub fn chevron(painter: &Painter, center: Pos2, open: bool, color: Color32) {
    let points = if open {
        vec![center + Vec2::new(-3.0, -1.5), center + Vec2::new(0.0, 1.5), center + Vec2::new(3.0, -1.5)]
    } else {
        vec![center + Vec2::new(-1.5, -3.0), center + Vec2::new(1.5, 0.0), center + Vec2::new(-1.5, 3.0)]
    };
    painter.add(egui::Shape::line(points, Stroke::new(1.1, color)));
}

/// Where the pointer is, when it is over `rect` and no layer is on top of `ui`'s there (a
/// dialog, a menu, a tooltip): what a view that reads the wheel, a pinch or a press itself may
/// act on, so they reach only the dialog while one is open.
pub fn pointer_over(ui: &Ui, rect: Rect) -> Option<Pos2> {
    if ui.rect_contains_pointer(rect) { ui.ctx().pointer_hover_pos() } else { None }
}

/// Normalized device parameter range traversed by a vertical drag.
pub(crate) const PARAMETER_DRAG_PIXELS: f64 = 400.;

/// `value` to `decimals` places, without the minus sign of a negative that rounds to zero.
pub(crate) fn fixed(value: f64, decimals: usize) -> String {
    let text = format!("{value:.decimals$}");
    match text.strip_prefix('-') {
        Some(rest) if !rest.bytes().any(|b| matches!(b, b'1'..=b'9')) => rest.to_string(),
        _ => text,
    }
}

/// A value drag in progress, while it holds the pointer. `raw` is set once the device has sent raw
/// motion: from then on only raw motion moves the value, and the pointer is locked.
#[derive(Clone, Copy)]
struct ValueDrag {
    raw: bool,
    /// Where the pointer goes on release, for a control that moves under it (a fader's cap).
    back: Option<Pos2>,
    /// Let go of: the pointer has been moved back and shows at the next frame, so it never flashes
    /// where it was held.
    released: bool,
    /// The pointer is hidden while the drag holds it ([`set_drag_hides_pointer`],
    /// [`value_drag_pointer`]); otherwise it shows `icon`.
    hides: bool,
    /// What the pointer shows while the drag holds it: nothing while it hides, else the arrow of
    /// the drag's axis ([`value_drag_shows`]).
    icon: egui::CursorIcon,
}

fn value_drag_id() -> Id {
    Id::unique("value-drag-holds-pointer")
}

fn drag_hides_pointer_id() -> Id {
    Id::unique("value-drag-hides-pointer")
}

/// Where a control's own answer to whether its next drag hides the pointer waits
/// ([`value_drag_pointer`]).
fn drag_pointer_id(response: &Response) -> Id {
    response.id.with("drag-pointer")
}

/// Settings › Appearance › Hide pointer while dragging: whether a value drag hides the pointer
/// (on by default) where its control's element says nothing of its own.
pub fn set_drag_hides_pointer(ctx: &egui::Context, on: bool) {
    ctx.data_mut(|d| d.insert_temp(drag_hides_pointer_id(), on));
}

/// The setting [`set_drag_hides_pointer`] gave; on unless turned off.
pub fn drag_hides_pointer(ctx: &egui::Context) -> bool {
    ctx.data(|d| d.get_temp(drag_hides_pointer_id())).unwrap_or(true)
}

/// Gives a drag that `response` starts the pointer `el` asks for: the sheet's `drag-pointer` on
/// it, else the setting. Called each frame by a control that has an element, before or after its
/// drag reads its movement: a drag starts with what its control said in the frame before, as the
/// pointer is over it before it is pressed. A control that never says follows the setting.
pub fn value_drag_pointer(ctx: &egui::Context, response: &Response, el: &crate::kit::element::Entered) {
    value_drag_hides(ctx, response, crate::kit::style_class::drag_hides_pointer(el, drag_hides_pointer(ctx)));
}

/// [`value_drag_pointer`] for a control that worked out its element's answer itself.
pub fn value_drag_hides(ctx: &egui::Context, response: &Response, hides: bool) {
    ctx.data_mut(|d| d.insert_temp(drag_pointer_id(response), hides));
}

/// How far a drag on a value control (knob, value field) moved this frame, called while it is
/// dragged. The pointer hides while the drag lasts, unless the setting or the control's element
/// shows it ([`value_drag_pointer`]): then it keeps its icon, or the arrow of the drag's axis
/// ([`value_drag_shows`]). A mouse sends raw motion, which is read and
/// the pointer locked, so a long drag never stops at the screen's edge (X11 refuses the lock, and
/// the pointer moves on hidden). A device with no relative motion (a pen in absolute mode, a remote
/// session) sends none, so it keeps the pointer's own movement, which a lock would stop.
/// [`release_value_drag`] gives the pointer back. Only a drag's first frame takes the pointer, in a
/// window with focus; a drag whose release went to another window moves nothing when focus comes
/// back, until the next press.
pub fn value_drag_delta(ui: &Ui, response: &Response) -> Vec2 {
    let ctx = ui.ctx();
    let mut drag = ctx.data(|d| d.get_temp::<ValueDrag>(value_drag_id()));
    if drag.is_none() && response.drag_started() && ctx.input(|i| i.focused) {
        let hides =
            ctx.data(|d| d.get_temp::<bool>(drag_pointer_id(response))).unwrap_or_else(|| drag_hides_pointer(ctx));
        let icon = if hides {
            ctx.send_viewport_cmd(egui::ViewportCommand::CursorVisible(false));
            egui::CursorIcon::None
        } else {
            // The pointer the control shows, until the drag gives it an arrow of its own.
            ctx.output(|o| o.cursor_icon)
        };
        drag = Some(ValueDrag { raw: false, back: None, released: false, hides, icon });
    }
    let Some(mut drag) = drag.filter(|d| !d.released) else {
        return Vec2::ZERO;
    };
    // egui shows the pointer again whenever the icon changes, so the icon stays hidden too, unless
    // the drag shows one of its own.
    ctx.set_cursor_icon(drag.icon);
    let (motion, per_point) = ui.input(|i| (i.pointer.motion(), i.pixels_per_point));
    if motion.is_some() && !drag.raw {
        ctx.send_viewport_cmd(egui::ViewportCommand::CursorGrab(egui::CursorGrab::Locked));
        drag.raw = true;
    }
    ctx.data_mut(|d| d.insert_temp(value_drag_id(), drag));
    if drag.raw && response.drag_started() {
        // This frame's raw motion may be from before the press.
        Vec2::ZERO
    } else if drag.raw {
        // Raw motion is in the mouse's counts, not in points: scaled like the pointer on a display.
        motion.unwrap_or(Vec2::ZERO) / per_point
    } else {
        response.drag_delta()
    }
}

/// Gives the pointer back once the buttons are up after a value drag, or once the window loses
/// focus (egui keeps the button down when the release went to another window). Called once a
/// frame, so a control that went away mid-drag (hidden, its panel closed) cannot keep the pointer.
pub fn release_value_drag(ctx: &egui::Context) {
    let Some(drag) = ctx.data(|d| d.get_temp::<ValueDrag>(value_drag_id())) else {
        return;
    };
    if drag.released {
        // The next icon shows the pointer: nothing else may, as the window applies a command after
        // the frame's icon, which would show it over a view that keeps it hidden for a shape of its
        // own (a magnifier).
        ctx.data_mut(|d| d.remove::<ValueDrag>(value_drag_id()));
    } else if !ctx.input(|i| i.pointer.any_down() && i.focused) {
        // Only in a window with focus: a pointer gone to another window stays there. Moved while
        // the drag still locks it, as Wayland moves a pointer only while it is locked.
        if let Some(at) = drag.back.filter(|_| ctx.input(|i| i.focused)) {
            ctx.send_viewport_cmd(egui::ViewportCommand::CursorPosition(at));
        }
        ctx.send_viewport_cmd(egui::ViewportCommand::CursorGrab(egui::CursorGrab::None));
        ctx.data_mut(|d| d.insert_temp(value_drag_id(), ValueDrag { released: true, ..drag }));
        ctx.request_repaint();
    }
}

/// Where the pointer comes back when this value drag ends.
pub fn value_drag_returns_to(ctx: &egui::Context, at: Pos2) {
    if let Some(drag) = ctx.data(|d| d.get_temp::<ValueDrag>(value_drag_id())) {
        ctx.data_mut(|d| d.insert_temp(value_drag_id(), ValueDrag { back: Some(at), ..drag }));
    }
}

/// The pointer shows `icon` for the rest of this value drag, unless the drag hides it: where it
/// was pressed, as the drag holds it there.
pub fn value_drag_shows(ctx: &egui::Context, icon: egui::CursorIcon) {
    if let Some(drag) = ctx.data(|d| d.get_temp::<ValueDrag>(value_drag_id())).filter(|d| !d.hides) {
        ctx.data_mut(|d| d.insert_temp(value_drag_id(), ValueDrag { icon, ..drag }));
        ctx.set_cursor_icon(icon);
    }
}

/// Keeps the pointer hidden (or showing the drag's icon) while a value drag holds it, called
/// after the views: the last icon set in a frame wins, and egui shows the pointer again for any
/// other icon.
pub fn hide_pointer_during_value_drag(ctx: &egui::Context) {
    if let Some(drag) = ctx.data(|d| d.get_temp::<ValueDrag>(value_drag_id())) {
        ctx.set_cursor_icon(drag.icon);
    }
}

/// A command for the session to run, made by a view: the same ids the CLI and MCP use.
#[derive(Debug, Clone, PartialEq)]
pub struct Action {
    pub id: &'static str,
    pub params: Value,
}

impl Action {
    pub fn new(id: &'static str, params: Value) -> Self {
        Self { id, params }
    }

    /// An action for a command named at run time (from the keymap); unknown ids run nothing.
    pub fn new_owned(id: &str, params: Value) -> Self {
        let known = suhina_session::commands()
            .iter()
            .map(|c| c.id)
            .chain(suhina_session::project_commands().iter().map(|c| c.id))
            .find(|known| *known == id)
            .unwrap_or("command.list");
        Self { id: known, params }
    }
}

/// One line of text cut to `max_width` with an ellipsis.
pub fn fitted(painter: &Painter, text: &str, font: FontId, color: Color32, max_width: f32) -> Arc<Galley> {
    let mut job = LayoutJob::simple_singleline(text.to_string(), font, color);
    job.wrap = TextWrapping::truncate_at_width(max_width.max(0.0));
    painter.layout_job(job)
}

/// Whether `galley`, laid out by [`fitted`] for `width`, is worth drawing there: it fits, and a
/// cut kept at least one letter before the ellipsis.
fn fits(galley: &Galley, width: f32) -> bool {
    let glyphs = galley.rows.first().map_or(0, |row| row.row.glyphs.len());
    galley.size().x <= width.max(0.0) + 0.5 && !(galley.elided && glyphs <= 1)
}

/// How wide `text` is in `font`.
pub fn text_width(ui: &Ui, text: &str, font: FontId) -> f32 {
    ui.fonts_mut(|f| f.layout_no_wrap(text.to_string(), font, Color32::WHITE).size().x)
}

/// Draws one line of text in `rect`, cut to its width, aligned horizontally by `align` and
/// centered vertically. Where not even one letter and the ellipsis fit, nothing is drawn: a cut
/// never leaves a lone ellipsis, or one wider than `rect` (#1198).
pub fn text_in(painter: &Painter, rect: Rect, align: Align2, text: &str, font: FontId, color: Color32) {
    let galley = fitted(painter, text, font, color, rect.width());
    if !fits(&galley, rect.width()) {
        return;
    }
    let pos = align.align_size_within_rect(galley.size(), rect).min;
    paint_text(painter, pos, galley, color);
}

pub fn paint_text(painter: &Painter, pos: Pos2, galley: Arc<Galley>, color: Color32) {
    super::native_css::paint_text(painter, pos, galley, color);
}

/// Makes `rect` clickable, with a label for screen readers and tests.
pub fn button_area(ui: &Ui, rect: Rect, id: egui::Id, label: impl ToString) -> Response {
    let response = ui.interact(rect, id, Sense::click());
    let label = label.to_string();
    response.widget_info(|| WidgetInfo::labeled(Role::Button, true, &label));
    response
}

/// How hovered (or dragged) a control is, 0 … 1, fading in and out.
pub fn hover(ui: &Ui, response: &Response) -> f32 {
    motion::toward(ui.ctx(), response.id.with("hover"), response.hovered() || response.dragged(), motion::HOVER)
}

/// Shared track-button surface. Colors and focus outlines follow the active theme.
/// Geometry stays inside the caller's hit target, including compact mixer strips.
/// `el` is the button its caller entered (`button#solo`).
pub fn track_button_face(painter: &Painter, rect: Rect, el: &Entered, response: &Response, fill: Color32) {
    let radius = el.radius_or((rect.height() * 0.25).min(5.0));
    surface(painter, rect, el, response, false, fill, radius, None);
    focus_ring(painter, rect, el, response, radius);
}

/// [`track_button_face`] for a toggle whose `fill` its caller worked out from `el` already
/// ([`Toggle`]): the fill is painted as it is, with `image` (0 to 1, [`Toggle::image`]) of the
/// sheet's background image, so neither the color nor the image of a rule for every `button`
/// covers the lit one's.
pub fn toggle_face(painter: &Painter, rect: Rect, el: &Entered, response: &Response, fill: Color32, image: f32) {
    let radius = el.radius_or((rect.height() * 0.25).min(5.0));
    surface(painter, rect, el, response, false, fill, radius, Some(image));
    focus_ring(painter, rect, el, response, radius);
}

/// [`toggle_face`] of a toggle that has no ring under the pointer and whose radius is `radius`
/// unless the sheet gives one (a chip, a view switch): the same fill and image as the lit one's.
pub fn toggle_surface(
    painter: &Painter,
    rect: Rect,
    el: &Entered,
    response: &Response,
    fill: Color32,
    image: f32,
    radius: f32,
) {
    surface(painter, rect, el, response, false, fill, radius, Some(image));
}

/// The ring of a track button under the pointer, in its `border-color`, and while it has the keys,
/// in its `accent-color`.
fn focus_ring(painter: &Painter, rect: Rect, el: &Entered, response: &Response, radius: f32) {
    if response.has_focus() || response.hovered() {
        let edge = if response.has_focus() { el.accent() } else { el.border() };
        painter.rect_stroke(rect, radius, Stroke::new(1.0, edge), egui::StrokeKind::Inside);
    }
}

/// CSS paint remains above the SVG body, below the control's glyph and label. `el` is the button
/// its caller entered, with `[checked]` when `on`.
pub fn button_surface(
    painter: &Painter,
    rect: Rect,
    el: &Entered,
    response: &Response,
    on: bool,
    fill: Color32,
    radius: f32,
) {
    surface(painter, rect, el, response, on, fill, radius, None);
}

/// [`button_surface`]; with `decided`, `fill` is the button's whole fill, its caller's from the
/// element already, painted over the sheet's `background-color`, with that much (0 to 1) of the
/// sheet's background image.
#[allow(clippy::too_many_arguments)]
fn surface(
    painter: &Painter,
    rect: Rect,
    el: &Entered,
    response: &Response,
    on: bool,
    fill: Color32,
    radius: f32,
    decided: Option<f32>,
) {
    let style = el.style();
    // A shape the sheet gives the button wins over the artwork Suhina ships, not over the theme's own.
    let shaped = style
        .as_ref()
        .is_some_and(|style| style.authored.background || style.authored.radius || style.authored.border_width);
    let role = super::skin::button_role(el);
    if !shaped || role != "button" || super::skin::has_own("button") {
        let fill = if decided.is_some() { fill } else { el.background_or(fill) };
        let radius = el.radius_or(radius);
        if super::skin::button(painter, rect, el, super::skin::State::response(response, on), fill, radius) {
            // The artwork cast the shadows already, in its own shape.
            el.paint_gradients_with(painter, rect, radius, decided.unwrap_or(1.0));
            return;
        }
    }
    let native = super::element::Native { radius, fill };
    match decided {
        Some(image) => el.paint_over(painter, rect, native, image),
        None => el.paint_with(painter, rect, native),
    };
}

/// The fill and the ink of a toggle drawn as `el` (a `button` entered with `[checked]` while on),
/// lit or not, with no fade: [`Toggle`] with `fills` and `inks` (on, off) as its tokens and the
/// unlit ink its own ([`UnlitInk::Own`]). `plain` is the same button without `[checked]`, looked
/// up before `el` is entered and given only while it is on
/// ([`super::element::Builder::enter_against_plain`]).
pub fn toggle_inks(
    el: &Entered,
    plain: Option<&super::element::Look>,
    fills: (Color32, Color32),
    inks: (Color32, Color32),
) -> (Color32, Color32) {
    let sides = plain.map(|plain| Sides { lit: el.look(), plain: plain.clone(), entered_lit: true });
    let toggle = Toggle::of(el, sides.as_ref(), UnlitInk::Own, (fills.1, inks.1), fills.0, |_| inks.0);
    if plain.is_some() { toggle.on } else { toggle.off }
}

/// Which color a toggle's unlit ink takes from the sheet.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum UnlitInk {
    /// Only a `color` a rule gives the button itself ([`super::element::Look::own_color_or`]): a
    /// glyph or a mark, which the text color of a view around it does not reach.
    Own,
    /// Its text color, inherited too ([`super::element::Look::color_or`]): a letter or a number
    /// on the button, which reads in the color the scoped palette gave it before.
    Inherited,
}

/// A toggle's fill and ink while off and while on, which [`Toggle::at`] fades between.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Toggle {
    /// The fill and the ink while off.
    pub off: (Color32, Color32),
    /// The fill and the ink while on.
    pub on: (Color32, Color32),
    /// Whether the lit button shown has a background image of its own, a lit state's rule's.
    pub own_image: bool,
}

impl Toggle {
    /// The colors of a toggle drawn as `el` with its two [`Sides`], lit and plain, from
    /// [`super::element::Builder::enter_toggle`] (given while it is lit at all, fading out
    /// included, so its fade runs between the same two colors both ways). Off, it takes the plain
    /// button's `background-color` where the sheet gives one, else `off.0`, and its ink as `unlit`
    /// says, else `off.1`. On, only a rule for the lit state counts (`button[checked]`), so a rule
    /// for every `button` does not hide which one is on: the lit fill where it differs from the
    /// plain button's, else `on`; the lit ink where it differs, else `ink_on` of the fill. With no
    /// sides (off, not fading), the lit side is `on` and its ink.
    ///
    /// A hovered lit button where a `button:hover` rule and a `button[checked]` rule are equally
    /// specific takes the hover rule's fill on both sides, which then counts as no lit color: the
    /// token's stands.
    pub fn of(
        el: &Entered,
        sides: Option<&Sides>,
        unlit: UnlitInk,
        off: (Color32, Color32),
        on: Color32,
        ink_on: impl Fn(Color32) -> Color32,
    ) -> Self {
        let own = el.look();
        let plain = sides.map_or(&own, |sides| &sides.plain);
        let ink = match unlit {
            UnlitInk::Own => plain.own_color_or(off.1),
            UnlitInk::Inherited => plain.color_or(off.1),
        };
        let off = (plain.background_or(off.0), ink);
        match sides {
            Some(sides) => {
                let fill = sides.lit.unlike(&sides.plain, Paint::Background, on);
                let ink = sides.lit.unlike(&sides.plain, Paint::OwnColor, ink_on(fill));
                Self { off, on: (fill, ink), own_image: sides.entered_lit && sides.lit.image_unlike(&sides.plain) }
            }
            None => Self { off, on: (on, ink_on(on)), own_image: false },
        }
    }

    /// The colors of `el`'s own style, off and on alike: a toggle at rest.
    fn flat(el: &Entered) -> Self {
        let colors = (el.background(), el.color());
        Self { off: colors, on: colors, own_image: false }
    }

    /// The fill and the ink `lit` of the way on (0 off, 1 on).
    pub fn at(&self, lit: f32) -> (Color32, Color32) {
        (motion::mix(self.off.0, self.on.0, lit), motion::mix(self.off.1, self.on.1, lit))
    }

    /// How much of the sheet's background image shows on the button `lit` of the way on
    /// ([`toggle_face`]): a lit state's own image all of it; the plain button's image fades out as
    /// the lit fill fades in, so it does not cover that fill.
    pub fn image(&self, lit: f32) -> f32 {
        if self.own_image { 1.0 } else { 1.0 - lit.clamp(0.0, 1.0) }
    }
}

/// A track's number, `track-number`: its own `color`, `font-size` and `display: none`, and
/// `[muted]` while the track is off.
pub fn track_number_text(painter: &Painter, rect: Rect, number: &str, ink: Color32, muted: bool) {
    let el = super::element::TRACK_NUMBER.attr(super::element::Muted, muted).enter();
    let computed = el.style();
    if computed.as_ref().is_some_and(|c| !c.visible) {
        return;
    }
    let size = computed.as_ref().filter(|c| c.authored.font_size).map_or(theme::TEXT_SMALL, |c| c.font_size);
    let ink = el.color_or(ink);
    text_in(painter, rect, Align2::CENTER_CENTER, number, theme::mono_font(size), ink);
}

/// Where a knob's value goes, from `value-position` on the control or an element it is in:
/// `right` (beside it, the default), `left`, `top`, `bottom`, `inside` (over its middle) or `none`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ValuePosition {
    Right,
    Left,
    Top,
    Bottom,
    Inside,
    None,
}

impl ValuePosition {
    /// The position the sheet gives `el`, or `default`.
    pub fn of(el: &Entered, default: Self) -> Self {
        match el.style().and_then(|style| style.keyword(suhina_style::property::Property::ValuePosition)) {
            Some("right") => Self::Right,
            Some("left") => Self::Left,
            Some("top") => Self::Top,
            Some("bottom") => Self::Bottom,
            Some("inside" | "center") => Self::Inside,
            Some("none" | "hidden") => Self::None,
            _ => default,
        }
    }
}

/// The smallest knob drawn with its value above or below it.
const MIN_STACKED_KNOB: f32 = 10.0;

/// A knob in `rect` at `position` (0 to 1) with its value `text` where the sheet puts it
/// ([`ValuePosition`], `default` where it says nothing), in `ink` unless the sheet's `color` says
/// otherwise, at its `font-size` (`font_size` where it says none). `el` is the control its caller
/// entered (`pan`, `send`).
#[allow(clippy::too_many_arguments)]
pub fn knob_with_value(
    painter: &Painter,
    rect: Rect,
    el: &Entered,
    position: f32,
    pointer: Color32,
    response: Option<&Response>,
    text: &str,
    ink: Color32,
    default: ValuePosition,
    font_size: f32,
) {
    let size_text = el.font_size_or(font_size);
    let ink = el.color_or(ink);
    let line = size_text + 3.0;
    let place = match ValuePosition::of(el, default) {
        // A row too low for a knob with its value above or below it keeps the value beside it,
        // so the knob stays big enough to see and use (a pan in a track's header).
        ValuePosition::Top | ValuePosition::Bottom if rect.height() - line < MIN_STACKED_KNOB => ValuePosition::Right,
        place => place,
    };
    let text = &formatted_value(el, text);
    let (knob_area, text_area, align) = match place {
        ValuePosition::Right | ValuePosition::Left => {
            let size = rect.height().min(theme::KNOB);
            if place == ValuePosition::Right {
                let knob =
                    Rect::from_min_size(Pos2::new(rect.left() + 2.0, rect.center().y - size / 2.0), Vec2::splat(size));
                (knob, Rect::from_min_max(Pos2::new(knob.right() + 4.0, rect.top()), rect.max), Align2::RIGHT_CENTER)
            } else {
                let knob = Rect::from_min_size(
                    Pos2::new(rect.right() - 2.0 - size, rect.center().y - size / 2.0),
                    Vec2::splat(size),
                );
                (knob, Rect::from_min_max(rect.min, Pos2::new(knob.left() - 4.0, rect.bottom())), Align2::LEFT_CENTER)
            }
        }
        ValuePosition::Top | ValuePosition::Bottom => {
            let size = (rect.height() - line).min(rect.width()).max(4.0);
            let (knob_y, text_rect) = if place == ValuePosition::Top {
                (rect.bottom() - size, Rect::from_min_max(rect.min, Pos2::new(rect.right(), rect.top() + line)))
            } else {
                (rect.top(), Rect::from_min_max(Pos2::new(rect.left(), rect.bottom() - line), rect.max))
            };
            let knob = Rect::from_min_size(Pos2::new(rect.center().x - size / 2.0, knob_y), Vec2::splat(size));
            (knob, text_rect, Align2::CENTER_CENTER)
        }
        ValuePosition::Inside | ValuePosition::None => {
            let size = rect.height().min(rect.width());
            (Rect::from_center_size(rect.center(), Vec2::splat(size)), rect, Align2::CENTER_CENTER)
        }
    };
    knob(painter, knob_area.center(), knob_area.width(), position, pointer, response);
    if place != ValuePosition::None && text_area.is_positive() {
        text_in(painter, text_area, align, text, theme::mono_font(size_text), ink);
    }
}

/// Fill and text of a track-on toggle: light when on, dark when off, a step brighter hovered. The
/// colors of `toggle` in the sheet, as a toggle drawn as a button takes its own with
/// [`lit_toggle`].
pub fn toggle_colors(ui: &Ui, response: &Response, on: bool) -> (Color32, Color32) {
    let (hover, lit) = toggle_motion(ui, response, on);
    sheet_toggle(&TOGGLE.builder(), &[Checked], on || lit > 0.0, on, hover, Readable::Always).at(lit)
}

/// A toggle drawn as `builder` (a `button` with its id, say), entered with `attrs` while it is on
/// and its colors ([`lit_toggle`]): what a lit switch of a view takes, from the sheet's
/// `button[kind="toggle"]` and its `[checked]` state.
pub struct Lit {
    /// The entered element.
    pub el: Entered,
    /// The colors it fades between.
    pub toggle: Toggle,
    /// How far on it is, 0 to 1, fading.
    pub lit: f32,
}

impl Lit {
    /// The fill and the ink now.
    pub fn colors(&self) -> (Color32, Color32) {
        self.toggle.at(self.lit)
    }
}

/// [`Lit`] of the toggle `on` that `response` belongs to.
pub fn lit_toggle(
    ui: &Ui,
    response: &Response,
    builder: super::element::Builder<'_>,
    attrs: &[super::element::Attr],
    on: bool,
) -> Lit {
    lit_toggle_of(ui, response, builder, attrs, on, (Some("toggle"), Readable::Always))
}

/// [`lit_toggle`] of a toggle of its own `kind` (a `button[kind="record"]`), whose lit ink the sheet
/// gives and which stays the sheet's where only the sheet's fill is under it: the record and
/// metronome buttons of the control bar and the Launcher.
pub fn lit_toggle_kind(
    ui: &Ui,
    response: &Response,
    builder: super::element::Builder<'_>,
    attrs: &[super::element::Attr],
    on: bool,
    kind: &'static str,
) -> Lit {
    lit_toggle_of(ui, response, builder, attrs, on, (Some(kind), Readable::OnThemeFill))
}

/// [`lit_toggle_kind`] of a toggle with no `kind` of its own, whose states `attrs` (`[playing]`) the
/// sheet colors: a scene's launch button.
pub fn lit_toggle_state(
    ui: &Ui,
    response: &Response,
    builder: super::element::Builder<'_>,
    attrs: &[super::element::Attr],
    on: bool,
) -> Lit {
    lit_toggle_of(ui, response, builder, attrs, on, (None, Readable::OnThemeFill))
}

/// How a lit toggle's ink is kept readable on its fill: a rule's ink is its own, else the sheet's
/// ink is moved off a fill it does not read on.
#[derive(Clone, Copy, PartialEq, Eq)]
enum Readable {
    /// On every fill.
    Always,
    /// Only on a fill a theme's rule gave, the sheet's own pair being the design's.
    OnThemeFill,
}

/// [`Lit`] of a toggle of `kind`, or the one its builder gives it, whose ink is kept readable as
/// `keep` says.
fn lit_toggle_of(
    ui: &Ui,
    response: &Response,
    builder: super::element::Builder<'_>,
    attrs: &[super::element::Attr],
    on: bool,
    (kind, keep): (Option<&'static str>, Readable),
) -> Lit {
    let (hover, lit) = toggle_motion(ui, response, on);
    let builder = match kind {
        Some(kind) => builder.attribute("kind", kind),
        None => builder,
    };
    let fading = lit > 0.0;
    let touched = response.hovered() || response.dragged();
    // The sheet's colors are looked up (before the element is entered, as comparisons are) unless
    // the button is off, still and with no pointer: then the element's own style is all of it.
    let looked_up =
        (on || fading || hover > 0.0 || touched).then(|| sheet_toggle(&builder, attrs, on || fading, on, hover, keep));
    let el = attrs.iter().fold(builder.response(response), |builder, attr| builder.attr(*attr, on)).enter();
    let toggle = looked_up.unwrap_or_else(|| Toggle::flat(&el));
    Lit { el, toggle, lit }
}

/// A toggle's colors from the sheet: `builder`'s element (no response), with its states `attrs`
/// off and, when `lit_side`, on, each `hover` of the way from its style at rest to its style under
/// the pointer. Only a rule for the lit state lights it, so a theme that styles every `button`
/// states the lit one too. `entered_lit`: the button is on, not fading out.
fn sheet_toggle(
    builder: &super::element::Builder<'_>,
    attrs: &[super::element::Attr],
    lit_side: bool,
    entered_lit: bool,
    hover: f32,
    keep: Readable,
) -> Toggle {
    use super::element::Look;
    let ends = |on: bool| attrs.iter().fold(builder.clone(), |builder, attr| builder.attr(*attr, on)).hover_ends(hover);
    let at = |ends: &(Look, Look), read: fn(&Look) -> Color32| motion::mix(read(&ends.0), read(&ends.1), hover);
    let plain = ends(false);
    let off = (at(&plain, Look::background), at(&plain, Look::color));
    if !lit_side {
        return Toggle { off, on: off, own_image: false };
    }
    let lit = ends(true);
    let fill = at(&lit, Look::background);
    let ink = at(&lit, Look::color);
    let (themed_ink, themed_fill) =
        lit.0.style().map_or((false, false), |style| (style.authored.color, style.authored.background));
    let ink = if themed_ink || (keep == Readable::OnThemeFill && !themed_fill) { ink } else { readable(ink, fill) };
    Toggle { off, on: (fill, ink), own_image: entered_lit && lit.0.image_unlike(&plain.0) }
}

/// `ink` on `fill`, unless the fill leaves it unreadable: then the ink that reads on the fill.
fn readable(ink: Color32, fill: Color32) -> Color32 {
    if theme::text_contrast(ink, fill) < 3.0 { theme::text_on(fill) } else { ink }
}

/// How hovered a toggle is and how far on, each fading (0 to 1): worked out before its element is
/// entered, which while it is lit at all needs its colors looked up first.
pub fn toggle_motion(ui: &Ui, response: &Response, on: bool) -> (f32, f32) {
    (hover(ui, response), motion::toward(ui.ctx(), response.id.with("on"), on, motion::VALUE))
}

/// A track's solo button: draws it in `rect` and returns the command a click asks for. A click
/// solos the tracks named in `tracks` alone — one track, or the whole selection the clicked one
/// is part of; with Ctrl (Cmd) held they are added to the tracks already soloed.
pub fn solo_button(
    ui: &Ui,
    painter: &Painter,
    rect: Rect,
    id: egui::Id,
    tracks: &[u32],
    name: &str,
    solo: bool,
) -> Option<Action> {
    let label = if solo { format!("End solo on {name}") } else { format!("Solo {name}") };
    let response = button_area(ui, rect, id, label);
    // `button#solo`, `[checked]` while soloed: only a rule for that state lights it.
    let Lit { el, toggle, lit } =
        lit_toggle_of(ui, &response, BUTTON.id("solo"), &[Checked], solo, (Some("solo"), Readable::Always));
    let (fill, text) = toggle.at(lit);
    toggle_face(painter, rect, &el, &response, fill, toggle.image(lit));
    if !super::skin::paint(painter, rect, "solo", super::skin::State::response(&response, solo), text, 0.0) {
        text_in(painter, rect, Align2::CENTER_CENTER, "S", theme::ui_font(theme::TEXT_SMALL), text);
    }
    if !response.clicked() {
        return None;
    }
    let adding = ui.input(|i| i.modifiers.command);
    Some(Action::new(
        "track.setSolo",
        serde_json::json!({ "tracks": tracks, "solo": !solo, "exclusive": !solo && !adding }),
    ))
}

/// A track's cue button (headphones; a C read as part of the pan field beside it, which shows C
/// for center, #744): draws it in `rect` and returns the command a click asks for,
/// which turns the track's cue on or off, so it is also heard on the cue output (headphones).
pub fn cue_button(
    ui: &Ui,
    painter: &Painter,
    rect: Rect,
    id: egui::Id,
    track: u32,
    name: &str,
    cue: bool,
) -> Option<Action> {
    let label = if cue { format!("Stop cueing {name}") } else { format!("Cue {name} on the headphones") };
    let response = button_area(ui, rect, id, label);
    let Lit { el, toggle, lit } = lit_toggle(ui, &response, BUTTON.id("cue"), &[Checked], cue);
    let (fill, text) = toggle.at(lit);
    toggle_face(painter, rect, &el, &response, fill, toggle.image(lit));
    if !super::skin::paint(painter, rect, "cue", super::skin::State::response(&response, cue), text, 0.0) {
        headphones(painter, rect, text);
    }
    response.clicked().then(|| Action::new("track.setCue", serde_json::json!({ "track": track, "cue": !cue })))
}

/// A pair of headphones in the middle of `rect`, in `color`: the band an arc over the top, a cup
/// at each of its ends. Sized from the rectangle's height, so it reads at a button's 15 points.
pub fn headphones(painter: &Painter, rect: Rect, color: Color32) {
    let size = rect.height().min(rect.width()) * 0.62;
    let radius = size * 0.45;
    let center = rect.center() + egui::vec2(0.0, size * 0.12);
    let band: Vec<Pos2> = (0..=12)
        .map(|i| {
            let angle = std::f32::consts::PI * (1.0 + i as f32 / 12.0);
            center + egui::vec2(angle.cos() * radius, angle.sin() * radius)
        })
        .collect();
    painter.add(egui::Shape::line(band, egui::Stroke::new((size * 0.13).max(1.0), color)));
    let cup = egui::vec2(size * 0.26, size * 0.42);
    for side in [-1.0f32, 1.0] {
        let at = center + egui::vec2(side * radius, cup.y * 0.35);
        painter.rect_filled(Rect::from_center_size(at, cup), size * 0.06, color);
    }
}

/// A track's arm button (components › Arm): a ring off, a filled red dot on its dark red
/// background when armed. Returns the command a click asks for: a click arms the track alone;
/// with Ctrl (Cmd) held it is added to the tracks armed already. Arming a track that plays
/// notes enables recording and, in Auto monitoring, hears its MIDI input.
pub fn arm_button(
    ui: &Ui,
    painter: &Painter,
    rect: Rect,
    id: egui::Id,
    track: u32,
    name: &str,
    armed: bool,
) -> Option<Action> {
    let label = if armed { format!("Disarm {name}") } else { format!("Arm {name} for recording") };
    let response = button_area(ui, rect, id, label).on_hover_text("Make this track ready to record");
    // `button#arm`, `[checked][armed]` while armed: only a rule for that state lights it, so the
    // plain button it is compared with has neither. The red dot on its dark red is the arm's look,
    // whatever their contrast; on a lit fill a rule gives, the red unless it does not read there.
    let Lit { el, toggle, lit } = lit_toggle_kind(ui, &response, BUTTON.id("arm"), &[Checked, Armed], armed, "arm");
    let (fill, ring) = toggle.at(lit);
    toggle_face(painter, rect, &el, &response, fill, toggle.image(lit));
    let radius = (rect.height().min(rect.width()) * 0.22).clamp(2.5, 4.0);
    if !super::skin::paint(painter, rect, "arm", super::skin::State::response(&response, armed), ring, 0.0) {
        painter.circle_stroke(rect.center(), radius, Stroke::new(1.2, ring));
        if lit > 0.0 {
            // The dot in the lit ink.
            painter.circle_filled(rect.center(), radius, toggle.on.1.gamma_multiply(lit));
        }
    }
    if !response.clicked() {
        return None;
    }
    let adding = ui.input(|i| i.modifiers.command);
    Some(Action::new(
        "track.setArm",
        serde_json::json!({ "track": track, "armed": !armed, "exclusive": !armed && !adding }),
    ))
}

/// A dropdown field (components › Dropdown): the current choice and a chevron; the caller opens
/// its menu from `response`.
pub fn dropdown(ui: &Ui, painter: &Painter, rect: Rect, response: &Response, text: &str) {
    form_dropdown(ui, painter, rect, response, text);
}

/// Text fields have no enclosing box; their underline is painted after interaction.
pub fn input_frame() -> egui::Frame {
    egui::Frame::NONE.inner_margin(2.0)
}

/// A text edit with the same transparent surface and focus underline in every view.
pub fn text_input(ui: &mut Ui, edit: egui::TextEdit<'_>) -> Response {
    let el = TEXT_INPUT.enter();
    let mut styled_ui = el.ui(ui);
    let ui = &mut *styled_ui;
    let response = ui.add(edit.frame(input_frame()));
    input_line_of(ui, &response, &el);
    response
}

/// The fixed-rectangle counterpart used by browser rows and project dialogs.
pub fn text_input_at(ui: &mut Ui, rect: Rect, edit: egui::TextEdit<'_>) -> Response {
    let el = TEXT_INPUT.enter();
    let mut styled_ui = el.ui(ui);
    let ui = &mut *styled_ui;
    let response = ui.put(rect, edit.frame(input_frame()));
    input_line_of(ui, &response, &el);
    response
}

/// Closed choice fields show their surrounding surface; their menus retain normal styling: the
/// rows take the `button` rules that reach them in the drop-down (`dropdown button`), not the
/// field's own radius, border or fill (#1487). The chosen value stays on one line, as it did when egui laid the field out in a row of its
/// own (before 0.37): a field given its width wraps "1 bar" otherwise. A field's own
/// `wrap_mode` still wins.
pub fn combo_box<R>(
    ui: &mut Ui,
    combo: egui::ComboBox,
    contents: impl FnOnce(&mut Ui) -> R,
) -> egui::InnerResponse<Option<R>> {
    let around = ui.style().clone();
    let el = DROPDOWN.enter();
    let mut styled_ui = el.ui(ui);
    let ui = &mut *styled_ui;
    let menu_style = super::style_class::rows_in_scope(&around, ui.style());
    // The list's frame: egui starts a pop-up from the root's look (the theme's), and the floating
    // layers' pass frames it as the root's `popup`; `popup` as the drop-down sees it (a device
    // panel's sheet included) goes over both.
    let own_popup = super::style_class::popup_in_scope();
    let combo = match own_popup.clone() {
        Some(popup) => combo.popup_style(egui::style::StyleModifier::new(move |style| {
            super::style_class::apply_menu(&popup, &mut style.visuals);
        })),
        None => combo,
    };
    let response = ui
        .scope(|ui| {
            transparent_control(ui);
            arrow_color(ui);
            ui.style_mut().wrap_mode = Some(egui::TextWrapMode::Extend);
            let response = combo.show_ui(ui, |ui| {
                ui.set_style(menu_style);
                contents(ui)
            });
            input_line_of(ui, &response.response, &el);
            response
        })
        .inner;
    if let Some(popup) = own_popup {
        // egui's id for a combo box's list (`ComboBox::widget_to_popup_id`), its area's and layer's.
        super::floating::style_in_place(ui.ctx(), response.response.id.with("popup"), popup);
    }
    response
}

/// egui draws a drop-down's arrow in its widgets' foreground color: `dropdown-arrow { color }` in
/// the drop-down's scope (the base sheet's `--text-hint`, a device panel's sheet included) goes
/// over it, as it does for [`form_dropdown`]'s arrow. The text keeps the field's own `color`.
fn arrow_color(ui: &mut Ui) {
    let look = DROPDOWN_ARROW.look();
    let Some(arrow) = look.style().filter(|style| style.authored.color).map(|style| style.color) else { return };
    let visuals = &mut ui.style_mut().visuals;
    if visuals.override_text_color.is_none() {
        // The text would take the arrow's color with it: keep it the theme's.
        visuals.override_text_color = Some(visuals.widgets.inactive.text_color());
    }
    for widget in [
        &mut visuals.widgets.inactive,
        &mut visuals.widgets.hovered,
        &mut visuals.widgets.active,
        &mut visuals.widgets.open,
    ] {
        widget.fg_stroke.color = arrow;
    }
}

fn transparent_control(ui: &mut Ui) {
    let visuals = &mut ui.style_mut().visuals;
    for widget in [
        &mut visuals.widgets.inactive,
        &mut visuals.widgets.hovered,
        &mut visuals.widgets.active,
        &mut visuals.widgets.open,
    ] {
        widget.bg_fill = Color32::TRANSPARENT;
        widget.weak_bg_fill = Color32::TRANSPARENT;
        widget.bg_stroke = Stroke::NONE;
    }
}

/// Draggable/typed numeric fields share the form underline without changing interaction.
pub fn number_input(ui: &mut Ui, size: Option<egui::Vec2>, number: egui::DragValue<'_>) -> Response {
    let el = VALUE.enter();
    let mut styled_ui = el.ui(ui);
    let ui = &mut *styled_ui;
    ui.scope(|ui| {
        transparent_control(ui);
        let response = if let Some(size) = size { ui.add_sized(size, number) } else { ui.add(number) };
        input_line_of(ui, &response, &el);
        response
    })
    .inner
}

pub fn number_input_at(ui: &mut Ui, rect: Rect, number: egui::DragValue<'_>) -> Response {
    let el = VALUE.enter();
    let mut styled_ui = el.ui(ui);
    let ui = &mut *styled_ui;
    ui.scope(|ui| {
        transparent_control(ui);
        let response = ui.put(rect, number);
        input_line_of(ui, &response, &el);
        response
    })
    .inner
}

/// A quiet underline, stronger on hover and accented while editing, in the colors of
/// `text-input`: a field drawn as another element takes its own with [`input_line_of`].
pub fn input_line(ui: &Ui, response: &Response) {
    let el = TEXT_INPUT.response(response).enter();
    input_line_of(ui, response, &el);
}

/// [`input_line`] of a field drawn as `el` (`text-input`, `value`, `dropdown`): its `border-color`,
/// growing stronger under the pointer, and its `accent-color` while it is edited.
pub fn input_line_of(ui: &Ui, response: &Response, el: &Entered) {
    underline(ui.painter(), response.rect, response.has_focus(), el, hover(ui, response));
}

/// The line under a field in `rect`: `el`'s `accent-color` while `focused`, else its
/// `border-color`, `hovered` (0 to 1) of the way to the one it has under the pointer.
fn underline(painter: &Painter, rect: Rect, focused: bool, el: &Entered, hovered: f32) {
    let color = if focused { el.accent() } else { el.hover_fade(hovered, Look::border) };
    painter.hline(rect.x_range(), rect.bottom() - 0.5, theme::hairline(color));
}

/// A form field whose surrounding surface shows through, with only an underline.
/// `dropdown`: its underline ([`input_line_of`]), and its choice and chevron in its `color`.
pub fn form_dropdown(ui: &Ui, painter: &Painter, rect: Rect, response: &Response, text: &str) {
    let el = DROPDOWN.response(response).enter();
    underline(painter, rect, response.has_focus(), &el, hover(ui, response));
    // The arrow is a part of the field, in its own color: a rule for the field's text leaves it.
    dropdown_contents(painter, rect, text, el.color(), DROPDOWN_ARROW.look().color());
}

/// A secondary routing field: quiet until hovered, with the same menu and hit area. It takes the
/// colors of the surface it is on (`background`), so a theme's rule for every `dropdown` leaves
/// it alone: only one for `dropdown[kind="subtle"]` gives its `background-color` while hovered,
/// its `border-color` for the line and its `color`. Its `accent-color` is the line while it has
/// the keys, and the sheet's `:hover` color what the surface fades toward.
pub fn subtle_dropdown(ui: &Ui, painter: &Painter, rect: Rect, response: &Response, text: &str, background: Color32) {
    let plain = DROPDOWN.response(response).probe();
    let own = DROPDOWN.response(response).attribute("kind", "subtle").look();
    // What only a rule for the kind gives: not what a rule for every dropdown does.
    let amount = hover(ui, response);
    let mut surface = background;
    if amount > 0.0 || response.has_focus() {
        surface = own.themed_unlike(&plain, Paint::Background).unwrap_or_else(|| {
            let hot = DROPDOWN.attribute("kind", "subtle").pointer(true, false, false).probe();
            motion::mix(background, hot.background(), amount.max(0.3))
        });
    }
    let line = if response.has_focus() {
        own.accent()
    } else {
        own.themed_unlike(&plain, Paint::Border)
            .unwrap_or_else(|| motion::mix(background, theme::text_on(background), 0.25 + 0.25 * amount))
    };
    painter.hline(rect.x_range(), rect.bottom() - 0.5, theme::hairline(line));
    let ink = own.themed_unlike(&plain, Paint::Color).unwrap_or_else(|| theme::text_on(surface));
    dropdown_contents(painter, rect, text, ink, motion::mix(surface, ink, 0.65));
}

fn dropdown_contents(painter: &Painter, rect: Rect, text: &str, ink: Color32, hint: Color32) {
    let inner = rect.shrink2(Vec2::new(4.0, 0.0));
    let chevron = Rect::from_center_size(Pos2::new(inner.right() - 3.0, rect.center().y), Vec2::new(6.0, 4.0));
    painter.add(egui::Shape::convex_polygon(
        vec![chevron.left_top(), chevron.right_top(), chevron.center_bottom()],
        hint,
        Stroke::NONE,
    ));
    let label = Rect::from_min_max(inner.min, Pos2::new(chevron.left() - 4.0, inner.bottom()));
    text_in(painter, label, Align2::LEFT_CENTER, text, theme::ui_font(theme::TEXT_SMALL), ink);
}

/// What a value control did this frame.
pub enum Drag {
    None,
    /// Being dragged: the value to show.
    Moving(f64),
    /// Released, or moved by the wheel or an arrow key: the value to commit.
    Done(f64),
    /// Typed or reset by a double-click: the value itself, which a selection takes as it is.
    Set(f64),
}

/// Whether the wheel over a value field changes its value this frame: with Ctrl/Cmd held. Without it
/// the wheel scrolls the view under the field (`docs/spec/arrangement-editing.md`).
pub fn wheel_moves_value(ui: &Ui) -> bool {
    ui.input(|i| (i.modifiers.ctrl || i.modifiers.command) && !i.modifiers.alt)
}

/// The points one tick of a wheel scrolls, which is one step of a value control (egui reports a
/// line of a wheel as this many points on most platforms).
const WHEEL_TICK: f64 = 50.0;

/// How far the wheel turned this frame, in points, read from the events themselves rather than
/// from egui's smoothed scroll, so one tick of a wheel is one step and not a run of them.
/// `value` moved by `step` within `range`. A value off the scale (a volume at -inf) moves from the
/// scale's end for a step up, as a drag does, and stays where it is for a step down.
fn stepped(value: f64, range: (f64, f64), step: f64) -> f64 {
    if value.is_finite() {
        (value + step).clamp(range.0, range.1)
    } else if step > 0.0 {
        (range.0 + step).clamp(range.0, range.1)
    } else {
        value
    }
}

fn wheel_points(ui: &Ui) -> f32 {
    ui.input(|i| {
        i.raw
            .events
            .iter()
            .filter_map(|event| match event {
                egui::Event::MouseWheel { unit, delta, .. } => {
                    // Some systems turn Alt and the wheel into a sideways scroll.
                    let turned = if delta.y == 0.0 { delta.x } else { delta.y };
                    Some(match unit {
                        egui::MouseWheelUnit::Point => turned,
                        egui::MouseWheelUnit::Line => turned * WHEEL_TICK as f32,
                        egui::MouseWheelUnit::Page => turned * 400.0,
                    })
                }
                _ => None,
            })
            .sum()
    })
}

/// The value control clicked or dragged last, and the frame it was drawn in: the arrow keys and
/// typed digits go to it (#707) until a click elsewhere, Enter or Escape.
const ACTIVE_VALUE: &str = "active-value";

/// Whether a value control takes this frame's keys: an arrow, a digit, Enter, Escape or
/// Backspace while one is active. The window's shortcuts wait for this frame then, so a digit
/// typed into it does not pick a tool; Ctrl+Z and Space still reach them.
pub fn value_active(ctx: &egui::Context) -> bool {
    let pass = ctx.cumulative_pass_nr();
    let active = ctx
        .data(|d| d.get_temp::<(egui::Id, u64)>(egui::Id::unique(ACTIVE_VALUE)))
        .is_some_and(|(_, drawn)| pass.saturating_sub(drawn) <= 2);
    // A control that reads separators too (a time signature, `3/4`) takes `/` and Space.
    let separators = active && ctx.data(|d| d.get_temp::<bool>(egui::Id::unique(SEPARATORS))).unwrap_or(false);
    active
        && ctx.input(|i| {
            i.events.iter().any(|event| match event {
                egui::Event::Key { key, pressed: true, modifiers, .. } => {
                    !modifiers.command
                        && (matches!(
                            key,
                            egui::Key::ArrowUp
                                | egui::Key::ArrowDown
                                | egui::Key::Enter
                                | egui::Key::Escape
                                | egui::Key::Backspace
                        ) || (separators && *key == egui::Key::Space))
                }
                egui::Event::Text(text) => text.chars().all(|c| {
                    c.is_ascii_digit() || matches!(c, '-' | '.' | ',') || (separators && matches!(c, '/' | ' '))
                }),
                _ => false,
            })
        })
}

/// Whether the control holding the keys reads `/` and Space as separators: see [`hold_value_keys`].
const SEPARATORS: &str = "active-value-separators";

/// Makes `id` the value control that takes this frame's keys, for a control that reads them
/// itself rather than through [`drag_value`]; `separators` says it reads `/` and Space too, so
/// the window's shortcuts leave them to it. Called each frame it holds them.
pub fn hold_value_keys(ctx: &egui::Context, id: egui::Id, separators: bool) {
    let pass = ctx.cumulative_pass_nr();
    ctx.data_mut(|d| {
        d.insert_temp(egui::Id::unique(ACTIVE_VALUE), (id, pass));
        d.insert_temp(egui::Id::unique(SEPARATORS), separators);
    });
}

/// Whether `id` holds the keys: no other value control was clicked since it took them.
pub fn holds_value_keys(ctx: &egui::Context, id: egui::Id) -> bool {
    ctx.data(|d| d.get_temp::<(egui::Id, u64)>(egui::Id::unique(ACTIVE_VALUE))).is_some_and(|(held, _)| held == id)
}

/// Lets the keys go, when `id` still holds them.
pub fn release_value_keys(ctx: &egui::Context, id: egui::Id) {
    if holds_value_keys(ctx, id) {
        ctx.data_mut(|d| {
            d.remove::<(egui::Id, u64)>(egui::Id::unique(ACTIVE_VALUE));
            d.remove::<bool>(egui::Id::unique(SEPARATORS));
        });
    }
}

/// The keys a drop-down field takes this frame once it is clicked (as a value control does, until a
/// click elsewhere, or Escape with its list closed): the arrow steps through its entries (down is
/// +1, the next entry), and Enter while its list is `open`. Escape with the list open is left to
/// the list, which it closes; the field keeps the keys, so the arrows then step the value itself.
/// Escape with the list closed lets the keys go and is left to the rest of the window too (the
/// panel's Escape, which leaves Quantise, still sees it).
pub(crate) fn chooser_keys(ui: &Ui, response: &Response, open: bool) -> (i32, bool) {
    let ctx = ui.ctx();
    let key = egui::Id::unique(ACTIVE_VALUE);
    let pass = ctx.cumulative_pass_nr();
    if response.clicked() {
        ctx.data_mut(|d| d.insert_temp(key, (response.id, pass)));
    }
    let active = ctx.data(|d| d.get_temp::<(egui::Id, u64)>(key)).is_some_and(|(id, _)| id == response.id);
    if !active {
        return (0, false);
    }
    // A press anywhere else lets it go; one in its own open list does not.
    let pressed_elsewhere =
        ui.input(|i| i.pointer.any_pressed() && i.pointer.interact_pos().is_none_or(|at| !response.rect.contains(at)));
    if pressed_elsewhere && !response.hovered() && !open {
        ctx.data_mut(|d| d.remove::<(egui::Id, u64)>(key));
        return (0, false);
    }
    ctx.data_mut(|d| d.insert_temp(key, (response.id, pass)));
    let (mut steps, mut enter, mut escape) = (0i32, false, false);
    ctx.input_mut(|i| {
        i.events.retain(|event| match event {
            egui::Event::Key { key: egui::Key::ArrowUp, pressed: true, modifiers, .. } if !modifiers.command => {
                steps -= 1;
                false
            }
            egui::Event::Key { key: egui::Key::ArrowDown, pressed: true, modifiers, .. } if !modifiers.command => {
                steps += 1;
                false
            }
            egui::Event::Key { key: egui::Key::Enter, pressed: true, modifiers, .. } if open && !modifiers.command => {
                enter = true;
                false
            }
            egui::Event::Key { key: egui::Key::Escape, pressed: true, modifiers, .. }
                if !open && !modifiers.command =>
            {
                escape = true;
                true
            }
            _ => true,
        });
    });
    if escape {
        ctx.data_mut(|d| d.remove::<(egui::Id, u64)>(key));
        return (0, false);
    }
    (steps, enter)
}

/// The keys an active value control takes this frame: arrow steps (up is +1), and Enter with
/// the number typed into it. Ctrl (Cmd) with a key leaves it to the window's shortcuts, as
/// [`value_active`] says.
pub(crate) fn value_keys(ui: &Ui, response: &Response) -> (i32, bool, Option<String>) {
    let ctx = ui.ctx();
    let key = egui::Id::unique(ACTIVE_VALUE);
    let typed_id = response.id.with("typed");
    let pass = ctx.cumulative_pass_nr();
    if response.clicked() || response.drag_started_by(egui::PointerButton::Primary) {
        suhina_plugins::trace!("value control takes the keys: {:?}", response.id);
        ctx.data_mut(|d| {
            d.insert_temp(key, (response.id, pass));
            d.remove::<bool>(egui::Id::unique(SEPARATORS));
            d.remove::<String>(typed_id);
        });
    }
    let active = ctx.data(|d| d.get_temp::<(egui::Id, u64)>(key)).is_some_and(|(id, _)| id == response.id);
    if !active {
        return (0, false, None);
    }
    // A press anywhere else lets it go.
    if ui.input(|i| i.pointer.any_pressed()) && !response.hovered() {
        suhina_plugins::trace!("value control lets the keys go: a press elsewhere");
        ctx.data_mut(|d| {
            d.remove::<(egui::Id, u64)>(key);
            d.remove::<String>(typed_id);
        });
        return (0, false, None);
    }
    ctx.data_mut(|d| d.insert_temp(key, (response.id, pass)));
    let mut typed = ctx.data(|d| d.get_temp::<String>(typed_id)).unwrap_or_default();
    let (mut steps, mut fine, mut enter, mut escape) = (0i32, false, false, false);
    ctx.input_mut(|i| {
        i.events.retain(|event| match event {
            egui::Event::Key { key: egui::Key::ArrowUp, pressed: true, modifiers, .. } if !modifiers.command => {
                steps += 1;
                fine |= modifiers.shift;
                false
            }
            egui::Event::Key { key: egui::Key::ArrowDown, pressed: true, modifiers, .. } if !modifiers.command => {
                steps -= 1;
                fine |= modifiers.shift;
                false
            }
            egui::Event::Key { key: egui::Key::Enter, pressed: true, modifiers, .. } if !modifiers.command => {
                enter = true;
                false
            }
            egui::Event::Key { key: egui::Key::Escape, pressed: true, modifiers, .. } if !modifiers.command => {
                escape = true;
                false
            }
            egui::Event::Key { key: egui::Key::Backspace, pressed: true, modifiers, .. } if !modifiers.command => {
                typed.pop();
                false
            }
            egui::Event::Text(text) if text.chars().all(|c| c.is_ascii_digit() || matches!(c, '-' | '.' | ',')) => {
                typed.push_str(text);
                false
            }
            _ => true,
        });
    });
    if escape {
        ctx.data_mut(|d| {
            d.remove::<(egui::Id, u64)>(key);
            d.remove::<String>(typed_id);
        });
        return (0, false, None);
    }
    if enter {
        suhina_plugins::trace!("value control: Enter with {typed:?} typed");
        ctx.data_mut(|d| {
            d.remove::<(egui::Id, u64)>(key);
            d.remove::<String>(typed_id);
        });
        return (0, fine, Some(typed));
    }
    ctx.data_mut(|d| d.insert_temp(typed_id, typed.clone()));
    // Lit while it takes the keys; what is typed shows over it, on a layer above the window's
    // own, as a theme's field drawn after this would hide it (#916: the typed number did not
    // show until Enter set it).
    let painter = ui.ctx().layer_painter(egui::LayerId::new(egui::Order::Foreground, response.id.with("typed-layer")));
    // Keep the target ring outside the surface: custom value painters run after key handling
    // and would cover an inset rectangle, leaving only sharp yellow corners behind.
    let radius = if response.rect.width() > response.rect.height() * 4.0 { response.rect.height() * 0.5 } else { 4.0 };
    // What is typed is `text-input[kind="typed"]`: its ring in its `border-color`, its
    // `background-color` and its `color`. It is drawn over the value control, not as a text field,
    // so only a rule for the kind reaches it, not one for every `text-input`.
    let look = TEXT_INPUT.attribute("kind", "typed").look();
    painter.rect_stroke(response.rect, radius, Stroke::new(1.0, look.border()), egui::StrokeKind::Outside);
    if !typed.is_empty() {
        painter.rect_filled(response.rect.shrink(1.0), 0.0, look.background());
        painter.text(
            response.rect.center(),
            egui::Align2::CENTER_CENTER,
            &typed,
            theme::mono_font(theme::TEXT_SMALL),
            look.color(),
        );
    }
    (steps, fine, None)
}

/// The way a value control is dragged: up or down (up raises it), across (right raises it), or
/// either, the drag's first few points choosing which. Each control picks the one that matches
/// how it is drawn: a knob or an upright fader up or down, a bar lying across a header either way.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum DragAxis {
    #[default]
    Vertical,
    Horizontal,
    /// Up or down, or across: whichever the drag moves along first, for the rest of that drag.
    Both,
}

impl DragAxis {
    /// How far a drag moved the value, in points: right or up is positive. [`DragAxis::Both`]
    /// takes the larger of the two; [`drag_value_axis`] keeps one axis for a whole drag instead.
    fn along(self, moved: Vec2) -> f32 {
        match self {
            Self::Vertical => -moved.y,
            Self::Horizontal => moved.x,
            Self::Both if moved.x.abs() > moved.y.abs() => moved.x,
            Self::Both => -moved.y,
        }
    }

    /// The pointer shown over a control dragged this way: none of its own for one dragged either
    /// way, as neither arrow would say how.
    fn cursor(self) -> Option<egui::CursorIcon> {
        match self {
            Self::Vertical => Some(egui::CursorIcon::ResizeVertical),
            Self::Horizontal => Some(egui::CursorIcon::ResizeHorizontal),
            Self::Both => None,
        }
    }
}

/// How far a [`DragAxis::Both`] drag moves, in points, before it chooses its axis.
const AXIS_CHOICE_POINTS: f32 = 3.0;

/// A [`DragAxis::Both`] drag's axis: the movement so far until it is far enough to choose, then
/// the axis chosen, kept until the drag ends.
#[derive(Debug, Clone, Copy)]
enum AxisChoice {
    Undecided(Vec2),
    Chosen(DragAxis),
}

/// How far a [`DragAxis::Both`] drag moved the value this frame: nothing until it has moved
/// [`AXIS_CHOICE_POINTS`], then the movement so far along the axis it moved along more, and from
/// then on that axis alone, so a drag that wavers never switches. Once chosen, the pointer shows
/// that axis's arrow.
fn along_chosen_axis(ui: &Ui, id: egui::Id, moved: Vec2) -> f32 {
    let key = id.with("drag-axis");
    let choice = ui.ctx().data(|d| d.get_temp::<AxisChoice>(key)).unwrap_or(AxisChoice::Undecided(Vec2::ZERO));
    let (choice, along) = match choice {
        AxisChoice::Chosen(axis) => (choice, axis.along(moved)),
        AxisChoice::Undecided(so_far) => {
            let so_far = so_far + moved;
            if so_far.length() < AXIS_CHOICE_POINTS {
                (AxisChoice::Undecided(so_far), 0.0)
            } else {
                let axis = if so_far.x.abs() > so_far.y.abs() { DragAxis::Horizontal } else { DragAxis::Vertical };
                (AxisChoice::Chosen(axis), axis.along(so_far))
            }
        }
    };
    ui.ctx().data_mut(|d| d.insert_temp(key, choice));
    if let AxisChoice::Chosen(axis) = choice
        && let Some(icon) = axis.cursor()
    {
        value_drag_shows(ui.ctx(), icon);
    }
    along
}

/// A number changed by dragging vertically (up raises it), by the wheel over it and by a
/// double-click; clicked, the arrow keys step it (Shift: ten times finer) and a number typed and
/// Enter set it (#707). `per_point` is the change per point of drag and `per_tick` the change
/// per wheel tick or arrow key; Shift makes both ten times finer.
#[allow(clippy::too_many_arguments)]
pub fn drag_value(
    ui: &Ui,
    response: &Response,
    preview: &mut Option<f64>,
    value: f64,
    per_point: f64,
    per_tick: f64,
    range: (f64, f64),
    default: f64,
) -> Drag {
    drag_value_axis(ui, response, preview, value, per_point, per_tick, range, default, DragAxis::Vertical)
}

/// A number typed into a value control, in the control's range. A pan (-1 to 1) shows and is
/// typed on its own scale, 50L to 50R ("-50" is all the way left, "25" half right), as
/// [`format_pan`] shows it, so a number past 1 is read so.
fn typed_value(number: f64, range: (f64, f64)) -> f64 {
    let pan = range == (-1.0, 1.0);
    let number = if pan && number.abs() > 1.0 { number / 50.0 } else { number };
    number.clamp(range.0, range.1)
}

/// [`drag_value`] dragged along `axis`.
#[allow(clippy::too_many_arguments)]
pub fn drag_value_axis(
    ui: &Ui,
    response: &Response,
    preview: &mut Option<f64>,
    value: f64,
    per_point: f64,
    per_tick: f64,
    range: (f64, f64),
    default: f64,
    axis: DragAxis,
) -> Drag {
    drag_value_inner(ui, response, preview, value, per_point, per_tick, range, default, axis, true)
}

/// [`drag_value`] for a control that reads its keys itself (a time signature, typed as `3/4`
/// across its two numbers, [`hold_value_keys`]): the drag, the wheel and a double-click only.
#[allow(clippy::too_many_arguments)]
pub fn drag_value_without_keys(
    ui: &Ui,
    response: &Response,
    preview: &mut Option<f64>,
    value: f64,
    per_point: f64,
    per_tick: f64,
    range: (f64, f64),
    default: f64,
) -> Drag {
    drag_value_inner(ui, response, preview, value, per_point, per_tick, range, default, DragAxis::Vertical, false)
}

#[allow(clippy::too_many_arguments)]
fn drag_value_inner(
    ui: &Ui,
    response: &Response,
    preview: &mut Option<f64>,
    value: f64,
    per_point: f64,
    per_tick: f64,
    range: (f64, f64),
    default: f64,
    axis: DragAxis,
    keys: bool,
) -> Drag {
    let _el = VALUE.response(response).enter();
    // Keys to the control clicked last: the arrows step it, Enter sets what was typed.
    let (steps, fine, typed) = if keys { value_keys(ui, response) } else { (0, false, None) };
    if let Some(typed) = typed
        && let Ok(number) = typed.replace(',', ".").trim().parse::<f64>()
        && number.is_finite()
    {
        *preview = None;
        return Drag::Set(typed_value(number, range));
    }
    if steps != 0 {
        let step = f64::from(steps) * per_tick * if fine { 0.1 } else { 1.0 };
        *preview = None;
        return Drag::Done(stepped(value, range, step));
    }
    // Ctrl/Cmd and the wheel over the control move it, as a drag would; the caller commits what it is
    // given. The wheel alone scrolls the view, so scrolling past a field never changes it.
    // Scrolling up raises the value, as dragging up does; Shift too, ten times finer.
    if response.hovered() && wheel_moves_value(ui) {
        let points = wheel_points(ui);
        if points != 0.0 {
            let fine = if ui.input(|i| i.modifiers.shift) { 0.1 } else { 1.0 };
            // A positive delta moves the content down: the wheel turned up, which raises it.
            let step = f64::from(points) / WHEEL_TICK * per_tick * fine;
            *preview = None;
            return Drag::Done(stepped(value, range, step));
        }
    }
    if response.double_clicked() {
        *preview = None;
        return Drag::Set(default);
    }
    if response.drag_started_by(egui::PointerButton::Primary) {
        *preview = Some(if value.is_finite() { value } else { range.0 });
        // A new drag chooses its own axis.
        ui.ctx().data_mut(|d| d.remove::<AxisChoice>(response.id.with("drag-axis")));
    }
    if response.dragged_by(egui::PointerButton::Primary) {
        let fine = if ui.input(|i| i.modifiers.shift) { 0.1 } else { 1.0 };
        let start = preview.unwrap_or(value);
        let moved = value_drag_delta(ui, response);
        // A drag that shows the pointer shows the arrow of its axis; one that may go either way
        // shows it once it has chosen (`along_chosen_axis`).
        if let Some(icon) = axis.cursor() {
            value_drag_shows(ui.ctx(), icon);
        }
        let delta = match axis {
            DragAxis::Both => along_chosen_axis(ui, response.id, moved),
            axis => axis.along(moved),
        };
        let next = (start + f64::from(delta) * per_point * fine).clamp(range.0, range.1);
        *preview = Some(next);
        return Drag::Moving(next);
    }
    if response.drag_stopped_by(egui::PointerButton::Primary) {
        return match preview.take() {
            Some(done) => Drag::Done(done),
            None => Drag::None,
        };
    }
    Drag::None
}

/// Makes `rect` a draggable value with a label for screen readers and tests, dragged up or down
/// (the up-down pointer over it); [`value_area_axis`] for another [`DragAxis`].
pub fn value_area(ui: &Ui, rect: Rect, id: egui::Id, label: impl ToString, value: impl ToString) -> Response {
    value_area_axis(ui, rect, id, label, value, DragAxis::Vertical)
}

/// A [`value_area`] dragged along `axis`, with the pointer that says so.
pub fn value_area_axis(
    ui: &Ui,
    rect: Rect,
    id: egui::Id,
    label: impl ToString,
    value: impl ToString,
    axis: DragAxis,
) -> Response {
    let mut response = ui.interact(rect, id, Sense::click_and_drag());
    if let Some(cursor) = axis.cursor() {
        response = response.on_hover_cursor(cursor);
    }
    let (label, value) = (label.to_string(), value.to_string());
    response.widget_info(|| {
        let mut info = WidgetInfo::labeled(Role::SpinButton, true, &label);
        info.current_text_value = Some(value.clone());
        info
    });
    response
}

/// The knob: a small circle with a pointer from the center, −135° … +135° of travel for
/// `position` 0 … 1 (components › Mixer strip).
/// The artwork roles of a knob's body and pointer: a pan's and a send's (a return's level) knob
/// take `pan.knob.*` and `send.knob.*` when the theme has them, in the Launcher and the
/// Arrangement alike, so they can differ from the devices' knobs; otherwise every knob shares
/// `knob.*`.
pub fn knob_roles() -> (&'static str, &'static str) {
    [
        (super::element::PAN, "pan.knob.body", "pan.knob.pointer"),
        (super::element::SEND, "send.knob.body", "send.knob.pointer"),
    ]
    .into_iter()
    .find(|(class, body, _)| super::style_class::within(*class) && super::skin::has_own(body))
    .map_or(("knob.body", "knob.pointer"), |(_, body, tip)| (body, tip))
}

pub fn knob(painter: &Painter, center: Pos2, size: f32, position: f32, pointer: Color32, response: Option<&Response>) {
    let el = KNOB.response_if(response).enter();
    let state =
        response.map(|response| super::skin::State::response(response, false)).unwrap_or(super::skin::State::Normal);
    let pointer = el.color_or(pointer);
    let radius = size * 9.0 / 22.0;
    let rect = Rect::from_center_size(center, Vec2::splat(size));
    let (body, tip_role) = knob_roles();
    // A knob is two elements, each coloring its own artwork (`currentColor`), with the knob's
    // pointer states. The body's color is its own (the palette's tier gives it one), never one
    // inherited from the knob; the pointer takes the knob's.
    let body_ink = super::element::KNOB_BODY.response_if(response).enter().color();
    if !super::skin::paint(painter, rect, body, state, body_ink, 0.0) {
        // Without artwork, a disc in the knob's `background-color`, ringed in its own `color` (a
        // line takes only a color given to it, not one inherited).
        painter.circle_filled(center, radius, el.background());
        painter.circle_stroke(center, radius, theme::hairline(el.color()));
    }
    el.paint_background(painter, rect, size * 0.5);
    let angle = (-135.0 + 270.0 * position.clamp(0.0, 1.0)).to_radians();
    let tip = center + Vec2::new(angle.sin(), -angle.cos()) * (radius - 1.0);
    let pointer = super::element::KNOB_POINTER.response_if(response).enter().color_or(pointer);
    if !super::skin::paint(painter, rect, tip_role, state, pointer, angle) {
        painter.line_segment([center, tip], Stroke::new(1.5, pointer));
    }
}

/// A dial as an `.sd` control's dial draws it, the built-in canvases' wheels (the reverb's and
/// the delay's): the rim arc, the ring's disc, the inner disc, the pointer line to 72 % of the
/// radius, 270° with the gap under. The colors are the tokens the `knob` scope gives — the
/// same scope the canvases' dials take theirs from, so a theme's own coloring applies — and
/// no artwork stands for the face, as a canvas has none.
pub fn dial(painter: &Painter, center: Pos2, size: f32, position: f32, response: Option<&Response>) {
    // The `knob`'s colors: its `border-color` for the rim, its `background-color` for the inner
    // disc and its `accent-color` for the pointer.
    let el = KNOB.response_if(response).attribute("kind", "dial").enter();
    let radius = (size * 0.5).max(1.0);
    let rim = dial_rim(el.style().as_ref(), radius);
    // The face: the rim arc (2.35 to 7.05 rad as an `.sd` dial's is, i.e. 135° past each side
    // over the top), the ring disc inside it, the inner disc — the theme's tokens.
    let points: Vec<Pos2> = (0..=48)
        .map(|i| {
            let a = (-135.0 + 270.0 * i as f32 / 48.0).to_radians();
            center + Vec2::new(a.sin(), -a.cos()) * (radius - rim / 2.0).max(0.0)
        })
        .collect();
    painter.add(egui::Shape::line(points, Stroke::new(rim, el.border())));
    painter.circle_filled(
        center,
        (radius - rim).max(0.0),
        crate::kit::style_class::surface_or(super::element::KNOB_RING.look().background()),
    );
    painter.circle_filled(center, (radius - 2.0 * rim).max(0.0), el.background());
    // The pointer: a quarter in, three quarters to the rim, in the accent token.
    let angle = (-135.0 + 270.0 * position.clamp(0.0, 1.0)).to_radians();
    let dir = Vec2::new(angle.sin(), -angle.cos());
    painter
        .line_segment([center + dir * (radius * 0.25), center + dir * (radius * 0.72)], Stroke::new(2.0, el.accent()));
}

/// The width of a dial's rim arc of `radius`: the `knob[kind="dial"]`'s `border-width` where the
/// sheet sets one (at most the radius), else a sixth of the radius, so a small dial's arc is as
/// thin for its size as a large one's (#1487 S4: a constant 4 points filled a small dial). The
/// ring and the inner disc are set in by one and two widths.
pub(crate) fn dial_rim(style: Option<&super::native_css::Computed>, radius: f32) -> f32 {
    let themed = style.filter(|style| style.authored.border_width).map(|style| style.border_width);
    themed.map_or(radius / 6.0, |width| width.min(radius)).max(1.0)
}

/// `bpm` on the steps of the place a drag holds, counted from where it started (`grab`: the step
/// and the start), to a thousandth of a BPM, within the tempo's range (#829).
pub fn snap_tempo(grab: Option<(f64, f64)>, bpm: f64) -> f64 {
    let snapped = match grab {
        Some((step, from)) if step > 0.0 => from + ((bpm - from) / step).round() * step,
        _ => bpm,
    };
    ((snapped * 1000.0).round() / 1000.0).clamp(20.0, 999.0)
}

/// The step of the place in the tempo `text` (right-aligned at `right`, characters `width`
/// apart) under `x`: 1 BPM for the integer part, 0.1, 0.01 or 0.001 for the decimals (#829).
pub fn tempo_place(text: &str, right: f32, width: f32, x: f32) -> f64 {
    let len = text.chars().count();
    let Some(dot) = text.find('.') else { return 1.0 };
    if width <= 0.0 {
        return 1.0;
    }
    let from_right = ((right - x) / width).floor().max(0.0) as usize;
    let index = len.saturating_sub(from_right + 1);
    if index <= dot {
        return 1.0;
    }
    let decimals = (index - dot).min(3);
    10f64.powi(-(decimals as i32))
}

/// The characters of the tempo `text` the place of `step` covers, as a range of indices: the
/// whole integer part for 1 BPM, one decimal otherwise.
pub fn tempo_place_span(text: &str, step: f64) -> (usize, usize) {
    let dot = text.find('.').unwrap_or(text.len());
    if step >= 1.0 {
        return (0, dot);
    }
    let decimals = (-step.log10()).round().max(1.0) as usize;
    (dot + decimals, dot + decimals + 1)
}

/// A volume in dB as the mixer shows it.
pub fn format_db(db: f64) -> String {
    if db.is_finite() && db > -70.0 { format!("{db:.1}") } else { "-inf".into() }
}

/// A value's text as the sheet formats it for `el`: `value-decimals: 0 | 1 | 2` rounds its
/// number, and `value-unit: " dB"` follows it (quoted; at most 8 characters). Text with no number
/// (`-inf`, `C`) only takes the unit when it ends in a digit.
pub fn formatted_value(el: &Entered, text: &str) -> String {
    use suhina_style::property::Property;
    let style = el.style();
    let decimals = style
        .as_ref()
        .and_then(|style| style.keyword(Property::ValueDecimals))
        .and_then(|v| v.parse::<usize>().ok())
        .filter(|d| *d <= 2);
    let unit = style
        .as_ref()
        .and_then(|style| style.text(Property::ValueUnit))
        .map(|v| v.trim().trim_matches(['"', '\'']).chars().take(8).collect::<String>());
    let mut out = match (decimals, text.trim().parse::<f64>()) {
        (Some(decimals), Ok(number)) if number.is_finite() => format!("{number:.decimals$}"),
        _ => text.to_owned(),
    };
    if let Some(unit) = unit
        && out.ends_with(|c: char| c.is_ascii_digit())
    {
        out.push_str(&unit);
    }
    out
}

/// A crossfader's value as the ends and the share between them: A, B, or the percentage toward
/// B (`docs/spec/crossfader.md`).
pub fn format_crossfader(value: f64) -> String {
    if value <= 0.005 {
        "A".into()
    } else if value >= 0.995 {
        "B".into()
    } else {
        format!("{}", (value * 100.0).round())
    }
}

/// A pan position as the mixer shows it: 50L … C … 50R.
pub fn format_pan(pan: f64) -> String {
    let amount = (pan * 50.0).round() as i64;
    match amount {
        0 => "C".into(),
        a if a < 0 => format!("{}L", -a),
        a => format!("{a}R"),
    }
}

pub(crate) const VOLUME_RANGE: (f64, f64) = (-70.0, 6.0);
/// What a send knob covers, in dB: it stops at all of the track's signal.
pub(crate) const SEND_RANGE: (f64, f64) = (VOLUME_RANGE.0, 0.0);

/// Where a volume sits on a field, 0 … 1, as the volume fader places it: 0 dB at 85 %, 40 dB
/// per travel down to -18 dB, faster below (components › Value field;
/// `suhina_model::automation::volume_position`).
pub fn volume_position(db: f64) -> f32 {
    suhina_engine::model::automation::volume_position(db) as f32
}

pub(crate) fn play_glyph(painter: &egui::Painter, center: Pos2, playing: bool, color: Color32, size: f32) {
    if playing {
        painter.rect_filled(Rect::from_center_size(center, Vec2::splat(size)), 0.0, color);
    } else {
        let s = size / 7.0;
        painter.add(egui::Shape::convex_polygon(
            vec![
                center + Vec2::new(-3.0 * s, -4.0 * s),
                center + Vec2::new(4.0 * s, 0.0),
                center + Vec2::new(-3.0 * s, 4.0 * s),
            ],
            color,
            Stroke::NONE,
        ));
    }
}

/// A plain push button's face: `button` with `id` (none: any button), entered with its `response`.
/// Its fill fades from its `background-color` to the one it has under the pointer; its edge is its
/// `border-color`; its label its `color`.
pub(crate) fn push_button(ui: &Ui, painter: &Painter, rect: Rect, id: &str, response: &Response, label: &str) {
    let hover = hover(ui, response);
    let el = BUTTON.id(id).response(response).enter();
    push_face(painter, rect, &el, hover, label);
}

/// [`push_button`] of the button `el`, its caller entered.
pub(crate) fn push_face(painter: &Painter, rect: Rect, el: &Entered, hover: f32, label: &str) {
    let fill = el.hover_fade(hover, Look::background);
    el.paint_with(painter, rect, super::element::Native { radius: 0.0, fill });
    if !el.style().is_some_and(|s| s.authored.border || s.authored.border_width) {
        painter.rect_stroke(rect, 0.0, theme::hairline(el.border()), egui::StrokeKind::Inside);
    }
    text_in(painter, rect, Align2::CENTER_CENTER, label, theme::ui_font(theme::TEXT_SMALL), el.color());
}

/// A button the view paints itself, in a flat fill: `button` entered with its `response` (of its
/// `kind`, when it has one: `button[kind="secondary"]` for one that stands back), its fill its
/// `background-color`, fading to the one it has under the pointer; its text its `color`, in a font
/// of `size`.
pub(crate) fn flat_button(
    ui: &Ui,
    painter: &Painter,
    rect: Rect,
    response: &Response,
    label: &str,
    (size, kind): (f32, Option<&str>),
) {
    let builder = BUTTON.response(response);
    let el = match kind {
        Some(kind) => builder.attribute("kind", kind),
        None => builder,
    }
    .enter();
    let fill = el.hover_fade(hover(ui, response), Look::background);
    painter.rect_filled(rect, 0.0, fill);
    text_in(painter, rect, Align2::CENTER_CENTER, label, theme::ui_font(size), el.color());
}

/// A button with `text` on it, `label` for screen readers and tests; true when clicked. Its id is
/// its label in its layer, so a dialog drawn over a panel with a button of the same label (the
/// start panel's *Download update now* under Help › Check for updates…) gets a button of its own
/// (#1229): egui keeps a widget in the layer it was first seen in that frame.
pub(crate) fn button(ui: &Ui, painter: &egui::Painter, rect: Rect, text: &str, label: &str) -> bool {
    let id = Id::unique(("dialog-button", ui.layer_id(), label));
    button_with_id(ui, painter, rect, id, text, label, false).clicked()
}

/// A [`button`] with an id of the caller's: for buttons whose label two of them may share, such
/// as a row's for a file whose name another row's file has too (#1461). `primary` draws it as
/// the one a page means most: filled in the button's `accent-color`, its text in `--text-inverse`.
pub(crate) fn button_with_id(
    ui: &Ui,
    painter: &egui::Painter,
    rect: Rect,
    id: Id,
    text: &str,
    label: &str,
    primary: bool,
) -> Response {
    let response = button_area(ui, rect, id, label);
    let el = BUTTON.response(&response).enter();
    let hovered = hover(ui, &response);
    let fill = if primary {
        motion::mix(el.accent(), el.hover_fade(1.0, Look::background), hovered * 0.25)
    } else {
        el.hover_fade(hovered, Look::background)
    };
    let radius = el.radius_or(0.0);
    button_surface(painter, rect, &el, &response, primary, fill, radius);
    // Its edge in its `border-color` (the accent for the primary one), its text in its `color`.
    let edge = if primary { el.accent() } else { el.border() };
    painter.rect_stroke(rect, radius, theme::hairline(edge), egui::StrokeKind::Inside);
    let small = theme::ui_font(theme::TEXT_SMALL);
    let ink = if primary { super::element::LABEL.attribute("kind", "inverse").look().color() } else { el.color() };
    text_in(painter, rect, Align2::CENTER_CENTER, text, small, ink);
    response
}

/// A horizontal slider of a percentage, 0 to 100 (swing, #1221): a click puts the handle there, a
/// drag moves it and the value follows while it moves (every value of a drag says so, for the
/// caller to send them as one gesture, one undo step), and a double-click puts it at 0. Named
/// "`label`: N %" for a reader and a test. `lit` draws the filled part in the accent, otherwise in
/// a quieter ink. Returns the value to set, and whether it comes from a drag.
#[allow(clippy::too_many_arguments)]
pub fn percent_slider(
    ui: &Ui,
    painter: &Painter,
    rect: Rect,
    id: Id,
    label: &str,
    value: f64,
    lit: bool,
) -> Option<(f64, bool)> {
    let response = ui.interact(rect, id, Sense::click_and_drag()).on_hover_cursor(egui::CursorIcon::ResizeHorizontal);
    let shown = if value.is_finite() { value.clamp(0.0, 100.0).round() } else { 0.0 };
    let named = format!("{label}: {shown} %");
    response.widget_info(|| {
        let mut info = WidgetInfo::labeled(Role::Slider, true, &named);
        info.value = Some(shown);
        info
    });
    let track = rect.shrink2(Vec2::new(4.0, 0.0));
    slider_face(ui, painter, track, &response, (shown / 100.0) as f32, lit);
    let sent = id.with("sent");
    if response.double_clicked() {
        ui.data_mut(|d| d.remove::<bool>(sent));
        return (shown != 0.0).then_some((0.0, false));
    }
    let mut out = None;
    if (response.clicked() || response.dragged())
        && let Some(pointer) = response.interact_pointer_pos()
        && track.width() > 0.0
    {
        let at = (f64::from((pointer.x - track.left()) / track.width()).clamp(0.0, 1.0) * 100.0).round();
        if (at - shown).abs() >= 0.5 {
            // Anything but a plain click is part of a drag, its frame included where the drag
            // is let go or has not passed egui's threshold yet.
            let dragged = !response.clicked();
            if dragged {
                ui.data_mut(|d| d.insert_temp(sent, true));
            }
            out = Some((at, dragged));
        }
    }
    if response.drag_stopped() || !response.dragged() {
        ui.data_mut(|d| d.remove::<bool>(sent));
    }
    out
}

/// A slider's face along `track`, a horizontal line: its `slider-rail`, the `slider-fill` up to
/// `share` (0 to 1) of the way and the `slider-handle` there, the `background-color` of each.
/// `slider` is entered with `response` (`:hover`, `:active`, `:focus`) and `[checked]` while `lit`;
/// the fill and the handle fade toward the color they have under the pointer, and while the
/// slider has the keys its handle is outlined in the slider's `accent-color`.
pub fn slider_face(ui: &Ui, painter: &Painter, track: Rect, response: &Response, share: f32, lit: bool) {
    let hovered = hover(ui, response);
    let mid = track.center().y;
    let handle_x = track.left() + track.width() * share.clamp(0.0, 1.0);
    // The colors under the pointer are looked up (as comparisons are) before the slider is
    // entered: only `:hover` differs between the two, so a rule for `:active`, `:disabled` or
    // `:focus` reaches its parts as it reaches the slider.
    let (pressed, disabled, focused) =
        (response.is_pointer_button_down_on(), !response.enabled(), response.has_focus());
    let slider = |pointer: bool| SLIDER.pointer(pointer, pressed, disabled).focused(focused).attr(Checked, lit);
    let part_fade = |part: super::element::Name| {
        let rest = part.probe_in(slider(false)).background();
        if hovered > 0.0 { motion::mix(rest, part.probe_in(slider(true)).background(), hovered) } else { rest }
    };
    let (unlit_fill, handle) = (part_fade(SLIDER_FILL), part_fade(SLIDER_HANDLE));
    let el = SLIDER.response(response).attr(Checked, lit).enter();
    let rail = SLIDER_RAIL.look().background();
    painter.line_segment([Pos2::new(track.left(), mid), Pos2::new(track.right(), mid)], Stroke::new(3.0, rail));
    // Lit, the fill is the lit slider's own color whatever the pointer does.
    let fill = if lit { SLIDER_FILL.look().background() } else { unlit_fill };
    painter.line_segment([Pos2::new(track.left(), mid), Pos2::new(handle_x, mid)], Stroke::new(3.0, fill));
    let cap = Rect::from_center_size(Pos2::new(handle_x, mid), Vec2::new(8.0, 12.0));
    painter.rect_filled(cap, 2.0, handle);
    if response.has_focus() {
        painter.rect_stroke(cap.expand(1.0), 2.0, Stroke::new(1.0, el.accent()), egui::StrokeKind::Outside);
    }
}

/// A band's slider, `slider[kind="band"]`, so a theme styles the color picker's Hue apart from a
/// plain slider.
const BAND: &str = "band";

/// A band's height: a row of the color picker's Steps grid.
const BAND_HEIGHT: f32 = 14.0;

/// The hairline round a band and between its blocks: `slider[kind="band"] slider-rail`'s
/// `background-color`.
fn band_rail() -> Color32 {
    SLIDER_RAIL.probe_in(SLIDER.attribute("kind", BAND)).background()
}

/// A color band's face in `track`: a [`BAND_HEIGHT`] band through `stops`, evenly spaced from left to right
/// (two at least for a gradient), under [`band_handle`] at `share` filled with `fill`.
pub fn band_face(
    ui: &Ui,
    painter: &Painter,
    track: Rect,
    response: &Response,
    share: f32,
    stops: &[Color32],
    fill: Color32,
) {
    let rail = band_rail();
    let band = Rect::from_center_size(track.center(), Vec2::new(track.width(), BAND_HEIGHT));
    let mut mesh = egui::Mesh::default();
    let last = stops.len().saturating_sub(1).max(1) as f32;
    for (index, &color) in stops.iter().enumerate() {
        let x = band.left() + band.width() * index as f32 / last;
        mesh.colored_vertex(Pos2::new(x, band.top()), color);
        mesh.colored_vertex(Pos2::new(x, band.bottom()), color);
        if index > 0 {
            let at = (index as u32) * 2;
            mesh.add_triangle(at - 2, at - 1, at);
            mesh.add_triangle(at - 1, at + 1, at);
        }
    }
    painter.add(egui::Shape::mesh(mesh));
    painter.rect_stroke(band, 0.0, theme::hairline(rail), egui::StrokeKind::Outside);
    band_handle(ui, painter, band, response, share, fill);
}

/// A [`band_face`] cut into equal blocks, one of each of `colors` from left to right with the
/// rail's hairline between them.
pub fn stepped_band_face(
    ui: &Ui,
    painter: &Painter,
    track: Rect,
    response: &Response,
    share: f32,
    colors: &[Color32],
    fill: Color32,
) {
    let rail = band_rail();
    let band = Rect::from_center_size(track.center(), Vec2::new(track.width(), BAND_HEIGHT));
    let width = band.width() / colors.len().max(1) as f32;
    for (index, &color) in colors.iter().enumerate() {
        let left = band.left() + width * index as f32;
        painter.rect_filled(
            Rect::from_min_max(Pos2::new(left, band.top()), Pos2::new(left + width, band.bottom())),
            0.0,
            color,
        );
        if index > 0 {
            painter.line_segment([Pos2::new(left, band.top()), Pos2::new(left, band.bottom())], theme::hairline(rail));
        }
    }
    painter.rect_stroke(band, 0.0, theme::hairline(rail), egui::StrokeKind::Outside);
    band_handle(ui, painter, band, response, share, fill);
}

/// The handle of [`band_face`] and [`stepped_band_face`] on `band` at `share`, filled with
/// `fill`: edged in `slider[kind="band"] slider-handle`'s `background-color` (`:hover`'s under
/// the pointer, faded as [`slider_face`] fades it) inside the rail's hairline, and ringed in the
/// slider's `accent-color` while it has the keys.
fn band_handle(ui: &Ui, painter: &Painter, band: Rect, response: &Response, share: f32, fill: Color32) {
    let hovered = hover(ui, response);
    let (pressed, disabled, focused) =
        (response.is_pointer_button_down_on(), !response.enabled(), response.has_focus());
    let slider = |pointer: bool| SLIDER.attribute("kind", BAND).pointer(pointer, pressed, disabled).focused(focused);
    let rest = SLIDER_HANDLE.probe_in(slider(false)).background();
    let edge = if hovered > 0.0 {
        motion::mix(rest, SLIDER_HANDLE.probe_in(slider(true)).background(), hovered)
    } else {
        rest
    };
    let rail = band_rail();
    let el = SLIDER.attribute("kind", BAND).response(response).enter();
    let handle_x = band.left() + band.width() * share.clamp(0.0, 1.0);
    let cap = Rect::from_center_size(Pos2::new(handle_x, band.center().y), Vec2::new(7.0, BAND_HEIGHT + 4.0));
    painter.rect_filled(cap, 3.0, fill);
    painter.rect_stroke(cap, 3.0, Stroke::new(2.0, edge), egui::StrokeKind::Inside);
    painter.rect_stroke(cap, 3.0, theme::hairline(rail), egui::StrokeKind::Outside);
    if focused {
        painter.rect_stroke(cap.expand(2.0), 4.0, Stroke::new(1.0, el.accent()), egui::StrokeKind::Outside);
    }
}

/// A tick box and its words in `rect`, one control named `label`: a `button[kind="check"]`,
/// `[checked]` while `on`, its `border-color` for the box (a step stronger under the pointer)
/// and its `background-color` for the mark; the words a `label[kind="check"]`'s `color`, its
/// `:hover` one under the pointer. The response; a click asks for the other value.
pub fn check_box(ui: &Ui, painter: &Painter, rect: Rect, id: Id, label: &str, on: bool) -> Response {
    let response = ui.interact(rect, id, Sense::click());
    response.widget_info(|| WidgetInfo::selected(Role::CheckBox, ui.is_enabled(), on, label));
    let mark = Rect::from_center_size(Pos2::new(rect.left() + 6.0, rect.center().y), Vec2::splat(11.0));
    let el = BUTTON.attribute("kind", "check").response(&response).attr(Checked, on).enter();
    let edge = if response.has_focus() { el.accent() } else { el.border() };
    painter.rect_stroke(mark, 0.0, theme::hairline(edge), egui::StrokeKind::Inside);
    if on {
        painter.rect_filled(mark.shrink(2.5), 0.0, el.background());
    }
    drop(el);
    let ink = super::element::LABEL
        .attribute("kind", "check")
        .response(&response)
        .enter()
        .hover_fade(hover(ui, &response), Look::color);
    let words = Rect::from_min_max(Pos2::new(mark.right() + 6.0, rect.top()), rect.max);
    text_in(painter, words, Align2::LEFT_CENTER, label, theme::ui_font(theme::TEXT_SMALL), ink);
    response
}

/// One part of a row of choices, a `button` of `kind` (`option`, `preset`), `[checked]` while it
/// is the one chosen: its face painted in `rect` and its fill and ink, which fade with the choice
/// and the pointer.
pub fn option_face(
    ui: &Ui,
    painter: &Painter,
    rect: Rect,
    response: &Response,
    chosen: bool,
    kind: &'static str,
) -> (Color32, Color32) {
    let lit = lit_toggle_kind(ui, response, BUTTON.builder(), &[Checked], chosen, kind);
    let (fill, color) = lit.colors();
    lit.el.paint_over(painter, rect, super::element::Native { radius: 0.0, fill }, lit.toggle.image(lit.lit));
    if response.has_focus() {
        painter.rect_stroke(rect, 0.0, Stroke::new(1.0, lit.el.accent()), egui::StrokeKind::Inside);
    }
    (fill, color)
}

/// A row of choices from the left of `rect`, one [`option_face`] part of `kind` `option` for each
/// of `options` (its words and its value), `widths[i]` wide and one point apart, `[checked]` on
/// `current`'s. Part `i` answers to `id(i)` and is named "`name`: words", selected while chosen.
/// The value clicked, if it is not `current`.
#[allow(clippy::too_many_arguments)]
pub fn option_parts<T: Copy + PartialEq>(
    ui: &Ui,
    painter: &Painter,
    rect: Rect,
    widths: &[f32],
    id: impl Fn(usize) -> Id,
    name: &str,
    options: &[(&str, T)],
    current: T,
) -> Option<T> {
    let mut picked = None;
    let mut x = rect.left();
    for (index, ((text, value), part)) in options.iter().zip(widths).enumerate() {
        let cell = Rect::from_min_size(Pos2::new(x, rect.top()), Vec2::new(*part, rect.height()));
        x += part + 1.0;
        let label = format!("{name}: {text}");
        let response = ui.interact(cell, id(index), Sense::click());
        let chosen = *value == current;
        response.widget_info(|| WidgetInfo::selected(Role::Button, ui.is_enabled(), chosen, &label));
        if response.clicked() && !chosen {
            picked = Some(*value);
        }
        let (_, ink) = option_face(ui, painter, cell, &response, chosen, "option");
        text_in(painter, cell, Align2::CENTER_CENTER, text, theme::ui_font(theme::TEXT_SMALL), ink);
    }
    picked
}

#[cfg(test)]
#[path = "widgets_tests.rs"]
mod tests;
