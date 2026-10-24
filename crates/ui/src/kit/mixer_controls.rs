// Copyright (C) 2026 The Suhina contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! Shared Session and Arrangement channel control faces; interaction belongs to each view.
use super::{motion, theme, widgets};
use crate::kit::surface::Backgrounds as _;
use egui::{Align2, Color32, Pos2, Rect, Vec2};

pub fn pan_field(painter: &egui::Painter, rect: Rect, pan: f64, hover: f32) {
    let to = (0.5 + 0.5 * pan.clamp(-1.0, 1.0)) as f32;
    let span = (0.5f32.min(to), 0.5f32.max(to));
    if crate::kit::skin::has_own("pan.track") || authored_shape() {
        level_bar(painter, rect, "pan", span, hover, &widgets::format_pan(pan), Align2::CENTER_CENTER);
        return;
    }
    // The native look: a flat field with centre ticks and a thin bar along its bottom.
    // `.pan` and `#mixer-pan` colour it as well: the theme's background, border and text.
    crate::kit::native_css::surface(
        painter,
        rect,
        0.0,
        motion::mix(theme::pan_background(), theme::panel_hover(), hover),
    );
    let centre = rect.center().x.round() - 0.5;
    painter.vline(centre, egui::Rangef::new(rect.top(), rect.top() + 2.0), theme::hairline(theme::pan_centre()));
    painter.vline(centre, egui::Rangef::new(rect.bottom() - 2.0, rect.bottom()), theme::hairline(theme::pan_centre()));
    let to = rect.center().x + rect.width() / 2.0 * pan.clamp(-1.0, 1.0) as f32;
    let bar =
        Rect::from_min_max(Pos2::new(centre.min(to), rect.bottom() - 2.0), Pos2::new(centre.max(to), rect.bottom()));
    painter.rect_filled(bar, 0.0, theme::control_selected());
    widgets::text_in(
        painter,
        rect,
        Align2::CENTER_CENTER,
        &widgets::format_pan(pan),
        theme::mono_font(theme::TEXT_SMALL),
        crate::kit::style_class::foreground(theme::text_primary()),
    );
}

/// Whether the sheet gives the element in scope a shape of its own.
fn authored_shape() -> bool {
    crate::kit::style_class::computed().is_some_and(|style| {
        style.authored.background || style.authored.radius || style.authored.border_width || style.height.is_some()
    })
}

