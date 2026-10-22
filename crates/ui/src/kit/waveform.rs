// Copyright (C) 2026 The Suhina contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! Waveforms of audio files, as `docs/spec/waveforms.md` describes: peaks (lowest, highest and
//! RMS per channel) at a few resolutions, computed once when a file is decoded, and one way of
//! drawing them for clips, the browser's preview and the sample map. Drawing reads the level
//! closest to the zoom, the samples themselves when zoomed in further, so a column costs a few
//! numbers at any zoom; the shape is an anti-aliased fill with the RMS as a brighter band inside,
//! one lane per channel when there is room, and a line through the samples once a sample is
//! wider than a pixel.

use std::collections::HashMap;
use std::sync::Arc;

use egui::epaint::{Mesh, PathShape, PathStroke};
use egui::{Color32, Painter, Pos2, Rect, Shape, Stroke};
use suhina_engine::{AudioData, AudioPool};

/// Frames per bucket at each level, finest first; each level is eight of the one before.
pub const LEVELS: [usize; 3] = [256, 2048, 16384];
/// At most this many channels get lanes of their own (left and right); others are not drawn.
pub const MAX_LANES: usize = 2;
/// A clip at least this tall draws a stereo file as two lanes, one per channel.
pub const STEREO_MIN_HEIGHT: f32 = 40.0;
/// The quietest level the dB scale shows, in dB below full scale.
pub const DB_RANGE: f32 = 48.0;
/// Samples at least this far apart, in points, are drawn as dots on their line.
const DOT_SPACING: f32 = 6.0;

/// The lowest and highest sample of a stretch, and its RMS level.
#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub struct Bucket {
    pub min: f32,
    pub max: f32,
    pub rms: f32,
}

/// Sums samples into a [`Bucket`].
#[derive(Clone, Copy)]
struct Sum {
    min: f32,
    max: f32,
    squares: f64,
    count: u32,
}

impl Sum {
    const EMPTY: Self = Self { min: f32::INFINITY, max: f32::NEG_INFINITY, squares: 0.0, count: 0 };

    /// Takes in a bucket, as `weight` samples (its frames).
    fn take(&mut self, bucket: Bucket, weight: u32) {
        self.min = self.min.min(bucket.min);
        self.max = self.max.max(bucket.max);
        self.squares += f64::from(bucket.rms) * f64::from(bucket.rms) * f64::from(weight);
        self.count = self.count.saturating_add(weight);
    }

    fn add(&mut self, sample: f32) {
        let sample = if sample.is_finite() { sample } else { 0.0 };
        self.min = self.min.min(sample);
        self.max = self.max.max(sample);
        self.squares += f64::from(sample) * f64::from(sample);
        self.count += 1;
    }

    fn bucket(self) -> Bucket {
        if self.count == 0 {
            return Bucket::default();
        }
        let rms = (self.squares / f64::from(self.count)).sqrt() as f32;
        Bucket { min: self.min, max: self.max, rms }
    }
}

/// One level: `frames` per bucket, buckets interleaved by lane.
#[derive(Debug, Clone, PartialEq, Default)]
struct Level {
    frames: usize,
    buckets: Vec<Bucket>,
}

/// One file's waveform.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct Peaks {
    pub sample_rate: u32,
    /// Frames in the file.
    pub frames: usize,
    /// Lanes it can be drawn in: 1 for a mono file, 2 (left, right) for more channels.
    pub lanes: usize,
    levels: Vec<Level>,
    /// The samples, for drawing closer than the finest level; shared with the audio pool.
    data: Option<Arc<AudioData>>,
}

/// Waveforms by the `file` string of the project's audio regions, as the audio pool keys them.
pub type Waveforms = HashMap<String, Arc<Peaks>>;

/// The waveforms of every file in `pool`.
pub fn from_pool(pool: &AudioPool) -> Waveforms {
    pool.files.iter().map(|(file, data)| (file.clone(), Arc::new(peaks(data)))).collect()
}

