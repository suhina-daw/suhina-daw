// Copyright (C) 2026 The Suhina contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! Level meters (`docs/spec/level-meters.md`, sizes in `docs/design/components.md` › Meter):
//! the engine's peak and RMS levels of a track or the main output as two bars, one per side,
//! on the volume control's dB scale, with the highest peak of the last moments held as a line
//! and a clip light that a click turns off (`mixer.resetClip`).

use crate::kit::surface::Backgrounds as _;
use egui::{Id, Pos2, Rect, Sense, Ui, Vec2};
use serde_json::json;
use suhina_engine::model::TrackId;
use suhina_engine::{Level, MeterMonitor};

use crate::kit::theme;
use crate::kit::widgets::Action;

/// The scale: the volume control's range, in dB, bottom to top.
pub const RANGE: (f64, f64) = (-70.0, 6.0);
/// Above this the bar turns `meter.hot`: the top 12 % of the scale.
const HOT_DB: f64 = -3.0;
/// How long the highest peak stays shown before it follows the level down, in seconds.
const HOLD_SECONDS: f64 = 1.5;
/// A bar's width, the gap between the two, and the clip light's height with its gap.
pub const BAR: f32 = 4.0;
const GAP: f32 = 2.0;
const CLIP: f32 = 3.0;
/// The width of a meter standing up, both bars and the gap.
pub const WIDTH: f32 = BAR * 2.0 + GAP;

/// What the meters show this frame, read from the playing engine.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct Meters {
    pub main: Level,
    pub tracks: Vec<(TrackId, Level)>,
}

impl Meters {
    /// The levels `monitor` holds now; nothing without an engine.
    pub fn read(monitor: Option<&MeterMonitor>) -> Self {
        monitor.map_or_else(Self::default, |m| Self { main: m.main(), tracks: m.tracks() })
    }

    pub fn track(&self, track: TrackId) -> Level {
        self.tracks.iter().find(|(t, _)| *t == track).map(|(_, l)| *l).unwrap_or_default()
    }

    /// Whether any meter shows a level, so the window keeps drawing while they fall: one below
    /// the scale's bottom shows nothing.
    pub fn moving(&self) -> bool {
        let lit = |l: &Level| l.peak.iter().any(|&p| shown_db(p).is_some());
        lit(&self.main) || self.tracks.iter().any(|(_, l)| lit(l))
    }

    /// Whether `other` draws the same meters: the same levels to the scale's step, the same clip
    /// lights. A plug-in's noise or a reverb's tail far below the scale changed the levels every
    /// block, and the window drew every frame for it, which held up an embedded plug-in
    /// editor's input (it shares the window's input queue).
    pub fn shows_same(&self, other: &Self) -> bool {
        let same = |a: &Level, b: &Level| {
            a.clipped == b.clipped
                && a.peak.iter().chain(&a.rms).zip(b.peak.iter().chain(&b.rms)).all(|(x, y)| step(*x) == step(*y))
        };
        same(&self.main, &other.main)
            && self.tracks.len() == other.tracks.len()
            && self.tracks.iter().zip(&other.tracks).all(|((ta, a), (tb, b))| ta == tb && same(a, b))
    }
}

/// A level in dB on the scale, `None` below its bottom (nothing drawn).
fn shown_db(level: f32) -> Option<f64> {
    let db = 20.0 * f64::from(level.max(0.0)).log10();
    (db.is_finite() && db >= RANGE.0).then_some(db)
}

/// A level as the meter draws it, to a quarter of a dB (finer than a point on any meter's
/// height); everything below the scale is one step.
fn step(level: f32) -> Option<i64> {
    shown_db(level).map(|db| (db * 4.0).round() as i64)
}

/// Whose meter it is: the clip light's click resets this one.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Source {
    Main,
    Track(TrackId),
}

/// Where a level falls on the scale, 0 … 1.
pub fn fraction(level: f32) -> f32 {
    let db = 20.0 * f64::from(level).log10();
    if !db.is_finite() {
        return 0.0;
    }
    ((db - RANGE.0) / (RANGE.1 - RANGE.0)).clamp(0.0, 1.0) as f32
}

