// Copyright (C) 2026 The Suhina contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! What floats over the window: menus, dropdowns and other popups, tooltips and dialogs (#1234).
//!
//! egui builds every popup, menu and tooltip as an area of its own at `Order::Foreground` or
//! `Order::Tooltip`. Each of those layers is a floating surface, styled by `popup` (or `tooltip`),
//! unless the app built it itself: an area of the app's own is made by [`area`] (or [`modal`]),
//! the one place that calls `egui::Area::new` (`egui::Modal::new`), which says what it is (a
//! [`Kind`]). A layer the app only paints on (`Context::layer_painter`, a drag's preview) is not an
//! area, and never taken for one. So a new popup blurs what is behind it without anyone
//! remembering to ask, and nothing of the app's does by mistake.
//!
//! Once the frame is painted, `kit::liquid::finish_backdrops` gives each surface the window
//! blurred under its frame, where its element's `backdrop-filter` asks for it, and paints the
//! frames of popups and tooltips from their elements ([`frame`]).

use std::collections::HashMap;

use egui::{Color32, Context, Id, Painter, Rect, Vec2};
use suhina_style::property::Property;

use super::element::{self, Entered, Name};
use super::theme;

/// What one of the app's own floating layers is.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Kind {
    /// A list or menu of the app's own (the project switcher): a popup like egui's, styled by
    /// `popup`.
    Popup,
    /// A menu of the menu bar, which paints its `menu` element itself: only blurred.
    Menu,
    /// The app's own, neither blurred nor framed here: a dialog (which places its own backdrop
    /// under its box, [`dialog`], not under its layer's first shape, the dimming over the
    /// window), Settings, the splash, an editor laid over a view. A menu over one has it behind.
    Own,
}

const DECLARED: &str = "floating-declared";
/// More layers than any frame has; the rest are taken for popups.
const MAX_DECLARED: usize = 256;

/// Forgets last frame's declarations; called at the start of each frame.
pub fn begin_frame(ctx: &Context) {
    ctx.data_mut(|data| data.remove_temp::<HashMap<Id, Kind>>(Id::unique(DECLARED)));
}

/// Says that the layer with `id` (at any order) is the app's own, and what it is.
pub fn declare(ctx: &Context, id: Id, kind: Kind) {
    ctx.data_mut(|data| {
        let declared = data.get_temp_mut_or_default::<HashMap<Id, Kind>>(Id::unique(DECLARED));
        if declared.len() < MAX_DECLARED || declared.contains_key(&id) {
            declared.insert(id, kind);
        }
    });
}

/// An area of the app's own, declared as `kind`.
pub fn area(ctx: &Context, id: Id, kind: Kind) -> egui::Area {
    declare(ctx, id, kind);
    #[allow(clippy::disallowed_methods)]
    egui::Area::new(id)
}

/// A modal of the app's own (egui builds it an area of its own), declared [`Kind::Own`].
pub fn modal(ctx: &Context, id: Id) -> egui::Modal {
    declare(ctx, id, Kind::Own);
    #[allow(clippy::disallowed_methods)]
    egui::Modal::new(id)
}

/// What the app said the layer with `id` is this frame, if it said.
pub(crate) fn declared(ctx: &Context) -> HashMap<Id, Kind> {
    ctx.data(|data| data.get_temp::<HashMap<Id, Kind>>(Id::unique(DECLARED))).unwrap_or_default()
}

/// How far the area with `id` has faded in, from 0 to 1, as egui works it out when it shows the
/// area (`egui::Area`'s fade-in): the shapes it painted are that much see-through. A layer that
/// is not an area is shown whole.
pub(crate) fn shown(ctx: &Context, id: Id) -> f32 {
    let Some(since) = egui::AreaState::load(ctx, id).and_then(|state| state.last_became_visible_at) else {
        return 1.0;
    };
    let age = ctx.input(|i| (i.time - since) as f32 + i.predicted_dt * 0.5);
    let fade = ctx.global_style().animation_time;
    if !age.is_finite() || fade <= 0.0 {
        return 1.0;
    }
    let t = (age / fade).clamp(0.0, 1.0);
    // egui eases it so: `emath::easing::quadratic_out`.
    -(t * (t - 2.0))
}

/// Whether a computed style blurs what is behind (`backdrop-filter`): yes unless it says `none`
/// or a radius of 0; a style that says nothing blurs.
fn style_blurs(style: Option<super::native_css::Computed>) -> bool {
    style.and_then(|style| style.text(Property::BackdropFilter)).is_none_or(|f| suhina_style::property::blurs(&f))
}

/// Whether surfaces of the element `name` blur the window behind them, from the rules on the
/// element alone (`popup { … }`).
pub(crate) fn blurs(name: Name) -> bool {
    style_blurs(super::native_css::settings_of(&[name]))
}

