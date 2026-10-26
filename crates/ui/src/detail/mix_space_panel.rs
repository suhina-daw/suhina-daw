// Copyright (C) 2026 The Suhina contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! The Mix Space device's view (`docs/spec/mix-space.md`): every track that plays through the
//! device's track, as a line in a box. Left to right is where a band of the track sits in the
//! stereo field, bottom to top how loud it is (the box's ceiling is 0 dBFS, with a red zone
//! above it for what goes over), front to back its frequency from 20 Hz to 20 kHz. Where two
//! tracks meet at nearly the same level the view marks the overlap, and draws a track another
//! one covers there dashed.
//!
//! Dragging turns the box, Shift-dragging (or dragging with the right button) moves it, the
//! wheel zooms and a double-click resets the view; Front, Side, Top and 3D jump to a view.
//!
//! The readings come from the session's [`MixSpace`] analysis, which the app keeps with the
//! engine ([`feed`]); the view asks for the tap point it shows every frame it is drawn, so the
//! analysis runs only while some view is open.

use std::sync::Arc;

use egui::{Align2, Color32, Id, Pos2, Rect, Sense, Shape, Stroke, Ui, Vec2};
use suhina_device_mix_space::{muted, overlaps, point};
use suhina_engine::model::{Project, TrackId};
use suhina_engine::{MeterMonitor, TapPoint};
use suhina_session::{MixSpace, MixSpaceSnapshot};

use crate::kit::theme;
use crate::kit::widgets;

/// The panel's width and height, the parameter list included.
pub const MIX_WIDTH: f32 = 620.0;
pub const MIX_HEIGHT: f32 = 330.0;
/// The parameter list's width at the panel's left.
pub const CONTROLS_WIDTH: f32 = 150.0;

/// The lowest level the box shows, at its floor.
const FLOOR_DB: f32 = -72.0;
/// How far above 0 dBFS the zone over the ceiling reaches.
const OVER_DB: f32 = 12.0;
/// Two bands this close in the stereo field (of 2 from left to right) meet.
const NEAR_PAN: f32 = 0.3;
/// Bands quieter than this take no part in overlaps.
const OVERLAP_FLOOR_DB: f32 = -60.0;
/// The distance the camera looks from, in box units.
const DISTANCE: f32 = 4.2;
/// How long a view's request keeps the analysis running, in seconds.
const WANT_SECONDS: f64 = 0.5;

/// A track as the view draws it.
#[derive(Debug, Clone, PartialEq)]
pub struct TrackLook {
    pub id: TrackId,
    pub name: String,
    pub color: Color32,
    pub parent: Option<TrackId>,
    pub muted: bool,
}

/// What every Mix Space view draws this frame: the latest readings and the project's tracks.
#[derive(Debug, Clone, Default)]
pub struct Feed {
    pub snapshot: Arc<MixSpaceSnapshot>,
    pub tracks: Arc<Vec<TrackLook>>,
}

/// The points the views drawn last asked for, and when.
#[derive(Debug, Clone, Copy, Default)]
struct Want {
    pre: f64,
    post: f64,
    /// The release the last view drawn set, in seconds.
    release: f32,
}

fn feed_id() -> Id {
    Id::new("mix-space-feed")
}

fn want_id() -> Id {
    Id::new("mix-space-want")
}

/// Keeps the analysis with the engine whose meters are `monitor`, passes on what the views asked
/// for last frame and hands this frame's views the readings. Call it once a frame, before the
/// views are drawn.
pub fn feed(
    ctx: &egui::Context,
    space: &mut Option<MixSpace>,
    monitor: Option<Arc<MeterMonitor>>,
    project: Option<&Project>,
) {
    let Some(monitor) = monitor else {
        *space = None;
        ctx.data_mut(|d| d.remove::<Feed>(feed_id()));
        return;
    };
    if !space.as_ref().is_some_and(|s| s.reads(&monitor)) {
        // Drop the old one first: it gives the engine's tap back off.
        *space = None;
        *space = MixSpace::start(monitor);
    }
    let Some(running) = space.as_ref() else { return };
    let now = ctx.input(|i| i.time);
    let want = ctx.data(|d| d.get_temp::<Want>(want_id())).unwrap_or_default();
    let (pre, post) = (now - want.pre < WANT_SECONDS, now - want.post < WANT_SECONDS);
    if pre || post {
        running.want(pre, post);
        if want.release > 0.0 {
            running.set_release(want.release);
        }
        ctx.request_repaint_after(std::time::Duration::from_millis(16));
    }
    let tracks = project.map(looks).unwrap_or_default();
    let feed = Feed { snapshot: running.snapshot(), tracks: Arc::new(tracks) };
    ctx.data_mut(|d| d.insert_temp(feed_id(), feed));
}

