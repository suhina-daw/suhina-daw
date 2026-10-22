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
mod tests {
    use super::*;

    #[test]
    #[ignore = "manual performance comparison; run without a simultaneous interactive test"]
    fn measure_short_moving_meter_glow() {
        for glow in [false, true] {
            let ctx = egui::Context::default();
            theme::install(&ctx);
            super::super::skin::configure(&super::super::skin::Skin {
                css: Some(
                    if glow {
                        ".meter-level { box-shadow: 0 0 2px #83d82b55; }"
                    } else {
                        ".meter-level { color: #83d82b; }"
                    }
                    .into(),
                ),
                ..Default::default()
            });
            let mut times = Vec::new();
            let mut uploads = 0;
            for frame in 0..24 {
                let started = std::time::Instant::now();
                let mut output =
                    ctx.run_ui(egui::RawInput { time: Some(frame as f64 * 0.02), ..Default::default() }, |ui| {
                        let _frame = super::super::native_css::frame(ui);
                        for track in 0..20 {
                            let amplitude = 0.0004 + (frame * 20 + track) as f32 * 0.000001;
                            show(
                                ui,
                                ui.painter(),
                                Rect::from_min_size(Pos2::new(20.0 + track as f32 * 20.0, 20.0), Vec2::new(10.0, 38.0)),
                                "test",
                                Source::Track(TrackId(track)),
                                &Level { peak: [amplitude; 2], rms: [amplitude * 0.5; 2], clipped: false },
                                &mut Vec::new(),
                            );
                        }
                    });
                if frame > 3 {
                    times.push(started.elapsed().as_secs_f64() * 1000.0);
                    uploads += output.textures_delta.set.len();
                }
                output.textures_delta.clear();
            }
            times.sort_by(f64::total_cmp);
            eprintln!("short moving meters glow={glow}: median {:.3} ms, uploads {uploads}", times[10]);
        }
    }

    #[test]
    fn intensity_dims_native_ink_without_changing_geometry_or_clip_light() {
        use crate::kit::{native_css, skin};
        for meter in [skin::MeterStyle::Continuous, skin::MeterStyle::Segmented, skin::MeterStyle::Needle] {
            for upright in [true, false] {
                let render = |intensity, css: Option<&str>| {
                    let ctx = egui::Context::default();
                    theme::install(&ctx);
                    skin::configure(&skin::Skin {
                        meter,
                        meter_intensity: intensity,
                        css: css.map(Into::into),
                        ..Default::default()
                    });
                    let mut ink = Vec::new();
                    for frame in 0..3 {
                        let mut output = ctx.run_ui(
                            egui::RawInput { time: Some(frame as f64 * 0.01), ..Default::default() },
                            |ui| {
                                let _frame = native_css::frame(ui);
                                let size = if upright { Vec2::new(12.0, 100.0) } else { Vec2::new(100.0, 12.0) };
                                show(
                                    ui,
                                    ui.painter(),
                                    Rect::from_min_size(Pos2::new(20.0, 20.0), size),
                                    "Main",
                                    Source::Main,
                                    &Level { peak: [0.5; 2], rms: [0.2; 2], clipped: true },
                                    &mut Vec::new(),
                                );
                            },
                        );
                        output.textures_delta.clear();
                        if frame == 2 {
                            ink = output
                                .shapes
                                .into_iter()
                                .filter_map(|shape| match shape.shape {
                                    egui::Shape::Rect(rect) => Some((rect.rect, rect.fill)),
                                    egui::Shape::LineSegment { points, stroke } => {
                                        Some((Rect::from_two_pos(points[0], points[1]), stroke.color))
                                    }
                                    _ => None,
                                })
                                .collect();
                        }
                    }
                    ink
                };
                let full = render(100, None);
                let dim = render(50, None);
                let opacity = render(100, Some(".meter-level { opacity: 0.5; }"));
                assert_eq!(dim, opacity, "native CSS and theme intensity agree");
                let zero = render(0, None);
                assert_eq!(zero, render(100, Some(".meter-level { opacity: 0; }")));
                assert!(zero.iter().any(|(_, color)| *color == theme::recording()));
                assert_eq!(full.len(), dim.len());
                let mut changed = 0;
                for ((bounds, color), (dim_bounds, dim_color)) in full.iter().zip(&dim) {
                    assert_eq!(bounds, dim_bounds);
                    if color != dim_color {
                        changed += 1;
                        assert_eq!(*dim_color, color.gamma_multiply(0.5));
                    }
                }
                assert!(changed >= 2, "both channels dim");
                assert!(dim.iter().any(|(_, color)| *color == theme::recording()), "clip warning stays visible");
                if meter != skin::MeterStyle::Needle {
                    let transparent = render(100, Some(".meter-level { background-color: transparent; }"));
                    assert!(
                        !transparent.iter().any(|(_, color)| *color == theme::meter_normal().gamma_multiply(0.45)),
                        "transparent fill must not revive native peak ink"
                    );
                }
            }
        }
        skin::configure(&skin::Skin::default());
    }