/// Summarises one decoded file at every level; keeps the samples for drawing close up.
pub fn peaks(data: &Arc<AudioData>) -> Peaks {
    let mut peaks = summarise(data);
    peaks.data = Some(Arc::clone(data));
    peaks
}

fn summarise(data: &AudioData) -> Peaks {
    let channels = usize::from(data.channels.max(1));
    let lanes = channels.min(MAX_LANES);
    let frames = data.samples.len() / channels;
    let mut finest = Vec::with_capacity((frames / LEVELS[0] + 1) * lanes);
    for chunk in data.samples.chunks(LEVELS[0] * channels) {
        let mut sums = [Sum::EMPTY; MAX_LANES];
        for frame in chunk.chunks_exact(channels) {
            for (sum, sample) in sums.iter_mut().zip(frame) {
                sum.add(*sample);
            }
        }
        finest.extend(sums.iter().take(lanes).map(|s| s.bucket()));
    }
    let mut levels = vec![Level { frames: LEVELS[0], buckets: finest }];
    for pair in LEVELS.windows(2) {
        let (from, to) = (pair[0], pair[1]);
        let ratio = to / from;
        let below = levels.last().map(|l| l.buckets.as_slice()).unwrap_or_default();
        let count = below.len() / lanes;
        let mut buckets = Vec::with_capacity((count / ratio + 1) * lanes);
        for group in (0..count).step_by(ratio) {
            for lane in 0..lanes {
                let mut sum = Sum::EMPTY;
                for i in group..(group + ratio).min(count) {
                    if let Some(b) = below.get(i * lanes + lane) {
                        sum.take(*b, from as u32);
                    }
                }
                buckets.push(sum.bucket());
            }
        }
        levels.push(Level { frames: to, buckets });
    }
    Peaks { sample_rate: data.sample_rate, frames, lanes, levels, data: None }
}

/// Which lane of a file a column is read from.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Lane {
    /// One channel (0 left, 1 right).
    Channel(usize),
    /// Every lane together.
    Mixed,
}

impl Peaks {
    /// The bucket covering frames `from` … `to` of `lane`, from the coarsest level with at
    /// least one bucket in that stretch, or the samples when it is shorter than the finest;
    /// `None` outside the file.
    pub fn column(&self, lane: Lane, from: f64, to: f64) -> Option<Bucket> {
        if !(from.is_finite() && to.is_finite()) || self.frames == 0 || self.lanes == 0 {
            return None;
        }
        let (from, to) = (from.min(to).max(0.0), from.max(to).min(self.frames as f64));
        if to <= from && from >= self.frames as f64 || to < 0.0 {
            return None;
        }
        let width = to - from;
        let lanes: &[usize] = match lane {
            Lane::Channel(c) if c < self.lanes => &[c][..],
            Lane::Channel(_) => return None,
            Lane::Mixed if self.lanes == 2 => &[0, 1][..],
            Lane::Mixed => &[0][..],
        };
        let raw = self.data.as_deref().filter(|_| width < LEVELS[0] as f64);
        if let Some(data) = raw {
            return Some(self.read_samples(data, lanes, from, to));
        }
        let level = self.levels.iter().rev().find(|l| l.frames as f64 <= width).or(self.levels.first())?;
        let first = (from / level.frames as f64).floor() as usize;
        let last = ((to / level.frames as f64).ceil() as usize).max(first + 1);
        let mut sum = Sum::EMPTY;
        for &l in lanes {
            for i in first..last {
                if let Some(b) = level.buckets.get(i * self.lanes + l) {
                    sum.take(*b, level.frames as u32);
                }
            }
        }
        (sum.count > 0).then(|| sum.bucket())
    }

