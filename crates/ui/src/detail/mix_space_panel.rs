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

use egui::{Align2, Color32, ColorImage, Id, Pos2, Rect, Sense, Shape, Stroke, Ui, Vec2};
use suhina_device_mix_space::{axis, draw, muted, overlaps, point, response};
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

/// The lowest level the box shows, at its floor, when the device's Floor is not set.
const FLOOR_DB: f32 = -72.0;
/// How far above 0 dBFS the zone over the ceiling reaches.
const OVER_DB: f32 = 12.0;
/// Two bands this close in the stereo field (of 2 from left to right) meet.
const NEAR_PAN: f32 = 0.3;
/// Bands quieter than this take no part in overlaps.
const OVERLAP_FLOOR_DB: f32 = -60.0;
/// The distance the camera looks from, in box units.
const DISTANCE: f32 = 4.2;
/// The rise and fall of the long average, in seconds: a chorus's tonal balance rather than its
/// moments.
const SLOW_SECONDS: f32 = 3.0;
/// The rise of the fast response, in seconds: short, so drum hits show.
const FAST_ATTACK: f32 = 0.01;
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
    /// Whether the last view drawn asked for the long average.
    slow: bool,
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
        if want.slow {
            running.set_times(SLOW_SECONDS, SLOW_SECONDS);
        } else if want.release > 0.0 {
            running.set_times(FAST_ATTACK, want.release);
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
    /// From the side, so the view opens as a familiar spectrum; 3D is one click away.
    fn default() -> Self {
        Self::view(View::Side)
    }
}

/// Where the camera is and where it is going: a view button turns it there over a few frames.
#[derive(Debug, Clone, Copy, Default, PartialEq)]
struct CameraState {
    now: Camera,
    target: Camera,
}