/// The held peak: the highest of the last `HOLD_SECONDS`, per side.
#[derive(Debug, Clone, Copy, Default)]
struct Hold {
    peak: [f32; 2],
    since: [f64; 2],
}

fn held(ui: &Ui, id: Id, level: &Level) -> [f32; 2] {
    let now = ui.input(|i| i.time);
    ui.ctx().data_mut(|d| {
        let hold = d.get_temp_mut_or_default::<Hold>(id);
        for side in 0..2 {
            let (Some(peak), Some(since), Some(&now_peak)) =
                (hold.peak.get_mut(side), hold.since.get_mut(side), level.peak.get(side))
            else {
                continue;
            };
            if now_peak >= *peak || now - *since > HOLD_SECONDS {
                *peak = now_peak;
                *since = now;
            }
        }
        hold.peak
    })
}

/// Text for screen readers and tests: the louder side's peak, and the clip light.
fn describe(name: &str, level: &Level) -> String {
    let peak = level.peak[0].max(level.peak[1]);
    let db = 20.0 * f64::from(peak).log10();
    let value = if db.is_finite() && db > RANGE.0 { format!("{db:.1} dB") } else { "silent".into() };
    let clipped = if level.clipped { ", clipped" } else { "" };
    format!("{name} level: {value}{clipped}")
}