    fn read_samples(&self, data: &AudioData, lanes: &[usize], from: f64, to: f64) -> Bucket {
        let channels = usize::from(data.channels.max(1));
        let first = from.floor() as usize;
        let last = (to.ceil() as usize).max(first + 1).min(self.frames);
        let Some(frames) = data.samples.get(first * channels..last * channels) else { return Bucket::default() };
        // At most the finest level's frames: sums in f32 are exact enough.
        let (mut min, mut max, mut squares, mut count) = (f32::INFINITY, f32::NEG_INFINITY, 0.0f32, 0u32);
        for frame in frames.chunks_exact(channels) {
            for &lane in lanes {
                let s = frame.get(lane).copied().unwrap_or(0.0);
                let s = if s.is_finite() { s } else { 0.0 };
                min = min.min(s);
                max = max.max(s);
                squares += s * s;
                count += 1;
            }
        }
        if count == 0 {
            return Bucket::default();
        }
        Bucket { min, max, rms: (squares / count as f32).sqrt() }
    }

    /// One frame's sample of `lane` (the lanes averaged for [`Lane::Mixed`]); `None` without the
    /// samples or outside the file.
    pub fn sample(&self, lane: Lane, frame: usize) -> Option<f32> {
        let data = self.data.as_deref()?;
        let channels = usize::from(data.channels.max(1));
        let at = |c: usize| data.samples.get(frame * channels + c).copied().filter(|s| s.is_finite());
        match lane {
            Lane::Channel(c) if c < self.lanes => at(c),
            Lane::Channel(_) => None,
            Lane::Mixed if self.lanes == 2 => Some((at(0)? + at(1)?) * 0.5),
            Lane::Mixed => at(0),
        }
    }

    /// The range of the samples between two positions in seconds, every lane together;
    /// (0, 0) outside the file.
    pub fn range(&self, from: f64, to: f64) -> (f32, f32) {
        let rate = f64::from(self.sample_rate);
        self.column(Lane::Mixed, from * rate, to * rate).map_or((0.0, 0.0), |b| (b.min, b.max))
    }

    /// The same peaks without the samples, for keeping many files' waveforms without their
    /// audio (the browser's cache): close up, they read the finest level.
    pub fn without_samples(&self) -> Self {
        Self { data: None, ..self.clone() }
    }

    /// Whether it can be drawn sample by sample.
    pub fn has_samples(&self) -> bool {
        self.data.is_some()
    }
}

/// How sample values map onto the height of a lane.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Scale {
    /// Full scale at the edges, silence in the middle.
    #[default]
    Linear,
    /// Decibels: full scale at the edges, [`DB_RANGE`] below it in the middle, so quiet parts
    /// show.
    Db,
}

impl Scale {
    /// `value` (−1 … 1, more when it clips) as a share of half the lane, −1 … 1.
    pub fn apply(self, value: f32) -> f32 {
        let value = if value.is_finite() { value.clamp(-1.0, 1.0) } else { 0.0 };
        match self {
            Self::Linear => value,
            Self::Db => {
                let magnitude = value.abs();
                if magnitude <= 0.0 {
                    return 0.0;
                }
                let db = 20.0 * magnitude.log10();
                ((db + DB_RANGE) / DB_RANGE).clamp(0.0, 1.0).copysign(value)
            }
        }
    }
}

/// One pixel column to draw: the stretch of the file it shows, in seconds, and the gain on it
/// (a clip's gain and fades).
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Column {
    pub x: f32,
    pub from: f64,
    pub to: f64,
    pub gain: f32,
}

/// A column of the envelope, in points: the peak span and the RMS span inside it.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Span {
    pub x: f32,
    pub top: f32,
    pub bottom: f32,
    pub rms_top: f32,
    pub rms_bottom: f32,
}

/// What is drawn in one lane.
#[derive(Debug, Clone, PartialEq)]
pub enum LaneShape {
    /// Peaks and RMS per column; runs of columns inside the file, broken where it is not.
    Envelope(Vec<Vec<Span>>),
    /// The samples joined by a line, with a dot on each when they are far enough apart.
    Samples { points: Vec<Pos2>, dots: bool },
}

/// A waveform ready to paint: one shape per lane, and the zero line of each lane.
#[derive(Debug, Clone, PartialEq)]
pub struct Geometry {
    pub lanes: Vec<(f32, LaneShape)>,
}