/// How big a dialog is in `area` that holds `size`: that, and its border and padding around it
/// (#1336), never closer than `PAD` to the window's edges.
pub(crate) fn dialog_size(dialog: &Entered, area: Rect, size: Vec2) -> Vec2 {
    let pad = theme::PAD;
    let [top, right, bottom, left] = dialog.insets();
    let size = size + Vec2::new(left + right, top + bottom);
    Vec2::new(size.x.min(area.width() - 2.0 * pad), size.y.min(area.height() - 2.0 * pad))
}

/// A dialog's box in `rect`: the window blurred under it (unless its `backdrop-filter` is
/// `none`), then its own background over `fill` in CSS's order, with Suhina's hairline round it
/// where the sheet gives it no border. Returns the content box what it holds goes in (#1336).
pub(crate) fn dialog(painter: &Painter, dialog: &Entered, rect: Rect, fill: Color32) -> Rect {
    if style_blurs(dialog.style()) {
        super::liquid::backdrop(painter, rect);
    }
    let content = dialog.paint_with(painter, rect, element::Native { radius: 0.0, fill }).content;
    if !dialog.style().is_some_and(|style| style.authored.border_width) {
        painter.rect_stroke(rect, 0.0, theme::hairline(theme::border_strong()), egui::StrokeKind::Inside);
    }
    content
}

/// The frame egui drew for a popup or tooltip, as the first shape of its layer.
#[derive(Clone, Debug)]
pub(crate) struct Frame {
    pub rect: Rect,
    pub radius: egui::CornerRadius,
    pub fill: Color32,
    pub stroke: egui::Stroke,
    /// What egui painted under it: its shadow.
    pub shadow: Vec<egui::Shape>,
}

/// The frame of a layer: its first shape that paints something, when that is a rectangle (with
/// a shadow, egui paints `[shadow, frame]`). Its index in the layer, and the frame.
pub(crate) fn find_frame(list: &egui::layers::PaintList) -> Option<(usize, Frame)> {
    let (index, first) =
        list.all_entries().enumerate().find(|(_, clipped)| !matches!(clipped.shape, egui::Shape::Noop))?;
    let (shape, shadow) = match &first.shape {
        egui::Shape::Vec(parts) => {
            let (last, before) = parts.split_last()?;
            (last, before.to_vec())
        }
        shape => (shape, Vec::new()),
    };
    match shape {
        egui::Shape::Rect(frame) if frame.rect.is_positive() && frame.rect.is_finite() => Some((
            index,
            Frame { rect: frame.rect, radius: frame.corner_radius, fill: frame.fill, stroke: frame.stroke, shadow },
        )),
        _ => None,
    }
}

/// A frame a sheet gives: painted under the contents in place of egui's, and its outline over
/// them, as CSS paints an outline.
pub(crate) struct Restyled {
    pub radius: f32,
    pub under: Vec<egui::Shape>,
    pub over: Vec<egui::Shape>,
}

/// The frame a sheet gives the element `name` (`popup`, `tooltip`), painted in place of egui's
/// `frame` at `opacity`: its background, border, corners and shadows, egui's own where the sheet
/// sets none. `None` when the sheet styles none of these, and egui's frame stays.
/// The contents keep egui's margins: `padding` does not move them.
pub(crate) fn frame(ctx: &Context, name: Name, frame: &Frame, opacity: f32) -> Option<Restyled> {
    let style = super::native_css::settings_of(&[name])?;
    let authored = style.authored.background || style.authored.border_width || style.authored.radius;
    if !authored && !style.has_shadow() && !style.has_gradient() && style.outline().is_none() {
        return None;
    }
    let scratch = egui::LayerId::new(egui::Order::Debug, Id::unique("floating-frame"));
    let mut painter = ctx.layer_painter(scratch);
    painter.set_opacity(opacity);
    let take = || {
        ctx.graphics_mut(|layers| {
            let list = std::mem::take(layers.entry(scratch));
            list.all_entries().map(|clipped| clipped.shape.clone()).collect::<Vec<_>>()
        })
    };
    // egui scales the frame's colours while the popup fades in: undo that for the fallbacks.
    let unfaded = |color: Color32| {
        if opacity > 0.0 && opacity < 1.0 { color.gamma_multiply(1.0 / opacity) } else { color }
    };
    let radius = super::native_css::box_begin(
        &painter,
        frame.rect,
        frame.radius.average(),
        unfaded(frame.fill),
        Some(&style),
        None,
    );
    if !style.authored.border_width && frame.stroke.width > 0.0 {
        let stroke = egui::Stroke::new(frame.stroke.width, unfaded(frame.stroke.color));
        painter.rect_stroke(frame.rect, radius, stroke, egui::StrokeKind::Inside);
    }
    let mut under = if style.has_shadow() { Vec::new() } else { frame.shadow.clone() };
    under.extend(take());
    super::native_css::box_end(&painter, frame.rect, radius, &style, None);
    Some(Restyled { radius, under, over: take() })
}

#[cfg(test)]
#[path = "floating_tests.rs"]
mod tests;