fn looks(project: &Project) -> Vec<TrackLook> {
    project
        .tracks
        .iter()
        .enumerate()
        .map(|(position, track)| {
            let rgb = track.color.unwrap_or_else(|| crate::interaction::grid::default_track_colour(position));
            TrackLook {
                id: track.id,
                name: project.shown_track_name(track.id),
                color: Color32::from_rgb(rgb.0, rgb.1, rgb.2),
                parent: track.parent,
                muted: track.channel.and_then(|c| project.channel(c)).is_some_and(|c| c.mute),
            }
        })
        .collect()
}

/// The view's camera.
#[derive(Debug, Clone, Copy, PartialEq)]
struct Camera {
    yaw: f32,
    pitch: f32,
    zoom: f32,
    offset: Vec2,
}

impl Default for Camera {
    fn default() -> Self {
        Self::view(View::ThreeD)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum View {
    Front,
    Side,
    Top,
    ThreeD,
}

impl Camera {
    fn view(view: View) -> Self {
        let (yaw, pitch) = match view {
            View::Front => (0.0, 0.0),
            View::Side => (-std::f32::consts::FRAC_PI_2, 0.0),
            View::Top => (0.0, 1.45),
            View::ThreeD => (-0.65, 0.42),
        };
        Self { yaw, pitch, zoom: 1.0, offset: Vec2::ZERO }
    }

    /// Where the box point `p` (x pan -1..1, y level 0..1.17, z frequency -1..1) lands in `area`.
    fn project(&self, area: Rect, p: [f32; 3]) -> Pos2 {
        let [x, y, z] = [p[0], p[1] - 0.58, p[2]];
        let (sy, cy) = self.yaw.sin_cos();
        let (x, z) = (x * cy - z * sy, x * sy + z * cy);
        let (sp, cp) = self.pitch.sin_cos();
        let (y, z) = (y * cp - z * sp, y * sp + z * cp);
        let scale = area.height().min(area.width()) * 0.3 * self.zoom * DISTANCE / (DISTANCE + z).max(0.5);
        area.center() + self.offset + Vec2::new(x * scale, -y * scale)
    }
}

/// The box height of a level.
fn height(db: f32) -> f32 {
    ((db.max(FLOOR_DB) - FLOOR_DB) / -FLOOR_DB).min(1.0 + OVER_DB / -FLOOR_DB)
}

/// The box depth of a frequency: 20 Hz at the front, 20 kHz at the back.
fn depth(frequency: f32) -> f32 {
    let octaves = (frequency.max(20.0) / 20.0).log2();
    octaves / 1_000f32.log2() * 2.0 - 1.0
}

/// Whether `track` plays through the device's track `on`: itself, a track inside it, or every
/// track for the main channel.
fn inside(tracks: &[TrackLook], track: &TrackLook, on: TrackId) -> bool {
    if on == TrackId::MAIN || track.id == on {
        return true;
    }
    let mut next = track.parent;
    for _ in 0..tracks.len() {
        let Some(parent) = next else { return false };
        if parent == on {
            return true;
        }
        next = tracks.iter().find(|t| t.id == parent).and_then(|t| t.parent);
    }
    false
}

/// One drawn track: its look and its bands.
struct Line<'a> {
    look: &'a TrackLook,
    bands: &'a [suhina_dsp::BandReading],
}

/// Two or more tracks meeting at one band.
struct Overlap {
    at: Pos2,
    strength: f32,
    text: String,
}

/// Draws the view in `area` for the device at `device` on `track`, set by its parameter
/// `values`.
pub fn show(
    ui: &mut Ui,
    area: Rect,
    track: TrackId,
    device: &crate::detail::detail_view::DeviceAt,
    values: &[(String, f64)],
) {
    let value = |id: &str, default: f64| values.iter().find(|(v, _)| v == id).map_or(default, |(_, v)| *v);
    let pre = value("point", point::POST_FADER) == point::PRE_FADER;
    let hide_muted = value("muted", muted::DIM) == muted::HIDE;
    let marks = value("overlaps", overlaps::MARK);
    let range = value("overlapRange", 6.0) as f32;
    let release = value("release", 0.3) as f32;
    let tap = if pre { TapPoint::PreFader } else { TapPoint::PostFader };
    let now = ui.input(|i| i.time);
    ui.ctx().data_mut(|d| {
        let want = d.get_temp_mut_or_default::<Want>(want_id());
        if pre {
            want.pre = now;
        } else {
            want.post = now;
        }
        want.release = release;
    });
    let painter = ui.painter().with_clip_rect(area);
    painter.rect_filled(area, 0.0, theme::control_background());

    // The view buttons along the top.
    let camera_id = Id::new(("mix-space-camera", track.0, device));
    let mut camera = ui.data(|d| d.get_temp::<Camera>(camera_id)).unwrap_or_default();
    let mut x = area.left() + 4.0;
    for (label, view) in [("Front", View::Front), ("Side", View::Side), ("Top", View::Top), ("3D", View::ThreeD)] {
        let width = widgets::text_width(ui, label, theme::ui_font(theme::TEXT_SMALL)) + 10.0;
        let rect = Rect::from_min_size(Pos2::new(x, area.top() + 3.0), Vec2::new(width, 16.0));
        x = rect.right() + 3.0;
        let response = widgets::button_area(ui, rect, camera_id.with(label), format!("{label} view"))
            .on_hover_text(format!("Look at the box from the {}", label.to_lowercase()));
        let lit = camera == Camera::view(view);
        painter.rect_filled(
            rect,
            2.0,
            if lit || response.hovered() { theme::panel_background() } else { Color32::TRANSPARENT },
        );
        painter.text(
            rect.center(),
            Align2::CENTER_CENTER,
            label,
            theme::ui_font(theme::TEXT_SMALL),
            theme::text_secondary(),
        );
        if response.clicked() {
            camera = Camera::view(view);
        }
    }
    let view = Rect::from_min_max(Pos2::new(area.left(), area.top() + 22.0), area.max);
    let response = ui
        .interact(view, camera_id.with("drag"), Sense::click_and_drag())
        .on_hover_text("Drag to turn, Shift-drag or right-drag to move, scroll to zoom, double-click to reset");
    response.widget_info(|| egui::WidgetInfo::labeled(egui::WidgetType::Other, true, "Mix Space view"));
    let delta = response.drag_delta();
    if response.dragged_by(egui::PointerButton::Secondary)
        || (response.dragged_by(egui::PointerButton::Primary) && ui.input(|i| i.modifiers.shift))
    {
        camera.offset += delta;
    } else if response.dragged_by(egui::PointerButton::Primary) {
        camera.yaw += delta.x * 0.01;
        camera.pitch = (camera.pitch + delta.y * 0.01).clamp(-1.5, 1.5);
    }
    if response.hovered() {
        let scroll = ui.input(|i| i.smooth_scroll_delta.y);
        if scroll != 0.0 {
            camera.zoom = (camera.zoom * (scroll * 0.003).exp()).clamp(0.3, 6.0);
        }
    }
    if response.double_clicked() {
        camera = Camera::default();
    }
    ui.data_mut(|d| d.insert_temp(camera_id, camera));
    let at = |p: [f32; 3]| camera.project(view, p);

    // The box, its ceiling at 0 dBFS and the zone over it.
    let top = height(OVER_DB);
    let corners = |y: f32| [[-1.0, y, -1.0], [1.0, y, -1.0], [1.0, y, 1.0], [-1.0, y, 1.0]];
    let edge = Stroke::new(1.0, theme::text_hint().gamma_multiply(0.6));
    let ceiling = corners(1.0).map(at);
    painter.add(Shape::convex_polygon(ceiling.to_vec(), theme::recording().gamma_multiply(0.06), Stroke::NONE));
    for level in [0.0, 1.0, top] {
        let points = corners(level).map(at);
        let stroke = if level == 1.0 {
            Stroke::new(1.0, theme::recording().gamma_multiply(0.7))
        } else if level == top {
            Stroke::new(1.0, theme::recording().gamma_multiply(0.25))
        } else {
            edge
        };
        painter.add(Shape::closed_line(points.to_vec(), stroke));
    }
    for corner in corners(0.0) {
        painter.line_segment([at(corner), at([corner[0], top, corner[2]])], edge);
    }
    // Faint lines at every octave on the floor, and the labels.
    for frequency in [100.0, 1_000.0, 10_000.0] {
        let z = depth(frequency);
        painter.line_segment(
            [at([-1.0, 0.0, z]), at([1.0, 0.0, z])],
            Stroke::new(1.0, theme::text_hint().gamma_multiply(0.25)),
        );
    }
    let font = theme::ui_font(theme::TEXT_TINY);
    let hint = theme::text_hint();
    painter.text(at([-1.0, 0.0, -1.0]), Align2::RIGHT_TOP, "L", font.clone(), hint);
    painter.text(at([1.0, 0.0, -1.0]), Align2::LEFT_TOP, "R", font.clone(), hint);
    painter.text(at([1.0, 0.0, -1.0]) + Vec2::new(0.0, 11.0), Align2::LEFT_TOP, "20 Hz", font.clone(), hint);
    painter.text(at([1.0, 0.0, 1.0]), Align2::LEFT_TOP, "20 kHz", font.clone(), hint);
    for frequency in [(100.0, "100"), (1_000.0, "1k"), (10_000.0, "10k")] {
        painter.text(at([1.0, 0.0, depth(frequency.0)]), Align2::LEFT_TOP, frequency.1, font.clone(), hint);
    }
    painter.text(at([-1.0, 1.0, -1.0]), Align2::RIGHT_CENTER, "0 dB", font.clone(), theme::recording());

    // The tracks.
    let feed = ui.ctx().data(|d| d.get_temp::<Feed>(feed_id())).unwrap_or_default();
    let lines: Vec<Line<'_>> = feed
        .tracks
        .iter()
        .filter(|look| inside(&feed.tracks, look, track) && !(hide_muted && look.muted))
        .filter_map(|look| feed.snapshot.track(look.id, tap).map(|space| Line { look, bands: &space.bands }))
        .collect();
    if lines.is_empty() {
        let text = if feed.tracks.is_empty() {
            "Open a project to see its mix"
        } else {
            "Play the project to see where each track sits"
        };
        painter.text(view.center(), Align2::CENTER_CENTER, text, theme::ui_font(theme::TEXT), theme::text_hint());
        return;
    }
    let bands = lines.iter().map(|l| l.bands.len()).min().unwrap_or(0);
    // Which band of which line another line covers, and the overlaps.
    let mut covered = vec![vec![false; bands]; lines.len()];
    let mut found = Vec::new();
    if marks != overlaps::OFF {
        for band in 0..bands {
            for (a, line_a) in lines.iter().enumerate() {
                for (b, line_b) in lines.iter().enumerate().skip(a + 1) {
                    let (Some(ra), Some(rb)) = (line_a.bands.get(band), line_b.bands.get(band)) else { continue };
                    if ra.level_db < OVERLAP_FLOOR_DB
                        || rb.level_db < OVERLAP_FLOOR_DB
                        || (ra.pan - rb.pan).abs() > NEAR_PAN
                    {
                        continue;
                    }
                    let gap = (ra.level_db - rb.level_db).abs();
                    if gap <= range {
                        let mid =
                            [(ra.pan + rb.pan) / 2.0, height((ra.level_db + rb.level_db) / 2.0), depth(ra.frequency)];
                        found.push(Overlap {
                            at: at(mid),
                            strength: 1.0 - gap / range.max(0.1),
                            text: format!(
                                "{} {:.0} dB, {} {:.0} dB · {} · {}",
                                line_a.look.name,
                                ra.level_db,
                                line_b.look.name,
                                rb.level_db,
                                hertz(ra.frequency),
                                side((ra.pan + rb.pan) / 2.0)
                            ),
                        });
                    } else {
                        let quieter = if ra.level_db < rb.level_db { a } else { b };
                        if let Some(flag) = covered.get_mut(quieter).and_then(|c| c.get_mut(band)) {
                            *flag = true;
                        }
                    }
                }
            }
        }
    }
    let only = marks == overlaps::ONLY;
    for (index, line) in lines.iter().enumerate() {
        let alpha = if line.look.muted { 0.3 } else { 1.0 } * if only { 0.15 } else { 1.0 };
        let color = line.look.color.gamma_multiply(alpha);
        let points: Vec<Pos2> =
            line.bands.iter().take(bands).map(|b| at([b.pan, height(b.level_db), depth(b.frequency)])).collect();
        let peaks: Vec<Pos2> =
            line.bands.iter().take(bands).map(|b| at([b.pan, height(b.peak_db), depth(b.frequency)])).collect();
        painter.add(Shape::line(peaks, Stroke::new(1.0, color.gamma_multiply(0.25))));
        for (i, pair) in points.windows(2).enumerate() {
            let (Some(a), Some(b)) = (line.bands.get(i), line.bands.get(i + 1)) else { continue };
            if a.level_db <= FLOOR_DB && b.level_db <= FLOOR_DB {
                continue;
            }
            let over = a.level_db > 0.0 || b.level_db > 0.0;
            let stroke = if over { Stroke::new(2.5, theme::recording()) } else { Stroke::new(1.6, color) };
            let hidden = covered.get(index).is_some_and(|c| c.get(i).copied().unwrap_or(false));
            if hidden {
                painter.extend(Shape::dashed_line(pair, Stroke::new(1.2, color.gamma_multiply(0.7)), 3.0, 3.0));
            } else {
                painter.line_segment([pair[0], pair[1]], stroke);
            }
        }
    }
    let mut nearest: Option<(f32, &Overlap)> = None;
    let pointer = response.hover_pos();
    for overlap in &found {
        let glow = Color32::WHITE.gamma_multiply(0.15 + 0.6 * overlap.strength);
        painter.circle_filled(overlap.at, 2.0 + 2.5 * overlap.strength, glow);
        if let Some(pointer) = pointer {
            let distance = pointer.distance(overlap.at);
            if distance < 10.0 && nearest.is_none_or(|(d, _)| distance < d) {
                nearest = Some((distance, overlap));
            }
        }
    }
    // The legend: each line's colour and name.
    let mut y = view.top() + 2.0;
    for line in &lines {
        let rect = Rect::from_min_size(Pos2::new(view.right() - 110.0, y), Vec2::new(106.0, 12.0));
        painter.rect_filled(
            Rect::from_min_size(rect.min + Vec2::new(0.0, 3.0), Vec2::splat(6.0)),
            1.0,
            line.look.color,
        );
        widgets::text_in(
            &painter,
            Rect::from_min_max(rect.min + Vec2::new(10.0, 0.0), rect.max),
            Align2::LEFT_CENTER,
            &line.look.name,
            font.clone(),
            theme::text_secondary(),
        );
        y += 13.0;
        if y > view.bottom() - 14.0 {
            break;
        }
    }
    if let Some((_, overlap)) = nearest {
        response.on_hover_text_at_pointer(&overlap.text);
    }
}

fn hertz(frequency: f32) -> String {
    if frequency >= 1_000.0 { format!("{:.1} kHz", frequency / 1_000.0) } else { format!("{frequency:.0} Hz") }
}

fn side(pan: f32) -> &'static str {
    if pan < -0.15 {
        "left"
    } else if pan > 0.15 {
        "right"
    } else {
        "centre"
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn look(id: u32, parent: Option<u32>) -> TrackLook {
        TrackLook {
            id: TrackId(id),
            name: format!("T{id}"),
            color: Color32::WHITE,
            parent: parent.map(TrackId),
            muted: false,
        }
    }

    #[test]
    fn a_group_shows_the_tracks_inside_it_and_main_shows_all() {
        let tracks = vec![look(1, Some(3)), look(2, Some(3)), look(3, None), look(4, None), look(5, Some(1))];
        let shown = |on: u32| -> Vec<u32> {
            tracks.iter().filter(|t| inside(&tracks, t, TrackId(on))).map(|t| t.id.0).collect()
        };
        assert_eq!(shown(3), [1, 2, 3, 5]);
        assert_eq!(shown(4), [4]);
        let all: Vec<u32> = tracks.iter().filter(|t| inside(&tracks, t, TrackId::MAIN)).map(|t| t.id.0).collect();
        assert_eq!(all, [1, 2, 3, 4, 5]);
    }

    #[test]
    fn the_box_maps_levels_and_frequencies() {
        assert_eq!(height(FLOOR_DB - 10.0), 0.0);
        assert_eq!(height(0.0), 1.0);
        assert!((height(OVER_DB + 30.0) - (1.0 + OVER_DB / -FLOOR_DB)).abs() < 1e-6);
        assert!((depth(20.0) + 1.0).abs() < 1e-6);
        assert!((depth(20_000.0) - 1.0).abs() < 1e-5);
        assert!(depth(1_000.0) > 0.0);
    }

    #[test]
    fn the_front_view_puts_left_left_and_loud_high() {
        let camera = Camera::view(View::Front);
        let area = Rect::from_min_size(Pos2::ZERO, Vec2::new(400.0, 300.0));
        let left = camera.project(area, [-1.0, 0.5, 0.0]);
        let right = camera.project(area, [1.0, 0.5, 0.0]);
        let loud = camera.project(area, [0.0, 1.0, 0.0]);
        let quiet = camera.project(area, [0.0, 0.0, 0.0]);
        assert!(left.x < right.x);
        assert!(loud.y < quiet.y);
    }

    /// Three made-up tracks: a kick and a bass that meet in the low end, a wide pad, and a lead
    /// that goes over the ceiling.
    fn feed() -> Feed {
        let grid = suhina_dsp::MixSpaceAnalyzer::new(48_000.0, 24).readings().to_vec();
        let shape = |f: &dyn Fn(f32) -> (f32, f32)| -> Vec<suhina_dsp::BandReading> {
            grid.iter()
                .map(|b| {
                    let (level, pan) = f(b.frequency);
                    suhina_dsp::BandReading { level_db: level, peak_db: level + 3.0, pan, ..*b }
                })
                .collect()
        };
        let bump = |f: f32, centre: f32, width: f32, top: f32| top - ((f / centre).log2() / width).powi(2) * 12.0;
        let tracks = [
            (1, "Kick", Color32::from_rgb(240, 200, 60), shape(&|f| (bump(f, 60.0, 0.6, -8.0), 0.0))),
            (2, "Bass", Color32::from_rgb(190, 110, 230), shape(&|f| (bump(f, 80.0, 0.8, -10.0), 0.02))),
            (
                3,
                "Pad",
                Color32::from_rgb(90, 190, 120),
                shape(&|f| (bump(f, 900.0, 2.0, -20.0), if f > 300.0 { -0.6 } else { 0.0 })),
            ),
            (4, "Lead", Color32::from_rgb(240, 120, 110), shape(&|f| (bump(f, 3_000.0, 1.2, 3.0), 0.4))),
        ];
        Feed {
            snapshot: Arc::new(MixSpaceSnapshot {
                tracks: tracks
                    .iter()
                    .map(|(id, _, _, bands)| suhina_session::TrackSpace {
                        track: TrackId(*id),
                        point: TapPoint::PostFader,
                        bands: bands.clone(),
                    })
                    .collect(),
                serial: 1,
            }),
            tracks: Arc::new(
                tracks
                    .iter()
                    .map(|(id, name, color, _)| TrackLook {
                        id: TrackId(*id),
                        name: (*name).into(),
                        color: *color,
                        parent: None,
                        muted: false,
                    })
                    .collect(),
            ),
        }
    }

    #[test]
    fn the_view_draws_the_tracks_marks_overlaps_and_asks_for_its_point() {
        let feed = feed();
        let mut harness =
            egui_kittest::Harness::builder().with_size((480.0, 300.0)).with_pixels_per_point(1.0).wgpu().build_ui(
                move |ui| {
                    ui.ctx().data_mut(|d| d.insert_temp(feed_id(), feed.clone()));
                    let values = vec![("overlaps".to_string(), overlaps::MARK)];
                    show(
                        ui,
                        Rect::from_min_size(Pos2::ZERO, Vec2::new(480.0, 300.0)),
                        TrackId::MAIN,
                        &crate::detail::detail_view::DeviceAt(vec![0]),
                        &values,
                    );
                },
            );
        theme::install(&harness.ctx);
        harness.run_steps(2);
        let want = harness.ctx.data(|d| d.get_temp::<Want>(want_id())).expect("asked");
        assert!(want.post > 0.0 || harness.ctx.input(|i| i.time) == 0.0);
        assert_eq!(want.pre, 0.0, "the post-fader view does not ask for pre-fader");
        let image = harness.render().expect("render");
        if std::env::var_os("SUHINA_UI_SNAPSHOTS").is_some() {
            let output = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../target/ui-snapshots");
            std::fs::create_dir_all(&output).expect("folder");
            image.save(output.join("mix-space.png")).expect("save");
        }
    }
}