/// Lays `peaks` out in `rect` over `columns` (left to right, one point apart): lanes, the
/// envelope from the level closest to the zoom, or the samples when a sample is wider than a
/// column.
pub fn geometry(peaks: &Peaks, rect: Rect, columns: &[Column], scale: Scale) -> Geometry {
    let _work_trace = crate::kit::frame_trace::WorkSpan::new("waveform geometry");
    let lanes: Vec<Lane> = if peaks.lanes >= 2 && rect.height() >= STEREO_MIN_HEIGHT {
        (0..peaks.lanes).map(Lane::Channel).collect()
    } else {
        vec![Lane::Mixed]
    };
    let rate = f64::from(peaks.sample_rate);
    let gap = if lanes.len() > 1 { 2.0 } else { 0.0 };
    let height = ((rect.height() - gap * (lanes.len() - 1) as f32) / lanes.len() as f32).max(1.0);
    // Frames a column covers, on average over the columns in the file.
    let widths: Vec<f64> = columns.iter().map(|c| (c.to - c.from).abs() * rate).filter(|w| w.is_finite()).collect();
    let per_column = if widths.is_empty() { 0.0 } else { widths.iter().sum::<f64>() / widths.len() as f64 };
    let close = peaks.has_samples() && per_column > 0.0 && per_column < 1.0;
    let shapes = lanes
        .iter()
        .enumerate()
        .map(|(i, lane)| {
            let top = rect.top() + i as f32 * (height + gap);
            let middle = top + height / 2.0;
            let half = height / 2.0;
            let y = |v: f32, gain: f32| middle - scale.apply(v * gain) * half;
            let shape = if close {
                samples(peaks, *lane, columns, rate, &y)
            } else {
                envelope(peaks, *lane, columns, rate, &y)
            };
            (middle, shape)
        })
        .collect();
    Geometry { lanes: shapes }
}

fn envelope(peaks: &Peaks, lane: Lane, columns: &[Column], rate: f64, y: &dyn Fn(f32, f32) -> f32) -> LaneShape {
    let mut runs: Vec<Vec<Span>> = Vec::new();
    let mut run: Vec<Span> = Vec::new();
    for column in columns {
        match peaks.column(lane, column.from * rate, column.to * rate) {
            Some(b) => {
                let top = y(b.max, column.gain);
                // At least a point tall, so silence shows as a line.
                let bottom = y(b.min, column.gain).max(top + 1.0);
                let rms = b.rms.min(b.max.abs().max(b.min.abs()));
                let rms_top = y(rms, column.gain).clamp(top, bottom);
                let rms_bottom = y(-rms, column.gain).clamp(rms_top, bottom);
                run.push(Span { x: column.x, top, bottom, rms_top, rms_bottom });
            }
            None if !run.is_empty() => runs.push(std::mem::take(&mut run)),
            None => {}
        }
    }
    if !run.is_empty() {
        runs.push(run);
    }
    LaneShape::Envelope(runs)
}

fn samples(peaks: &Peaks, lane: Lane, columns: &[Column], rate: f64, y: &dyn Fn(f32, f32) -> f32) -> LaneShape {
    let mut points = Vec::new();
    let mut spacing = f32::MAX;
    let mut last_frame = None;
    for column in columns {
        let (from, to) = (column.from * rate, column.to * rate);
        if !(from.is_finite() && to.is_finite()) || to <= from {
            continue;
        }
        // Every sample in this column, placed by its time within the column (one point wide).
        let first = from.ceil().max(0.0) as usize;
        let last = to.ceil().max(0.0) as usize;
        let per_point = (to - from) as f32;
        spacing = spacing.min(1.0 / per_point.max(f32::EPSILON));
        for frame in first..last {
            if last_frame.is_some_and(|l| frame <= l) {
                continue;
            }
            let Some(value) = peaks.sample(lane, frame) else { continue };
            let x = column.x + ((frame as f64 - from) / (to - from)) as f32;
            points.push(Pos2::new(x, y(value, column.gain)));
            last_frame = Some(frame);
        }
    }
    LaneShape::Samples { points, dots: spacing >= DOT_SPACING }
}

/// The colours of a waveform: the peaks, and the RMS band drawn over them.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Colors {
    pub peak: Color32,
    pub rms: Color32,
}