/// A level as a bar in `rect`: a track, and over it a fill from `span.0` to `span.1` (fractions of
/// the width), with `text` on top. `kind` names its artwork (`volume.track`, `volume.fill`). The
/// shape comes from the sheet: the element in scope is the track and `.value-fill` the fill
/// (background, border, radius, `height` for the thickness); the native look is a capsule as tall
/// as the field. Artwork of the theme's own replaces either.
pub fn level_bar(
    painter: &egui::Painter,
    rect: Rect,
    kind: &str,
    span: (f32, f32),
    hover: f32,
    text: &str,
    align: Align2,
) {
    use crate::kit::skin::{self, State};
    // A field squeezed below a point (a track being resized, a panel opening) draws nothing:
    // clamping to a range whose top is under its bottom would panic.
    if !rect.is_finite() || rect.height() < 1.0 || rect.width() < 1.0 {
        return;
    }
    let state = if hover > 0.5 { State::Hover } else { State::Normal };
    let height = crate::kit::native_css::thickness(rect.height()).clamp(1.0, rect.height());
    let track = Rect::from_center_size(rect.center(), Vec2::new(rect.width(), height));
    let back = motion::mix(theme::control_background(), theme::control_hover(), hover);
    let radius = if skin::has_own(&format!("{kind}.track"))
        && skin::paint(painter, track, &format!("{kind}.track"), state, back, 0.0)
    {
        0.0
    } else {
        crate::kit::native_css::surface(painter, track, height * 0.5, back)
    };
    let x = |at: f32| track.left() + track.width() * at.clamp(0.0, 1.0);
    let filled = Rect::from_min_max(Pos2::new(x(span.0), track.top()), Pos2::new(x(span.1), track.bottom()));
    let fill = {
        let _fill = crate::kit::style_class::enter("value-fill");
        let fill = crate::kit::style_class::background(theme::control_selected());
        if filled.width() > 0.0 {
            let fill_height = crate::kit::native_css::thickness(height).clamp(1.0, rect.height());
            let bar = Rect::from_center_size(track.center(), Vec2::new(track.width(), fill_height));
            // Clipped rather than shrunk, so the fill keeps the track's rounded ends.
            let clipped = painter
                .with_clip_rect(painter.clip_rect().intersect(Rect::from_x_y_ranges(filled.x_range(), rect.y_range())));
            if !(skin::has_own(&format!("{kind}.fill"))
                && skin::paint(&clipped, bar, &format!("{kind}.fill"), state, fill, 0.0))
            {
                crate::kit::native_css::surface(&clipped, bar, radius, fill);
            }
        }
        fill
    };
    // A handle where the fill ends, when the sheet gives `.value-handle` a width (or artwork
    // `{kind}.handle`): its background, border and radius, as tall as the field.
    {
        let _handle = crate::kit::style_class::enter("value-handle");
        let width = crate::kit::style_class::computed().and_then(|style| style.width);
        let role = format!("{kind}.handle");
        if let Some(width) = width.filter(|w| *w >= 1.0).or_else(|| skin::has_own(&role).then_some(8.0)) {
            // The end that moves: a volume's right end, a pan's end away from the centre.
            let at = if span.0 > 0.0 && span.0 < 0.5 { x(span.0) } else { x(span.1) };
            let handle = Rect::from_center_size(
                Pos2::new(at.clamp(track.left() + width / 2.0, track.right() - width / 2.0), rect.center().y),
                Vec2::new(width.min(rect.width()), crate::kit::native_css::thickness(rect.height()).min(rect.height())),
            );
            if !(skin::has_own(&role) && skin::paint(painter, handle, &role, state, fill, 0.0)) {
                crate::kit::native_css::surface(painter, handle, 2.0, theme::text_primary());
            }
        }
    }
    // The value's size from the sheet (`font-size`).
    let font = theme::mono_font(
        crate::kit::style_class::computed()
            .filter(|style| style.authored.font_size)
            .map_or(theme::TEXT_SMALL, |s| s.font_size),
    );
    // A narrow field (a header at its least width) gives up its inset before its value is cut.
    let inset = ((rect.width() - 32.0) / 2.0).clamp(2.0, 8.0);
    let inner = rect.shrink2(Vec2::new(inset, 0.0));
    // The sheet may move the value (`text-align`) and give the lit part's text its own colour
    // (`.value-fill { color }`).
    let align = match crate::kit::style_class::computed().and_then(|style| style.text_align) {
        Some(-1) => Align2::LEFT_CENTER,
        Some(0) => Align2::CENTER_CENTER,
        Some(_) => Align2::RIGHT_CENTER,
        None => align,
    };
    let lit_ink = {
        let _fill = crate::kit::style_class::enter("value-fill");
        crate::kit::style_class::computed().filter(|style| style.authored.color).map(|style| style.color)
    };
    // The value can cross the fill's edge; each part keeps the ink that reads on what is under it.
    let unfilled = Rect::from_min_max(Pos2::new(filled.right(), rect.top()), rect.max);
    let before = Rect::from_min_max(rect.min, Pos2::new(filled.left(), rect.bottom()));
    for part in [before, unfilled] {
        widgets::text_in(
            &painter.with_clip_rect(painter.clip_rect().intersect(part)),
            inner,
            align,
            text,
            font.clone(),
            crate::kit::style_class::foreground(theme::text_primary()),
        );
    }
    let on_fill = lit_ink.unwrap_or_else(|| {
        if fill.a() > 0 && height >= rect.height() * 0.6 { theme::text_on(fill) } else { theme::text_primary() }
    });
    widgets::text_in(
        &painter.with_clip_rect(painter.clip_rect().intersect(filled.with_min_y(rect.top()).with_max_y(rect.bottom()))),
        inner,
        align,
        text,
        font,
        on_fill,
    );
}

pub fn send_field(
    painter: &egui::Painter,
    rect: Rect,
    letter: &str,
    db: f64,
    pre_fader: bool,
    pointer: Color32,
    position: f32,
) {
    // A labelled send tile with a small level rail, rather than a miniature rotary dial.
    let rail = Rect::from_min_max(
        Pos2::new(rect.left() + 4.0, rect.bottom() - 3.0),
        Pos2::new(rect.right() - 4.0, rect.bottom() - 1.5),
    );
    let quiet = motion::mix(theme::control_background(), pointer, 0.25);
    painter.fill_background(rail, 1.0, quiet);
    let level = Rect::from_min_size(rail.min, Vec2::new(rail.width() * position.clamp(0.0, 1.0), rail.height()));
    if level.width() >= 1.0
        && !super::skin::paint(painter, level, "send.fill", super::skin::State::Normal, pointer, 0.0)
    {
        painter.rect_filled(level, 1.0, motion::mix(quiet, pointer, 0.4));
    }
    // The destination stays legible; a dot denotes pre-fader mode.
    if rect.width() < 40.0 {
        // Session's two-column send grid preserves the narrow channel width.
        let title = Rect::from_min_max(rect.min, Pos2::new(rect.right(), rect.top() + 12.0));
        let value =
            Rect::from_min_max(Pos2::new(rect.left(), title.bottom()), Pos2::new(rect.right(), rect.bottom() - 4.0));
        widgets::text_in(painter, title, Align2::CENTER_CENTER, letter, theme::mono_font(9.0), pointer);
        widgets::text_in(
            painter,
            value,
            Align2::CENTER_CENTER,
            &widgets::format_db(db),
            theme::mono_font(7.0),
            pointer,
        );
    } else {
        let text = rect.translate(Vec2::new(0.0, -1.0));
        widgets::text_in(
            painter,
            text,
            Align2::LEFT_CENTER,
            letter,
            theme::mono_font(theme::TEXT_SMALL),
            theme::text_hint(),
        );
        widgets::text_in(
            painter,
            text,
            Align2::RIGHT_CENTER,
            &widgets::format_db(db),
            theme::mono_font(theme::TEXT_SMALL),
            pointer,
        );
    }
    if pre_fader {
        painter.circle_filled(Pos2::new(rect.center().x, rect.bottom() + 1.5), 1.5, pointer);
    }
}