/// Draws a meter of `level` in `rect`: standing up (bars bottom to top, the clip light on top)
/// when `rect` is taller than wide, lying down (left to right, the light on the right)
/// otherwise. A click on it turns the clip light off.
pub fn show(
    ui: &Ui,
    painter: &egui::Painter,
    rect: Rect,
    name: &str,
    source: Source,
    level: &Level,
    actions: &mut Vec<Action>,
) {
    if rect.width() <= 0.0 || rect.height() <= 0.0 || !rect.intersects(painter.clip_rect()) {
        return;
    }
    let id = Id::new(("meter", source));
    let response = ui.interact(rect.intersect(painter.clip_rect()), id, Sense::click());
    let _style = crate::kit::style_class::enter_response("meter", &response);
    // A surrounding CSS gradient has a transparent background-color. The empty
    // rails must retain their native ink unless the meter itself overrides it.
    let background = crate::kit::style_class::background(ui.visuals().extreme_bg_color);
    super::native_css::background(painter, rect);
    response.widget_info(|| egui::WidgetInfo::labeled(egui::WidgetType::Button, true, describe(name, level)));
    if response.clicked() && level.clipped {
        actions.push(match source {
            Source::Main => Action::new("mixer.resetClip", json!({ "main": true })),
            Source::Track(track) => Action::new("mixer.resetClip", json!({ "track": track.0 })),
        });
    }
    let hold = held(ui, id, level);
    let skin = super::skin::current();
    let upright = rect.height() > rect.width();
    let _orientation = super::style_class::enter(if upright { "meter-vertical" } else { "meter-horizontal" });
    let (light, bars) = if upright {
        let light = Rect::from_min_size(rect.min, Vec2::new(rect.width(), CLIP));
        (light, Rect::from_min_max(Pos2::new(rect.left(), light.bottom() + 1.0), rect.max))
    } else {
        let light = Rect::from_min_max(Pos2::new(rect.right() - CLIP, rect.top()), rect.max);
        (light, Rect::from_min_max(rect.min, Pos2::new(light.left() - 1.0, rect.bottom())))
    };
    painter.fill_background(light, 0.0, if level.clipped { theme::recording() } else { background });
    // Intensity changes only measured ink, never rails or the safety indicator.
    let mut ink = painter.clone();
    ink.multiply_opacity(f32::from(skin.meter_intensity.min(100)) / 100.0);
    if skin.meter == super::skin::MeterStyle::Frames {
        let peak = fraction(level.peak.iter().copied().fold(0.0, f32::max));
        let state = if response.is_pointer_button_down_on() {
            super::skin::State::Active
        } else if response.hovered() {
            super::skin::State::Hover
        } else {
            super::skin::State::Normal
        };
        if super::skin::meter_frame(&ink, bars, state, peak) {
            return;
        }
    }
    let hot_from = ((HOT_DB - RANGE.0) / (RANGE.1 - RANGE.0)) as f32;
    let thickness = if upright { bars.width() } else { bars.height() };
    let bar_width = ((thickness - GAP) / 2.0).max(0.0);
    for side in 0..2 {
        let offset = side as f32 * (bar_width + GAP);
        // The bar's track, and a part of it from 0 to `to` (0 … 1).
        let bar = if upright {
            Rect::from_min_size(Pos2::new(bars.left() + offset, bars.top()), Vec2::new(bar_width, bars.height()))
        } else {
            Rect::from_min_size(Pos2::new(bars.left(), bars.top() + offset), Vec2::new(bars.width(), bar_width))
        };
        let part = |from: f32, to: f32| {
            if upright {
                Rect::from_x_y_ranges(
                    bar.x_range(),
                    egui::Rangef::new(bar.bottom() - bar.height() * to, bar.bottom() - bar.height() * from),
                )
            } else {
                Rect::from_x_y_ranges(
                    egui::Rangef::new(bar.left() + bar.width() * from, bar.left() + bar.width() * to),
                    bar.y_range(),
                )
            }
        };
        painter.fill_background(bar, 0.0, background);
        let _level = super::style_class::enter("meter-level");
        let opacity = super::style_class::computed().map_or(1.0, |style| style.opacity);
        let mut native_ink = ink.clone();
        native_ink.multiply_opacity(opacity);
        if skin.meter == super::skin::MeterStyle::Needle {
            let origin = if upright { bar.center_bottom() } else { bar.left_center() };
            for (value, width) in [
                (level.peak.get(side).copied().unwrap_or(0.0), 1.0),
                (level.rms.get(side).copied().unwrap_or(0.0), 2.0),
            ] {
                let value = fraction(value);
                let to = egui::pos2(bar.left() + bar.width() * value, bar.bottom() - bar.height() * value);
                let color = if value > hot_from { theme::meter_hot() } else { theme::meter_normal() };
                native_ink.line_segment([origin, to], egui::Stroke::new(width, color));
            }
            continue;
        }
        let peak = fraction(level.peak.get(side).copied().unwrap_or(0.0));
        let rms = fraction(level.rms.get(side).copied().unwrap_or(0.0));
        if peak > 0.0 {
            super::native_css::level_glow(&ink, part(0.0, peak));
        }
        // The peak dimmer, the RMS level in the full colour over it.
        for (to, alpha) in [(peak, 0.45), (rms, 1.0)] {
            if to <= 0.0 {
                continue;
            }
            let fill = |from: f32, to: f32, color: egui::Color32| {
                let lit = part(from, to);
                let mut clipped = ink.with_clip_rect(painter.clip_rect().intersect(lit));
                clipped.multiply_opacity(alpha);
                if !super::native_css::level_fill(&clipped, bar) {
                    native_ink.rect_filled(lit, 0.0, color.gamma_multiply(alpha));
                }
            };
            if skin.meter == super::skin::MeterStyle::Segmented {
                let count = skin.segments.clamp(4, 64);
                for segment in 0..count {
                    let from = f32::from(segment) / f32::from(count);
                    if from >= to {
                        break;
                    }
                    let end = ((f32::from(segment) + 0.8) / f32::from(count)).min(to);
                    let color = if from >= hot_from { theme::meter_hot() } else { theme::meter_normal() };
                    fill(from, end, color);
                }
            } else {
                fill(0.0, to.min(hot_from), theme::meter_normal());
                if to > hot_from {
                    fill(hot_from, to, theme::meter_hot());
                }
            }
        }
        let hold = fraction(hold.get(side).copied().unwrap_or(0.0));
        if hold > 0.0 {
            let color = if hold > hot_from { theme::meter_hot() } else { theme::meter_normal() };
            let line = if upright {
                let y = (bar.bottom() - bar.height() * hold).max(bar.top());
                Rect::from_x_y_ranges(bar.x_range(), egui::Rangef::new(y, y + 1.0))
            } else {
                let x = (bar.left() + bar.width() * hold).min(bar.right());
                Rect::from_x_y_ranges(egui::Rangef::new(x - 1.0, x), bar.y_range())
            };
            native_ink.rect_filled(line, 0.0, color);
        }
    }
}

#[cfg(test)]
#[path = "meter_tests.rs"]
mod tests;