impl Colors {
    /// From one colour: the peaks lighter (more see-through), the RMS band in the colour itself.
    pub fn from(color: Color32) -> Self {
        Self { peak: color.gamma_multiply(0.55), rms: color }
    }
}

/// Paints `geometry`: each run of the envelope as a filled shape with anti-aliased edges and the
/// RMS band inside it, or the samples as a line with dots.
pub fn paint(painter: &Painter, geometry: &Geometry, colors: Colors) {
    let _work_trace = crate::kit::frame_trace::WorkSpan::new("waveform paint");
    for (_, shape) in &geometry.lanes {
        match shape {
            LaneShape::Envelope(runs) => {
                for run in runs {
                    painter.add(Shape::mesh(band(run, |s| (s.top, s.bottom), colors.peak)));
                    painter.add(Shape::mesh(band(run, |s| (s.rms_top, s.rms_bottom), colors.rms)));
                    if run.len() > 1 {
                        // The edges, drawn as lines so they are smoothed.
                        let edge = PathStroke::new(1.0, colors.peak);
                        let top: Vec<Pos2> = run.iter().map(|s| Pos2::new(s.x, s.top)).collect();
                        let bottom: Vec<Pos2> = run.iter().map(|s| Pos2::new(s.x, s.bottom)).collect();
                        painter.add(Shape::Path(PathShape::line(top, edge.clone())));
                        painter.add(Shape::Path(PathShape::line(bottom, edge)));
                    }
                }
            }
            LaneShape::Samples { points, dots } => {
                if points.len() > 1 {
                    painter.add(Shape::line(points.clone(), Stroke::new(1.0, colors.rms)));
                }
                if *dots {
                    for point in points {
                        painter.circle_filled(*point, 1.5, colors.rms);
                    }
                }
            }
        }
    }
}

/// A strip between two edges of `run`, one quad per pair of columns; a single column is a
/// point-wide bar.
fn band(run: &[Span], edges: impl Fn(&Span) -> (f32, f32), color: Color32) -> Mesh {
    let mut mesh = Mesh::default();
    if let [only] = run {
        let (top, bottom) = edges(only);
        mesh.add_colored_rect(Rect::from_x_y_ranges(only.x - 0.5..=only.x + 0.5, top..=bottom), color);
        return mesh;
    }
    mesh.reserve_vertices(run.len() * 2);
    mesh.reserve_triangles(run.len().saturating_sub(1) * 2);
    for (i, span) in run.iter().enumerate() {
        let (top, bottom) = edges(span);
        mesh.colored_vertex(Pos2::new(span.x, top), color);
        mesh.colored_vertex(Pos2::new(span.x, bottom), color);
        if i > 0 {
            let (a, b, c, d) = ((i as u32 - 1) * 2, (i as u32 - 1) * 2 + 1, i as u32 * 2, i as u32 * 2 + 1);
            mesh.add_triangle(a, b, c);
            mesh.add_triangle(c, b, d);
        }
    }
    mesh
}

/// Columns for a stretch of a file shown at a steady zoom: `count` columns from `x`, starting
/// at `from` seconds, `seconds_per_point` each, at `gain`.
pub fn steady_columns(x: f32, count: usize, from: f64, seconds_per_point: f64, gain: f32) -> Vec<Column> {
    (0..count)
        .map(|i| {
            let start = from + i as f64 * seconds_per_point;
            Column { x: x + i as f32 + 0.5, from: start, to: start + seconds_per_point, gain }
        })
        .collect()
}

/// The scale waveforms are drawn with, as the window's settings choose it for this frame.
pub fn scale(ctx: &egui::Context) -> Scale {
    ctx.data(|d| d.get_temp::<Scale>(egui::Id::new("waveform-scale"))).unwrap_or_default()
}

/// Draws waveforms with `scale` from now on.
pub fn set_scale(ctx: &egui::Context, scale: Scale) {
    ctx.data_mut(|d| d.insert_temp(egui::Id::new("waveform-scale"), scale));
}

#[cfg(test)]
mod tests;