/// A tactile vertical fader cap. Its centre marker, rather than its edges, indicates the gain.
pub fn fader_cap(painter: &egui::Painter, cap: Rect, hover: f32, state: super::skin::State) {
    let _style = super::style_class::enter("fader-cap");
    super::native_css::background_rounded(painter, cap, 2.0);
    if super::skin::paint(painter, cap, "fader.cap", state, theme::text_value(), 0.0) {
        return;
    }
    let base = motion::mix(theme::control_background(), theme::control_hover(), hover);
    let face = motion::mix(base, theme::text_value(), 0.28);
    let light = motion::mix(face, theme::text_on(face), 0.32);
    let dark = motion::mix(base, Color32::BLACK, 0.55);
    painter.rect_filled(cap.translate(Vec2::new(1.0, 2.0)), 3.0, Color32::from_black_alpha(100));
    painter.rect_filled(cap, 2.0, face);
    let inner = cap.shrink(1.0);
    let shoulder = 5.0;
    painter.add(egui::Shape::convex_polygon(
        vec![
            inner.left_top(),
            inner.right_top(),
            Pos2::new(inner.right() - 2.0, inner.top() + shoulder),
            Pos2::new(inner.left() + 2.0, inner.top() + shoulder),
        ],
        light,
        egui::Stroke::NONE,
    ));
    painter.add(egui::Shape::convex_polygon(
        vec![
            Pos2::new(inner.left() + 2.0, inner.bottom() - shoulder),
            Pos2::new(inner.right() - 2.0, inner.bottom() - shoulder),
            inner.right_bottom(),
            inner.left_bottom(),
        ],
        dark,
        egui::Stroke::NONE,
    ));
    painter.rect_stroke(cap, 2.0, theme::hairline(dark), egui::StrokeKind::Inside);
    let marker = Rect::from_center_size(cap.center(), Vec2::new(cap.width() - 4.0, 5.0));
    painter.rect_filled(marker, 1.0, dark);
    painter.hline(
        marker.shrink(1.0).x_range(),
        cap.center().y.round() + 0.5,
        egui::Stroke::new(2.0, theme::text_on(dark)),
    );
}

/// Read-only track type, based on supported content rather than installed instruments.
#[allow(clippy::too_many_arguments)]
pub fn track_type(
    ui: &egui::Ui,
    painter: &egui::Painter,
    rect: Rect,
    id: egui::Id,
    name: &str,
    content: (bool, bool),
    color: Color32,
) {
    let (notes, audio) = content;
    if super::control_layout::placement("track-type").hidden {
        return;
    }
    let rect = super::control_layout::place("track-type", rect, painter.clip_rect());
    let label = match content {
        (true, true) => "MIDI and audio track",
        (true, false) => "MIDI track",
        (false, true) => "Audio track",
        (false, false) => return,
    };
    let response = ui.interact(rect.intersect(painter.clip_rect()), id, egui::Sense::hover());
    response.widget_info(|| egui::WidgetInfo::labeled(egui::WidgetType::Other, true, format!("{name}: {label}")));
    let state = super::skin::State::response(&response, false);
    response.on_hover_text(label);
    let center = rect.center();
    let stroke = egui::Stroke::new(1.2, color);
    if notes {
        let x = center.x - if audio { 4.0 } else { 0.0 };
        let icon = Rect::from_center_size(Pos2::new(x, center.y), Vec2::splat(if audio { 10.0 } else { 14.0 }));
        if !super::skin::paint(painter, icon, "track.midi", state, color, 0.0) {
            // A compact note remains readable if a custom renderer has no SVG texture.
            painter.line_segment([Pos2::new(x + 2.0, center.y - 5.0), Pos2::new(x + 2.0, center.y + 3.0)], stroke);
            painter.circle_filled(Pos2::new(x, center.y + 3.0), 2.0, color);
        }
    }
    if audio {
        let x = center.x + if notes { 5.0 } else { 0.0 };
        let scale = if notes { 0.6 } else { 1.0 };
        let icon = Rect::from_center_size(Pos2::new(x, center.y), Vec2::splat(if notes { 10.0 } else { 14.0 }));
        if super::skin::paint(painter, icon, "track.audio", state, color, 0.0) {
            return;
        }
        for (dx, height) in [(-4.0, 2.0), (-2.0, 4.0), (0.0, 5.0), (2.0, 3.0), (4.0, 1.5)] {
            painter.line_segment(
                [Pos2::new(x + dx * scale, center.y - height), Pos2::new(x + dx * scale, center.y + height)],
                stroke,
            );
        }
    }
}