    #[test]
    fn css_meter_ramps_keep_the_full_scale_anchor_when_signal_and_orientation_change() {
        use crate::kit::{native_css, skin};
        for upright in [true, false] {
            let ctx = egui::Context::default();
            theme::install(&ctx);
            skin::configure(&skin::Skin { meter_intensity: 50, css: Some(".meter-vertical .meter-level { background: linear-gradient(to top, #00ff00, #ffff00 80%, #ff0000); } .meter-horizontal .meter-level { background: linear-gradient(to right, #00ff00, #ffff00 80%, #ff0000); }".into()), ..Default::default() });
            let mut observed = Vec::new();
            for frame in 0..4 {
                let peak = if frame == 3 { 1.0 } else { 0.1 };
                let mut output =
                    ctx.run_ui(egui::RawInput { time: Some(frame as f64 * 0.01), ..Default::default() }, |ui| {
                        let _frame = native_css::frame(ui);
                        let size = if upright { egui::vec2(12.0, 100.0) } else { egui::vec2(100.0, 12.0) };
                        show(
                            ui,
                            ui.painter(),
                            Rect::from_min_size(egui::pos2(20.0, 20.0), size),
                            "Main",
                            Source::Main,
                            &Level { peak: [peak; 2], rms: [peak * 0.5; 2], ..Default::default() },
                            &mut Vec::new(),
                        );
                    });
                if frame >= 2 {
                    observed.push(output.shapes.iter().find_map(|shape| match &shape.shape {
                        egui::Shape::Mesh(mesh) => {
                            Some((mesh.texture_id, mesh.calc_bounds(), shape.clip_rect, mesh.vertices[0].color.a()))
                        }
                        _ => None,
                    }));
                }
                output.textures_delta.clear();
            }
            let low = observed[0].as_ref().expect("quiet gradient");
            let high = observed[1].as_ref().expect("loud gradient");
            assert_eq!((low.0, low.1), (high.0, high.1), "material must not move or stretch with signal");
            assert!((55..=58).contains(&low.3), "native dim peak alpha combines with theme intensity");
            if upright {
                assert!(high.2.top() < low.2.top());
            } else {
                assert!(high.2.right() > low.2.right());
            }
        }
        skin::configure(&skin::Skin::default());
    }