impl CameraState {
    /// Moves a step towards the target; true while it is still on its way.
    fn step(&mut self) -> bool {
        let (now, target) = (&mut self.now, self.target);
        let ease = |a: &mut f32, b: f32| {
            *a += (b - *a) * 0.18;
            if (b - *a).abs() < 1e-3 {
                *a = b;
            }
        };
        ease(&mut now.yaw, target.yaw);
        ease(&mut now.pitch, target.pitch);
        ease(&mut now.zoom, target.zoom);
        ease(&mut now.offset.x, target.offset.x);
        ease(&mut now.offset.y, target.offset.y);
        *now != target
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
    /// The view for the time axis: frequency left to right, time going back into the depth.
    fn view_for(view: View, time: bool) -> Self {
        if time && view == View::ThreeD {
            return Self { yaw: 0.25, pitch: 0.55, zoom: 1.05, offset: Vec2::ZERO };
        }
        Self::view(view)
    }

    fn view(view: View) -> Self {
        let (yaw, pitch) = match view {
            View::Front => (0.0, 0.0),
            View::Side => (-std::f32::consts::FRAC_PI_2, 0.0),
            View::Top => (0.0, 1.45),
            View::ThreeD => (-0.65, 0.42),
        };
        // The flat views fill the panel; 3D leaves room to turn.
        let zoom = if view == View::ThreeD { 1.0 } else { 1.35 };
        Self { yaw, pitch, zoom, offset: Vec2::ZERO }
    }

    /// How far the box point `p` is from the camera, along its view: larger is further.
    fn distance(&self, p: [f32; 3]) -> f32 {
        let [x, y, z] = [p[0], p[1] - 0.58, p[2]];
        let (sy, cy) = self.yaw.sin_cos();
        let z = x * sy + z * cy;
        let (sp, cp) = self.pitch.sin_cos();
        y * sp + z * cp
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

/// The box height of a level, with `floor` (dBFS, below 0) at the bottom and 0 dBFS at 1.
fn height(db: f32, floor: f32) -> f32 {
    let floor = if floor.is_finite() { floor.min(-6.0) } else { FLOOR_DB };
    ((db.max(floor) - floor) / -floor).min(1.0 + OVER_DB / -floor)
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

/// The bands with their stereo position and correlation averaged over their neighbours, two on
/// each side, weighted by how loud each is: the levels and so the peaks stay as they are, but a
/// sound whose position wavers from band to band (detuned voices beating) draws as one smooth
/// sheet rather than a tangle.
fn smooth_position(bands: Vec<suhina_dsp::BandReading>) -> Vec<suhina_dsp::BandReading> {
    let weight = |b: &suhina_dsp::BandReading| 10f32.powf(b.level_db.max(-120.0) / 20.0);
    (0..bands.len())
        .map(|i| {
            let around = bands.get(i.saturating_sub(2)..(i + 3).min(bands.len())).unwrap_or_default();
            let total: f32 = around.iter().map(weight).sum();
            let mut band = bands.get(i).copied().unwrap_or(suhina_dsp::BandReading {
                frequency: 0.0,
                level_db: -120.0,
                peak_db: -120.0,
                pan: 0.0,
                correlation: 1.0,
            });
            if total > 0.0 {
                band.pan = around.iter().map(|b| b.pan * weight(b)).sum::<f32>() / total;
                band.correlation = around.iter().map(|b| b.correlation * weight(b)).sum::<f32>() / total;
            }
            band
        })
        .collect()
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
    let marks = value("overlaps", overlaps::OFF);
    let range = value("overlapRange", 6.0) as f32;
    let release = value("release", 0.3) as f32;
    let draw = value("draw", draw::MESH);
    let time = value("axis", axis::STEREO) == axis::TIME;
    let tilt = value("tilt", 0.0) as f32;
    let floor = value("floor", f64::from(FLOOR_DB)) as f32;
    let slow = value("response", response::FAST) == response::SLOW;
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
        want.slow = slow;
    });
    let painter = ui.painter().with_clip_rect(area);
    painter.rect_filled(area, 0.0, theme::control_background());
    // The view itself is nearly black, so the lit ground stands out.
    painter.rect_filled(
        Rect::from_min_max(Pos2::new(area.left(), area.top() + 22.0), area.max),
        0.0,
        Color32::from_gray(10),
    );

    // The view buttons along the top.
    let camera_id = Id::new(("mix-space-camera", track.0, device));
    let mut state = ui.data(|d| d.get_temp::<CameraState>(camera_id)).unwrap_or_default();
    let mut x = area.left() + 4.0;
    for (label, view) in [("Front", View::Front), ("Side", View::Side), ("Top", View::Top), ("3D", View::ThreeD)] {
        let width = widgets::text_width(ui, label, theme::ui_font(theme::TEXT_SMALL)) + 10.0;
        let rect = Rect::from_min_size(Pos2::new(x, area.top() + 3.0), Vec2::new(width, 16.0));
        x = rect.right() + 3.0;
        let response = widgets::button_area(ui, rect, camera_id.with(label), format!("{label} view"))
            .on_hover_text(format!("Look at the box from the {}", label.to_lowercase()));
        let lit = state.target == Camera::view_for(view, time);
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
            state.target = Camera::view_for(view, time);
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
        state.now.offset += delta;
        state.target = state.now;
    } else if response.dragged_by(egui::PointerButton::Primary) {
        state.now.yaw += delta.x * 0.01;
        state.now.pitch = (state.now.pitch + delta.y * 0.01).clamp(-1.5, 1.5);
        state.target = state.now;
    }
    if response.hovered() {
        let scroll = ui.input(|i| i.smooth_scroll_delta.y);
        if scroll != 0.0 {
            state.now.zoom = (state.now.zoom * (scroll * 0.003).exp()).clamp(0.3, 6.0);
            state.target.zoom = state.now.zoom;
        }
    }
    if response.double_clicked() {
        state.target = Camera::default();
    }
    if state.step() {
        ui.ctx().request_repaint();
    }
    ui.data_mut(|d| d.insert_temp(camera_id, state));
    let camera = state.now;
    let at = |p: [f32; 3]| camera.project(view, p);

    // The box, its ceiling at 0 dBFS and the zone over it.
    let top = height(OVER_DB, floor);
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
    // Faint lines at every decade on the floor, and the labels.
    let font = theme::ui_font(theme::TEXT_TINY);
    let hint = theme::text_hint();
    let faint = Stroke::new(1.0, theme::text_hint().gamma_multiply(0.25));
    let ticks = [(100.0, "100"), (1_000.0, "1k"), (10_000.0, "10k")];
    if time {
        // Frequency runs across, time into the depth: now at the front.
        for (frequency, label) in ticks {
            let x = depth(frequency);
            painter.line_segment([at([x, 0.0, -1.0]), at([x, 0.0, 1.0])], faint);
            painter.text(at([x, 0.0, 1.0]), Align2::CENTER_TOP, label, font.clone(), hint);
        }
        painter.text(at([-1.0, 0.0, 1.0]), Align2::CENTER_TOP, "20 Hz", font.clone(), hint);
        painter.text(at([1.0, 0.0, 1.0]), Align2::CENTER_TOP, "20 kHz", font.clone(), hint);
        painter.text(at([1.0, 0.0, 1.0]) + Vec2::new(8.0, -2.0), Align2::LEFT_BOTTOM, "now", font.clone(), hint);
        painter.text(at([1.0, 0.0, -1.0]) + Vec2::new(8.0, -2.0), Align2::LEFT_BOTTOM, "earlier", font.clone(), hint);
    } else {
        for (frequency, label) in ticks {
            let z = depth(frequency);
            painter.line_segment([at([-1.0, 0.0, z]), at([1.0, 0.0, z])], faint);
            painter.text(at([1.0, 0.0, z]), Align2::LEFT_TOP, label, font.clone(), hint);
        }
        painter.text(at([-1.0, 0.0, -1.0]), Align2::RIGHT_TOP, "L", font.clone(), hint);
        painter.text(at([1.0, 0.0, -1.0]), Align2::LEFT_TOP, "R", font.clone(), hint);
        painter.text(at([1.0, 0.0, -1.0]) + Vec2::new(0.0, 11.0), Align2::LEFT_TOP, "20 Hz", font.clone(), hint);
        painter.text(at([1.0, 0.0, 1.0]), Align2::LEFT_TOP, "20 kHz", font.clone(), hint);
    }
    painter.text(at([-1.0, 1.0, -1.0]), Align2::RIGHT_CENTER, "0 dB", font.clone(), theme::recording());
    painter.text(
        at([-1.0, 0.0, -1.0]) + Vec2::new(-4.0, -6.0),
        Align2::RIGHT_BOTTOM,
        format!("{floor:.0} dB"),
        font.clone(),
        hint,
    );

    // The tracks. A track the pointer is on in the legend shows every band; the rest show their
    // envelope at the device's Detail and fade back.
    let feed = ui.ctx().data(|d| d.get_temp::<Feed>(feed_id())).unwrap_or_default();
    let focus_id = camera_id.with("focus");
    let focus = ui.data(|d| d.get_temp::<TrackId>(focus_id));
    // Bands on each side an envelope reaches over: 1/3, 1/6, 1/12 octave, or none.
    let spread = match value("detail", 0.0).round() as i64 {
        0 => 4,
        1 => 2,
        2 => 1,
        _ => 0,
    };
    // The tilt lifts each band by its octaves above 1 kHz, so a balanced mix reads level.
    let tilted: Vec<(TrackLook, Vec<suhina_dsp::BandReading>)> = feed
        .tracks
        .iter()
        .filter_map(|look| {
            let space = feed.snapshot.track(look.id, tap)?;
            let bands = space
                .bands
                .iter()
                .map(|b| {
                    let lift = tilt * (b.frequency.max(1.0) / 1_000.0).log2();
                    let above = |db: f32| if db <= suhina_dsp::mix_space::FLOOR_DB { db } else { db + lift };
                    suhina_dsp::BandReading { level_db: above(b.level_db), peak_db: above(b.peak_db), ..*b }
                })
                .collect();
            let bands = if focus == Some(look.id) { smooth_position(bands) } else { envelope(&bands, spread) };
            Some((look.clone(), bands))
        })
        .collect();
    let lines: Vec<Line<'_>> = tilted
        .iter()
        .filter(|(look, _)| inside(&feed.tracks, look, track) && !(hide_muted && look.muted))
        .map(|(look, bands)| Line { look, bands })
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
                        let mid = [
                            (ra.pan + rb.pan) / 2.0,
                            height((ra.level_db + rb.level_db) / 2.0, floor),
                            depth(ra.frequency),
                        ];
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
    if draw != draw::LINES {
        let alphas: Vec<f32> = lines
            .iter()
            .map(|line| {
                let muted = if line.look.muted { 0.3 } else { 1.0 };
                let faded = if only { 0.15 } else { 1.0 };
                let other = if focus.is_some_and(|f| f != line.look.id) { 0.2 } else { 1.0 };
                muted * faded * other
            })
            .collect();
        let scale = ui.ctx().pixels_per_point();
        let image = if time {
            // Each new reading becomes the front row; older rows move back and fall off.
            let history_id = camera_id.with("history");
            let mut history = ui.data(|d| d.get_temp::<History>(history_id)).unwrap_or_default();
            if history.serial != feed.snapshot.serial || history.rows.is_empty() {
                history.serial = feed.snapshot.serial;
                history.rows.push_front(time_row(&lines, &alphas, bands, floor));
                history.rows.truncate(HISTORY_ROWS);
            }
            let rows: Vec<Vec<(f32, [f32; 3])>> = history.rows.iter().cloned().collect();
            let xs: Vec<f32> = lines
                .first()
                .map(|l| l.bands.iter().take(bands).map(|b| depth(b.frequency)).collect())
                .unwrap_or_default();
            let count = rows.len().max(2);
            // The newest row nearest the camera, which looks from the depth's far end.
            let zs: Vec<f32> = (0..rows.len()).map(|t| 1.0 - t as f32 / (count - 1) as f32 * 2.0).collect();
            ui.data_mut(|d| d.insert_temp(history_id, history));
            surface(&camera, view, scale, &rows, &xs, &zs, floor, draw == draw::MESH, [6.0, 4.0])
        } else {
            landscape(&camera, view, scale, &lines, &alphas, bands, floor, draw == draw::MESH)
        };
        let texture_id = camera_id.with("landscape");
        let texture = match ui.data(|d| d.get_temp::<egui::TextureHandle>(texture_id)) {
            Some(mut texture) => {
                texture.set(image, egui::TextureOptions::LINEAR);
                texture
            }
            None => ui.ctx().load_texture("mix-space-landscape", image, egui::TextureOptions::LINEAR),
        };
        painter.image(texture.id(), view, Rect::from_min_max(Pos2::ZERO, Pos2::new(1.0, 1.0)), Color32::WHITE);
        ui.data_mut(|d| d.insert_temp(texture_id, texture));
    } else {
        for (index, line) in lines.iter().enumerate() {
            let alpha = if line.look.muted { 0.3 } else { 1.0 }
                * if only { 0.15 } else { 1.0 }
                * if focus.is_some_and(|f| f != line.look.id) { 0.2 } else { 1.0 };
            let color = line.look.color.gamma_multiply(alpha);
            let points: Vec<Pos2> = line
                .bands
                .iter()
                .take(bands)
                .map(|b| at([b.pan, height(b.level_db, floor), depth(b.frequency)]))
                .collect();
            let peaks: Vec<Pos2> = line
                .bands
                .iter()
                .take(bands)
                .map(|b| at([b.pan, height(b.peak_db, floor), depth(b.frequency)]))
                .collect();
            painter.add(Shape::line(peaks, Stroke::new(1.0, color.gamma_multiply(0.25))));
            for (i, pair) in points.windows(2).enumerate() {
                let (Some(a), Some(b)) = (line.bands.get(i), line.bands.get(i + 1)) else { continue };
                if a.level_db <= floor && b.level_db <= floor {
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
    }
    let mut nearest: Option<(f32, &Overlap)> = None;
    let pointer = response.hover_pos();
    for overlap in &found {
        // Faint where the levels are only near, bright where they are the same.
        if overlap.strength > 0.3 {
            let glow = Color32::WHITE.gamma_multiply(0.1 + 0.4 * overlap.strength);
            painter.circle_stroke(overlap.at, 1.5 + 2.0 * overlap.strength, Stroke::new(1.0, glow));
        }
        if let Some(pointer) = pointer {
            let distance = pointer.distance(overlap.at);
            if distance < 10.0 && nearest.is_none_or(|(d, _)| distance < d) {
                nearest = Some((distance, overlap));
            }
        }
    }
    // The legend: each line's colour and name.
    let mut y = view.top() + 2.0;
    let mut hovered = None;
    for line in &lines {
        let rect = Rect::from_min_size(Pos2::new(view.right() - 110.0, y), Vec2::new(106.0, 12.0));
        let row = ui
            .interact(rect, camera_id.with(("legend", line.look.id.0)), Sense::hover())
            .on_hover_text(format!("{}: every band, the others faded", line.look.name));
        if row.hovered() {
            hovered = Some(line.look.id);
            painter.rect_filled(rect.expand(1.0), 2.0, theme::panel_background());
        }
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
    if hovered != focus {
        ui.data_mut(|d| match hovered {
            Some(id) => {
                d.insert_temp(focus_id, id);
            }
            None => d.remove::<TrackId>(focus_id),
        });
        ui.ctx().request_repaint();
    }
    if let Some((_, overlap)) = nearest {
        response.on_hover_text_at_pointer(&overlap.text);
    }
}

/// A track's spectrum as one shape: each band's level and peak the highest within `spread`
/// bands on each side (so the shape rides over the harmonics' tips rather than between them),
/// then averaged over the same reach so it is round; its stereo position and correlation are
/// averaged by loudness as [`smooth_position`] does. `spread` 0 keeps the levels as they are.
fn envelope(bands: &[suhina_dsp::BandReading], spread: usize) -> Vec<suhina_dsp::BandReading> {
    let bands = smooth_position(bands.to_vec());
    if spread == 0 {
        return bands;
    }
    let reach = |len: usize, i: usize| i.saturating_sub(spread)..(i + spread + 1).min(len);
    let tops: Vec<(f32, f32)> = (0..bands.len())
        .map(|i| {
            bands
                .get(reach(bands.len(), i))
                .unwrap_or_default()
                .iter()
                .fold((f32::MIN, f32::MIN), |(l, p), b| (l.max(b.level_db), p.max(b.peak_db)))
        })
        .collect();
    bands
        .iter()
        .enumerate()
        .map(|(i, band)| {
            let near = tops.get(reach(tops.len(), i)).unwrap_or_default();
            let count = near.len().max(1) as f32;
            let level_db = near.iter().map(|t| t.0).sum::<f32>() / count;
            let peak_db = near.iter().map(|t| t.1).sum::<f32>() / count;
            suhina_dsp::BandReading { level_db, peak_db, ..*band }
        })
        .collect()
}

/// Rows of the time view: about two seconds of readings at 60 a second.
const HISTORY_ROWS: usize = 120;

/// The time view's past readings, newest first, and the snapshot the newest came from.
#[derive(Debug, Clone, Default)]
struct History {
    serial: u64,
    rows: std::collections::VecDeque<Vec<(f32, [f32; 3])>>,
}

/// One row of the time view: per band, every track's power summed and its colour weighted by
/// each track's share, smoothed along the frequencies so the row is one ridge.
fn time_row(lines: &[Line<'_>], alphas: &[f32], bands: usize, floor: f32) -> Vec<(f32, [f32; 3])> {
    let mut row = vec![(0.0f32, [0.0f32; 3]); bands];
    for (line, alpha) in lines.iter().zip(alphas) {
        let rgb = [line.look.color.r(), line.look.color.g(), line.look.color.b()].map(|v| f32::from(v) / 255.0);
        for (cell, band) in row.iter_mut().zip(line.bands) {
            if band.level_db <= floor {
                continue;
            }
            let power = 10f32.powf(band.level_db / 10.0) * alpha;
            cell.0 += power;
            for (sum, value) in cell.1.iter_mut().zip(rgb) {
                *sum += value * power;
            }
        }
    }
    let before = row.clone();
    for (i, cell) in row.iter_mut().enumerate() {
        let (Some(h), Some(a), Some(b)) =
            (before.get(i), before.get(i.saturating_sub(1)), before.get((i + 1).min(bands.saturating_sub(1))))
        else {
            continue;
        };
        cell.0 = (a.0 + 2.0 * h.0 + b.0) / 4.0;
        for k in 0..3 {
            cell.1[k] = (a.1[k] + 2.0 * h.1[k] + b.1[k]) / 4.0;
        }
    }
    row
}

/// Columns of the landscape across the stereo field.
const FIELD_COLUMNS: usize = 128;
/// Columns between two grid lines across the stereo field.
const LINE_COLUMNS: f32 = 4.0;
/// Bands between two grid lines along the frequencies.
const LINE_BANDS: f32 = 5.0;

/// Every track blended into one landscape, like a spectrogram in three dimensions, drawn pixel
/// by pixel ([`crate::detail::mix_space_raster`]). Over a grid of the stereo field against the
/// bands, each point is as high as all tracks' sound there together and coloured by the tracks
/// that make it, each in proportion to its share, so where two tracks meet their colours mix.
/// A band spreads across the stereo field as a soft bell centred on its position, as wide as the
/// band is; quiet ground fades out towards the floor and the loudest burns towards white. The
/// surface is lit from above, the left and the front, and carries a net of fine lines: strong
/// for `mesh`, faint otherwise.
#[allow(clippy::too_many_arguments)]
fn landscape(
    camera: &Camera,
    view: Rect,
    scale: f32,
    lines: &[Line<'_>],
    alphas: &[f32],
    bands: usize,
    floor: f32,
    mesh: bool,
) -> ColorImage {
    let column = |c: usize| c as f32 / (FIELD_COLUMNS - 1) as f32 * 2.0 - 1.0;
    // Per band and column: the summed power and the colour weighted by each track's power.
    let mut grid: Vec<Vec<(f32, [f32; 3])>> = Vec::with_capacity(bands);
    let mut depths = Vec::with_capacity(bands);
    for band in 0..bands {
        let mut row = vec![(0.0f32, [0.0f32; 3]); FIELD_COLUMNS];
        let mut frequency = 0.0;
        for (line, alpha) in lines.iter().zip(alphas) {
            let Some(b) = line.bands.get(band) else { continue };
            frequency = b.frequency;
            if b.level_db <= floor {
                continue;
            }
            let power = 10f32.powf(b.level_db / 10.0) * alpha;
            let sigma = 0.05 + 0.9 * b.width();
            let rgb = [line.look.color.r(), line.look.color.g(), line.look.color.b()].map(|v| f32::from(v) / 255.0);
            for (c, cell) in row.iter_mut().enumerate() {
                let distance = (column(c) - b.pan) / sigma;
                let share = power * (-0.5 * distance * distance).exp();
                cell.0 += share;
                for (sum, value) in cell.1.iter_mut().zip(rgb) {
                    *sum += value * share;
                }
            }
        }
        grid.push(row);
        depths.push(depth(frequency));
    }
    // Smoothed along the frequencies, twice with a 1-2-1 kernel, so a sound whose position
    // wavers from band to band (detuned voices beating) lies as one smooth slope instead of
    // ridged stripes; a peak keeps its place and most of its height.
    for _ in 0..2 {
        let before = grid.clone();
        for (band, row) in grid.iter_mut().enumerate() {
            let Some(here) = before.get(band) else { continue };
            let below = before.get(band.saturating_sub(1)).unwrap_or(here);
            let above = before.get(band + 1).unwrap_or(here);
            for (c, cell) in row.iter_mut().enumerate() {
                let (Some(a), Some(b), Some(h)) = (below.get(c), above.get(c), here.get(c)) else { continue };
                cell.0 = (a.0 + 2.0 * h.0 + b.0) / 4.0;
                for i in 0..3 {
                    cell.1[i] = (a.1[i] + 2.0 * h.1[i] + b.1[i]) / 4.0;
                }
            }
        }
    }
    let xs: Vec<f32> = (0..FIELD_COLUMNS).map(column).collect();
    surface(camera, view, scale, &grid, &xs, &depths, floor, mesh, [LINE_COLUMNS, LINE_BANDS])
}

/// A landscape from `grid` (rows of columns: the summed power and the power-weighted colour)
/// drawn pixel by pixel: column `c` of row `r` stands at `xs[c]` across and `zs[r]` into the
/// depth, as high as its power. Lines fall every `lines_every` columns and rows.
#[allow(clippy::too_many_arguments)]
fn surface(
    camera: &Camera,
    view: Rect,
    scale: f32,
    grid: &[Vec<(f32, [f32; 3])>],
    xs: &[f32],
    zs: &[f32],
    floor: f32,
    mesh: bool,
    lines_every: [f32; 2],
) -> ColorImage {
    use crate::detail::mix_space_raster::{Corner, Look, render};
    let span = -floor.min(-6.0);
    let (bands, columns) = (grid.len().min(zs.len()), xs.len());
    let heights: Vec<Vec<f32>> = grid
        .iter()
        .map(|row| {
            row.iter().map(|(power, _)| if *power > 0.0 { height(10.0 * power.log10(), floor) } else { 0.0 }).collect()
        })
        .collect();
    let at_height = |band: usize, c: usize| heights.get(band).and_then(|r| r.get(c)).copied().unwrap_or(0.0);

    let mut corners = Vec::with_capacity(bands * columns);
    for band in 0..bands {
        let z = zs.get(band).copied().unwrap_or(0.0);
        let next = (band + 1).min(bands.saturating_sub(1));
        let previous = band.saturating_sub(1);
        let dz_span = (zs.get(next).copied().unwrap_or(z) - zs.get(previous).copied().unwrap_or(z)).max(1e-3);
        for c in 0..columns {
            let (power, rgb) = grid.get(band).and_then(|r| r.get(c)).copied().unwrap_or((0.0, [0.0; 3]));
            let h = at_height(band, c);
            let right = (c + 1).min(columns.saturating_sub(1));
            let left = c.saturating_sub(1);
            let dx_span = (xs.get(right).copied().unwrap_or(0.0) - xs.get(left).copied().unwrap_or(0.0)).max(1e-3);
            let dx = (at_height(band, right) - at_height(band, left)) / dx_span;
            let dz = (at_height(next, c) - at_height(previous, c)) / dz_span;
            let db = if power > 0.0 { 10.0 * power.log10() } else { floor };
            let loud = ((db - floor) / span).clamp(0.0, 1.2);
            let base = if power > 0.0 { rgb.map(|v| v / power) } else { [0.0; 3] };
            // The loudest ground burns towards white, as a spectrogram's does.
            let hot = ((loud - 0.85) * 2.5).clamp(0.0, 0.5);
            let color = base.map(|v| v + (1.0 - v) * hot);
            let point = [xs.get(c).copied().unwrap_or(0.0), h, z];
            let at = camera.project(view, point);
            corners.push(Corner {
                at: [(at.x - view.left()) * scale, (at.y - view.top()) * scale],
                depth: camera.distance(point),
                normal: [-dx, 1.0, -dz],
                color: [color[0], color[1], color[2], (loud / 0.08).clamp(0.0, 1.0)],
                grid: [c as f32 / lines_every[0], band as f32 / lines_every[1]],
            });
        }
    }
    let mut triangles = Vec::with_capacity(bands * columns * 2);
    let index = |band: usize, c: usize| u32::try_from(band * columns + c).unwrap_or(u32::MAX);
    for band in 0..bands.saturating_sub(1) {
        for c in 0..columns.saturating_sub(1) {
            let (a, b, d, e) = (index(band, c), index(band, c + 1), index(band + 1, c + 1), index(band + 1, c));
            triangles.push([a, b, d]);
            triangles.push([a, d, e]);
        }
    }
    let light = {
        let [x, y, z] = [-0.45f32, 0.8, -0.4];
        let length = (x * x + y * y + z * z).sqrt();
        [x / length, y / length, z / length]
    };
    let look = Look { light, lines: if mesh { 0.85 } else { 0.18 }, line_width: 0.9 * scale.max(1.0) };
    let width = (view.width() * scale).round().max(1.0) as usize;
    let height = (view.height() * scale).round().max(1.0) as usize;
    render(width, height, &corners, &triangles, &look)
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
        assert_eq!(height(FLOOR_DB - 10.0, FLOOR_DB), 0.0);
        assert_eq!(height(0.0, FLOOR_DB), 1.0);
        assert!((height(OVER_DB + 30.0, FLOOR_DB) - (1.0 + OVER_DB / -FLOOR_DB)).abs() < 1e-6);
        assert_eq!(height(-48.0, -48.0), 0.0, "a raised floor");
        assert_eq!(height(-24.0, -48.0), 0.5);
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

    /// Four tracks of real sound run through the analysis: a kick (a falling sine), a mono saw
    /// bass, a wide pad of detuned saws and a square lead panned right.
    fn feed() -> Feed {
        const RATE: f32 = 48_000.0;
        let frames = 48_000;
        let saw = |f: f32, n: usize| 2.0 * (f * n as f32 / RATE).fract() - 1.0;
        let analyse = |make: &dyn Fn(usize) -> (f32, f32)| {
            let mut analyzer = suhina_dsp::MixSpaceAnalyzer::new(f64::from(RATE), 24);
            let (left, right): (Vec<f32>, Vec<f32>) = (0..frames).map(make).unzip();
            analyzer.push(&left, &right);
            analyzer.readings().to_vec()
        };
        let kick = analyse(&|n| {
            let t = (n % 24_000) as f32 / RATE;
            let phase = std::f32::consts::TAU * (50.0 * t + 60.0 * (1.0 - (-t * 30.0).exp()) / 30.0);
            let v = phase.sin() * (-t * 6.0).exp() * 0.9;
            (v, v)
        });
        let bass = analyse(&|n| {
            let v = saw(55.0, n) * 0.3;
            (v, v)
        });
        let pad = analyse(&|n| {
            let l = (saw(220.0, n) + saw(277.2, n) + saw(329.6, n)) * 0.05;
            let r = (saw(220.9, n) + saw(276.4, n) + saw(330.5, n)) * 0.05;
            (l, r)
        });
        let lead = analyse(&|n| {
            let v = if (880.0 * n as f32 / RATE).fract() < 0.5 { 1.0 } else { -1.0 };
            let (gl, gr) = suhina_dsp::pan::gains(0.5);
            (v * gl as f32, v * gr as f32)
        });
        // A little noise generator, with one-pole filters for the noisy tracks.
        let noise = |seed: u32| {
            let mut state = seed;
            move || {
                state ^= state << 13;
                state ^= state >> 17;
                state ^= state << 5;
                state as f32 / u32::MAX as f32 * 2.0 - 1.0
            }
        };
        let filtered = |seed: u32, low: f32, high: f32, gain: f32, side: f32| {
            let (mut left, mut right) = (noise(seed), noise(seed ^ 0x9e37_79b9));
            let k = |f: f32| 1.0 - (-std::f32::consts::TAU * f / RATE).exp();
            let (kl, kh) = (k(low), k(high));
            let mut state = [0.0f32; 4];
            let (mut out_l, mut out_r) = (Vec::with_capacity(frames), Vec::with_capacity(frames));
            for n in 0..frames {
                let burst = if (n % 12_000) < 2_400 { (-((n % 12_000) as f32) / 600.0).exp() } else { 0.0 };
                let (l, r) = (left(), right());
                state[0] += (l - state[0]) * kh;
                state[1] += (state[0] - state[1]) * kl;
                state[2] += (r - state[2]) * kh;
                state[3] += (state[2] - state[3]) * kl;
                let (bl, br) = (state[0] - state[1], state[2] - state[3]);
                let mid = (bl + br) / 2.0;
                out_l.push((mid + (bl - mid) * side) * gain * burst.max(if gain < 0.2 { 1.0 } else { 0.0 }));
                out_r.push((mid + (br - mid) * side) * gain * burst.max(if gain < 0.2 { 1.0 } else { 0.0 }));
            }
            let mut analyzer = suhina_dsp::MixSpaceAnalyzer::new(f64::from(RATE), 24);
            analyzer.push(&out_l, &out_r);
            analyzer.readings().to_vec()
        };
        let snare = filtered(7, 180.0, 6_000.0, 0.8, 0.2);
        let hats = filtered(11, 7_000.0, 18_000.0, 0.5, 0.6);
        let room = filtered(13, 300.0, 4_000.0, 0.08, 1.0);
        let vocal = analyse(&|n| {
            let f = 196.0 * (1.0 + 0.004 * (std::f32::consts::TAU * 5.0 * n as f32 / RATE).sin());
            let v: f32 =
                (1..12).map(|h| (std::f32::consts::TAU * f * h as f32 * n as f32 / RATE).sin() / (h * h) as f32).sum();
            let (gl, gr) = suhina_dsp::pan::gains(-0.15);
            (v * 0.35 * gl as f32, v * 0.35 * gr as f32)
        });
        let tracks = [
            (1, "Kick", Color32::from_rgb(240, 200, 60), kick),
            (2, "Snare", Color32::from_rgb(90, 170, 240), snare),
            (3, "Hats", Color32::from_rgb(120, 230, 230), hats),
            (4, "Bass", Color32::from_rgb(190, 110, 230), bass),
            (5, "Pad", Color32::from_rgb(90, 190, 120), pad),
            (6, "Lead", Color32::from_rgb(240, 120, 110), lead),
            (7, "Vocal", Color32::from_rgb(250, 160, 60), vocal),
            (8, "Reverb", Color32::from_rgb(150, 150, 170), room),
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
            egui_kittest::Harness::builder().with_size((720.0, 420.0)).with_pixels_per_point(2.0).wgpu().build_ui(
                move |ui| {
                    ui.ctx().data_mut(|d| d.insert_temp(feed_id(), feed.clone()));
                    let values: Vec<(String, f64)> = Vec::new();
                    show(
                        ui,
                        Rect::from_min_size(Pos2::ZERO, Vec2::new(720.0, 420.0)),
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
        let save = |harness: &mut egui_kittest::Harness<'_>, name: &str| {
            let image = harness.render().expect("render");
            if std::env::var_os("SUHINA_UI_SNAPSHOTS").is_some() {
                let output = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../target/ui-snapshots");
                std::fs::create_dir_all(&output).expect("folder");
                image.save(output.join(name)).expect("save");
            }
        };
        save(&mut harness, "mix-space-side.png");
        let camera_id = Id::new(("mix-space-camera", TrackId::MAIN.0, &crate::detail::detail_view::DeviceAt(vec![0])));
        let low = Camera { yaw: -0.35, pitch: 0.15, zoom: 1.15, offset: Vec2::ZERO };
        let behind = Camera { yaw: -2.5, pitch: 0.35, zoom: 1.0, offset: Vec2::ZERO };
        for (name, camera) in [
            ("mix-space-3d.png", Camera::view(View::ThreeD)),
            ("mix-space-front.png", Camera::view(View::Front)),
            ("mix-space-top.png", Camera::view(View::Top)),
            ("mix-space-low.png", low),
            ("mix-space-behind.png", behind),
        ] {
            harness.ctx.data_mut(|d| d.insert_temp(camera_id, CameraState { now: camera, target: camera }));
            harness.run_steps(2);
            save(&mut harness, name);
        }
    }

    #[test]
    fn the_time_view_draws_the_last_readings_front_to_back() {
        let feed = feed();
        // Two seconds of a beat: a kick every half second, a snare between, hats on every
        // eighth, the rest held.
        let envelope = |id: u32, t: usize| -> f32 {
            let since = |period: usize, offset: usize| ((t + period - offset) % period) as f32;
            match id {
                1 => (-since(30, 0) / 4.0).exp(),
                2 => (-since(30, 15) / 3.0).exp(),
                3 => (-since(8, 0) / 1.5).exp(),
                _ => 1.0,
            }
        };
        let mut history = History { serial: feed.snapshot.serial, ..History::default() };
        for t in 0..HISTORY_ROWS {
            let shaped: Vec<(TrackLook, Vec<suhina_dsp::BandReading>)> = feed
                .tracks
                .iter()
                .filter_map(|look| {
                    let space = feed.snapshot.track(look.id, TapPoint::PostFader)?;
                    let gain = 20.0 * envelope(look.id.0, t).max(1e-4).log10();
                    let bands = space
                        .bands
                        .iter()
                        .map(|b| suhina_dsp::BandReading { level_db: b.level_db + gain, ..*b })
                        .collect();
                    Some((look.clone(), envelope_free(bands)))
                })
                .collect();
            let lines: Vec<Line<'_>> = shaped.iter().map(|(look, bands)| Line { look, bands }).collect();
            let alphas = vec![1.0; lines.len()];
            let bands = lines.iter().map(|l| l.bands.len()).min().unwrap_or(0);
            history.rows.push_back(time_row(&lines, &alphas, bands, FLOOR_DB));
        }
        let camera_id = Id::new(("mix-space-camera", TrackId::MAIN.0, &crate::detail::detail_view::DeviceAt(vec![0])));
        let mut harness =
            egui_kittest::Harness::builder().with_size((720.0, 420.0)).with_pixels_per_point(2.0).wgpu().build_ui(
                move |ui| {
                    ui.ctx().data_mut(|d| d.insert_temp(feed_id(), feed.clone()));
                    let values = vec![("axis".to_string(), axis::TIME)];
                    show(
                        ui,
                        Rect::from_min_size(Pos2::ZERO, Vec2::new(720.0, 420.0)),
                        TrackId::MAIN,
                        &crate::detail::detail_view::DeviceAt(vec![0]),
                        &values,
                    );
                },
            );
        theme::install(&harness.ctx);
        harness.ctx.data_mut(|d| d.insert_temp(camera_id.with("history"), history));
        for (name, camera) in [
            ("mix-space-time-3d.png", Camera::view_for(View::ThreeD, true)),
            ("mix-space-time-low.png", Camera { yaw: -0.3, pitch: 0.35, zoom: 1.2, offset: Vec2::ZERO }),
        ] {
            harness.ctx.data_mut(|d| d.insert_temp(camera_id, CameraState { now: camera, target: camera }));
            harness.run_steps(2);
            let image = harness.render().expect("render");
            if std::env::var_os("SUHINA_UI_SNAPSHOTS").is_some() {
                let output = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../target/ui-snapshots");
                std::fs::create_dir_all(&output).expect("folder");
                image.save(output.join(name)).expect("save");
            }
        }
    }

    /// The bands as the view draws them by default: their envelope.
    fn envelope_free(bands: Vec<suhina_dsp::BandReading>) -> Vec<suhina_dsp::BandReading> {
        envelope(&bands, 4)
    }
}