    #[test]
    fn custom_frames_follow_signal_and_keep_native_clip_reset() {
        let ctx = egui::Context::default();
        theme::install(&ctx);
        let mut skin = super::super::skin::Skin {
            meter: super::super::skin::MeterStyle::Frames,
            meter_intensity: 50,
            ..Default::default()
        };
        for (index, color) in [(0, "#ff0000"), (1, "#00ff00")] {
            skin.assets.insert(format!("meter.frame-{index}"), format!(r##"<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 10 10"><rect width="10" height="10" fill="{color}"/></svg>"##));
        }
        super::super::skin::configure(&skin);
        let pos = egui::pos2(30.0, 30.0);
        let mut actions = Vec::new();
        for frame in 0..4 {
            let events = match frame {
                2 | 3 => vec![egui::Event::PointerButton {
                    pos,
                    button: egui::PointerButton::Primary,
                    pressed: frame == 2,
                    modifiers: egui::Modifiers::default(),
                }],
                _ => vec![egui::Event::PointerMoved(pos)],
            };
            let mut output =
                ctx.run_ui(egui::RawInput { events, time: Some(frame as f64 * 0.1), ..Default::default() }, |ui| {
                    show(
                        ui,
                        ui.painter(),
                        Rect::from_min_size(egui::pos2(20.0, 20.0), egui::vec2(100.0, 40.0)),
                        "Main",
                        Source::Main,
                        &Level {
                            peak: if frame == 0 { [0.0, 0.0] } else { [0.0, 4.0] },
                            clipped: true,
                            ..Default::default()
                        },
                        &mut actions,
                    );
                });
            if frame < 2 {
                let expected = if frame == 0 { egui::Color32::RED } else { egui::Color32::GREEN };
                let found = output.textures_delta.set.values().flatten().any(|delta| match &delta.image {
                    egui::ImageData::Color(image) => image.pixels.iter().all(|pixel| *pixel == expected),
                });
                output.textures_delta.clear();
                assert!(found, "missing signal frame {frame}");
                assert!(output.shapes.iter().any(|shape| matches!(&shape.shape,
                    egui::Shape::Mesh(mesh) if mesh.vertices.iter().all(|vertex| (127..=128).contains(&vertex.color.a()))
                )), "frame artwork respects intensity");
            }
        }
        assert_eq!(actions, vec![Action::new("mixer.resetClip", json!({"main": true}))]);
    }

    #[test]
    fn silent_rails_survive_parent_gradients_and_accept_their_own_background() {
        use crate::kit::{native_css, skin, style_class};
        for override_color in [None, Some("#081014")] {
            let ctx = egui::Context::default();
            theme::install(&ctx);
            let native = theme::palette().window_background;
            let css = format!(
                ".session {{ background: linear-gradient(red, blue); }} {}",
                override_color.map_or(String::new(), |color| format!(".meter {{ background-color: {color}; }}"))
            );
            skin::configure(&skin::Skin { css: Some(css.into()), ..Default::default() });
            for frame in 0..3 {
                let mut output =
                    ctx.run_ui(egui::RawInput { time: Some(frame as f64 * 0.01), ..Default::default() }, |ui| {
                        let _frame = native_css::frame(ui);
                        let _parent = style_class::enter("session");
                        show(
                            ui,
                            ui.painter(),
                            Rect::from_min_size(Pos2::new(20.0, 20.0), Vec2::new(WIDTH, 100.0)),
                            "Main",
                            Source::Main,
                            &Level::default(),
                            &mut Vec::new(),
                        );
                    });
                if frame == 2 {
                    let expected = override_color.and_then(theme::parse_hex).unwrap_or(native);
                    let rails = output
                        .shapes
                        .iter()
                        .filter(|shape| matches!(&shape.shape, egui::Shape::Rect(rect) if rect.fill == expected));
                    assert_eq!(rails.count(), 3, "two silent rails and the unlit clip light");
                }
                output.textures_delta.clear();
            }
            skin::configure(&skin::Skin::default());
        }
    }

    #[test]
    fn the_scale_is_the_volume_controls() {
        assert_eq!(fraction(0.0), 0.0);
        assert_eq!(fraction(f32::NAN), 0.0);
        assert!((fraction(1.0) - 70.0 / 76.0).abs() < 1e-6);
        assert_eq!(fraction(4.0), 1.0);
        // -3 dB is where the top 12 % begin.
        let hot = fraction(10f32.powf(-3.0 / 20.0));
        assert!((hot - 0.88).abs() < 0.005, "{hot}");
    }

    #[test]
    fn the_description_names_the_louder_side_and_the_clip() {
        let level = Level { peak: [0.25, 0.5], rms: [0.1, 0.1], clipped: false };
        assert_eq!(describe("Drums", &level), "Drums level: -6.0 dB");
        assert_eq!(describe("Main", &Level { clipped: true, ..Level::default() }), "Main level: silent, clipped");
    }
}
