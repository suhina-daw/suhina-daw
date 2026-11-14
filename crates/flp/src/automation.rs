// SPDX-License-Identifier: GPL-3.0-or-later
// Copyright (C) 2026 ItsAlphaNeon
// Copyright (C) 2026 thacer13
// Copyright (C) 2026 The Suhina contributors

//! Automation record layouts adapted from Lemon Studio os-flp/src/project.rs and
//! os-model/src/{lib,curve,clip_lfo}.rs, revision 011a03f23d9db7f10f07cd7d165cc978a9dda263.
//! https://github.com/ItsAlphaNeon/Lemon-Studio. Changes: bounded conversion into clip gain
//! envelopes and device/channel automation lanes.

use crate::{
    Flp, FlpError,
    records::{Placement, data, dword, error, number, word},
};
use std::collections::{BTreeMap, BTreeSet};
use suhina_model::{AutomationLane, AutomationPoint, ChannelId, DeviceKind, GainPoint, LaneData, Project};

struct Point {
    beat: f64,
    value: f64,
    hold: bool,
    mode: u8,
    tension: f32,
}
struct Curve {
    points: Vec<Point>,
    min: f64,
    max: f64,
}

fn float(d: &[u8], at: usize) -> Option<f64> {
    Some(f64::from_le_bytes(d.get(at..at.checked_add(8)?)?.try_into().ok()?))
}

fn curve(d: &[u8]) -> Option<Vec<Point>> {
    let count = usize::try_from(dword(d, 17, 0).ok()?).ok()?;
    if count == 0 || count > 4096 {
        return None;
    }
    let end = 21usize.checked_add(count.checked_mul(24)?)?;
    let tail = d.get(end..)?;
    // An active LFO cannot be represented by the base points alone.
    if tail.len() >= 48 {
        let lfo_points = usize::try_from(dword(tail, 44, 0).ok()?).ok()?;
        let at = 48usize.checked_add(lfo_points.checked_mul(24)?)?;
        if tail.len() >= at.checked_add(40)? && dword(tail, at + 4, 0).ok()? != 0 {
            return None;
        }
    }
    let mut beat = 0.;
    let mut out = Vec::with_capacity(count);
    for r in d.get(21..end)?.as_chunks::<24>().0 {
        let delta = float(r, 0)?;
        let value = float(r, 8)?;
        let tension = f32::from_bits(dword(r, 16, 0).ok()?);
        if !delta.is_finite() || delta < 0. || !value.is_finite() || !(0. ..=1.).contains(&value) {
            return None;
        }
        let hold = r[20] == 2;
        if !tension.is_finite() || !(-1. ..=1.).contains(&tension) {
            return None;
        }
        if !hold && !matches!(r[20], 0 | 1) && !(tension == 0. && matches!(r[20], 0 | 1 | 7 | 8 | 9 | 11 | 12)) {
            return None;
        }
        beat += delta;
        if !beat.is_finite() {
            return None;
        }
        out.push(Point { beat, value, hold, mode: r[20], tension });
    }
    Some(out)
}

fn value(curve: &Curve, beat: f64) -> f64 {
    let index = curve.points.partition_point(|p| p.beat <= beat);
    let raw = match (index.checked_sub(1).and_then(|i| curve.points.get(i)), curve.points.get(index)) {
        (None, Some(p)) | (Some(p), None) => p.value,
        (Some(a), Some(b)) if b.hold || b.beat <= a.beat => a.value,
        (Some(a), Some(b)) => {
            let x = (beat - a.beat) / (b.beat - a.beat);
            let shaped = if b.tension.abs() < 1e-6 {
                x
            } else {
                // Pinned os-model curve::Segment coefficients use single precision.
                let v = ((f64::from(b.tension.abs()) * 1_000_001f64.ln()).exp() - 1.) as f32;
                let c1 = f64::from((1. / f64::from(v)) as f32);
                let c2 = f64::from((f64::from(v) + 1.).ln() as f32);
                let single = |x: f64| {
                    if b.tension < 0. { ((x * c2).exp() - 1.) * c1 } else { 1. - (((1. - x) * c2).exp() - 1.) * c1 }
                };
                if b.mode == 1 {
                    if x < 0.5 { single(2. * x) / 2. } else { 1. - single(2. * (1. - x)) / 2. }
                } else {
                    single(x)
                }
            };
            a.value + (b.value - a.value) * shaped.clamp(0., 1.)
        }
        _ => 0.,
    };
    curve.min + raw * (curve.max - curve.min)
}

pub(crate) fn apply(
    flp: &Flp,
    project: &mut Project,
    placements: &[Placement],
    warnings: &mut Vec<String>,
) -> Result<(), FlpError> {
    if placements.len() > 16384 {
        return Err(error(0, "too many automation placements"));
    }
    let mut placed_by_source: BTreeMap<u32, Vec<&Placement>> = BTreeMap::new();
    for p in placements.iter().filter(|p| p.enabled) {
        placed_by_source.entry(u32::from(p.source)).or_default().push(p);
    }
    for items in placed_by_source.values_mut() {
        items.sort_by(|a, b| a.start.total_cmp(&b.start));
    }
    let ambiguous: BTreeSet<_> = placed_by_source
        .iter()
        .filter(|(_, items)| items.windows(2).any(|p| p[0].start + p[0].length > p[1].start))
        .map(|(&id, _)| id)
        .collect();
    let mut curves = BTreeMap::new();
    let mut ranges = BTreeMap::new();
    let mut channel = None;
    for e in &flp.events {
        match e.id {
            64 => channel = Some(number(e)?),
            51 | 99 | 233 | 236 | 149 | 42 => channel = None,
            219 => {
                if let Some(id) = channel {
                    let d = data(e)?;
                    ranges.insert(
                        id,
                        (f64::from(dword(d, 0, e.offset)?) / 12800., f64::from(dword(d, 4, e.offset)?) / 12800.),
                    );
                }
            }
            234 => {
                if let Some(id) = channel
                    && let Some(points) = curve(data(e)?)
                {
                    curves.insert(id, Curve { points, min: 0., max: 1. });
                }
            }
            _ => {}
        }
    }
    for (&id, curve) in &mut curves {
        if let Some(&(min, max)) = ranges.get(&id) {
            curve.min = min;
            curve.max = max;
        }
    }
    curves.retain(|_, curve| (0. ..=1.).contains(&curve.min) && (0. ..=1.).contains(&curve.max));
    let mut linked_lanes: BTreeMap<u32, Vec<u32>> = BTreeMap::new();
    let mut links: BTreeMap<u32, Vec<(u32, u16)>> = BTreeMap::new();
    for (i, e) in flp.events.iter().enumerate().filter(|(_, e)| e.id == 227) {
        let d = data(e)?;
        if word(d, 0, e.offset)? & 0x8000 != 0 {
            continue;
        }
        let target = dword(d, 8, e.offset)?;
        if flp.events.get(i + 1).is_some_and(|e| e.id == 230) {
            continue;
        }
        let source = u32::from(word(d, 2, e.offset)?);
        let parameter = target as u16;
        let sources = linked_lanes.entry(target).or_default();
        if !sources.contains(&source) {
            sources.push(source);
        }
        if target >> 16 < 0x4000 && matches!(parameter, 0 | 0x11) {
            let list = links.entry(target >> 16).or_default();
            if !list.contains(&(source, parameter)) {
                list.push((source, parameter));
            }
        }
    }
    apply_lanes(project, &linked_lanes, &curves, &placed_by_source, &ambiguous, warnings);
    let targets: BTreeMap<_, _> = project
        .contents
        .iter()
        .filter_map(|c| {
            c.lanes
                .iter()
                .find_map(|l| matches!(l.data, LaneData::Audio(_)).then_some(l.target).flatten())
                .map(|t| (c.id, t.0))
        })
        .collect();
    let mut mapped = 0;
    let mut skipped = 0;
    let mut total = 0usize;
    for clip in project.arrangement.lanes.iter_mut().flat_map(|l| &mut l.clips) {
        let Some(target) = targets.get(&clip.content) else {
            continue;
        };
        let Some(links) = links.get(target) else {
            continue;
        };
        let [(source, parameter)] = links.as_slice() else {
            skipped += 1;
            continue;
        };
        let (source, parameter) = (*source, *parameter);
        let Some(items) = placed_by_source.get(&source) else {
            continue;
        };
        if ambiguous.contains(&source) {
            skipped += 1;
            continue;
        }
        let i = items.partition_point(|p| p.start <= clip.start);
        let Some(placed) = i.checked_sub(1).and_then(|i| items.get(i)) else {
            skipped += 1;
            continue;
        };
        let Some(curve) = curves.get(&source) else {
            skipped += 1;
            continue;
        };
        if placed.start > clip.start || placed.start + placed.length < clip.start + clip.length {
            skipped += 1;
            continue;
        }
        let static_volume = ranges.get(target).map_or(10000. / 12800., |r| r.1);
        let gain_curve = |v: f64| (11f64.powf(v) - 1.) / 10.;
        let divisor = if parameter == 0 { gain_curve(static_volume) } else { 1. };
        if divisor <= 0. {
            skipped += 1;
            continue;
        }
        let from = placed.offset + clip.start - placed.start;
        let to = from + clip.length;
        let db = |at| {
            let v = value(curve, at) * if parameter == 0x11 { 2. } else { 1. };
            let gain = gain_curve(v) / divisor;
            if gain <= 0. { -70. } else { (20. * gain.log10()).clamp(-70., 24.) }
        };
        let mut times = vec![from];
        times.extend(curve.points.iter().filter(|p| p.beat > from && p.beat < to).map(|p| p.beat));
        times.push(to);
        if times.len() * 66 > 10000 || total + times.len() * 66 > 1_000_000 {
            skipped += 1;
            continue;
        }
        let mut envelope = Vec::new();
        for pair in times.windows(2) {
            let a = pair[0];
            let b = pair[1];
            for step in 0..64 {
                let beat = a + (b - a) * f64::from(step) / 64.;
                envelope.push(GainPoint::new(clip.offset + beat - from, db(beat)));
            }
            // Preserve a hold's jump with two points on its boundary.
            if curve.points.iter().any(|p| p.beat == b && p.hold) {
                envelope.push(GainPoint::new(clip.offset + b - from, db(b - (b - a) * 1e-10)));
            }
            envelope.push(GainPoint::new(clip.offset + b - from, db(b)));
        }
        total += envelope.len();
        clip.audio.gain_envelope = envelope;
        mapped += 1;
    }
    if mapped > 0 {
        warnings.push(format!(
            "Volume automation became gain envelopes on {mapped} audio clips (sampled curves; -70 to +24 dB)."
        ));
    }
    if skipped > 0 {
        warnings.push(format!("{skipped} audio clips have volume automation with unsupported curves, overlap or range; retained without playback."));
    }
    Ok(())
}

// Resolve the original address before identity assignment. Slot metadata survives native substitutions.
type Target = (usize, Option<usize>, String, Option<f64>);
fn target(project: &Project, address: u32) -> Option<Target> {
    let location = address >> 16;
    let parameter = address as u16;
    let (channel_id, slot) = if location >= 0x7000 {
        (131_072 + (location - 0x7000) / 64, Some((location - 0x7000) % 64))
    } else if (0x2000..0x4000).contains(&location) {
        (131_072 + (location - 0x2000) / 64, Some((location - 0x2000) % 64))
    } else {
        (location, None)
    };
    let ci = project.channels.iter().position(|c| c.id == ChannelId(channel_id))?;
    let channel = project.channels.get(ci)?;
    if slot.is_some() && matches!(parameter, 0x1fc0 | 0x1fc1) {
        let (name, initial) = if parameter == 0x1fc0 { ("volume", channel.volume) } else { ("pan", channel.pan) };
        return Some((ci, None, name.into(), Some(initial)));
    }
    let index = parameter.checked_sub(0x8000)?;
    let di = if let Some(slot) = slot {
        channel.devices.iter().position(|d| {
            d.extensions.0.iter().any(|e| e.format == "flp" && e.key == "mixer-slot" && e.data == slot.to_le_bytes())
        })?
    } else {
        channel.devices.iter().position(|d| matches!(d.kind, DeviceKind::Vst2 { .. } | DeviceKind::Vst3 { .. }))?
    };
    let device = channel.devices.get(di)?;
    let id = if device.extensions.0.iter().any(|e| e.format == "flp" && e.key.eq_ignore_ascii_case("Fruity Balance")) {
        match index {
            0 => "balance".into(),
            1 => "gain".into(),
            _ => return None,
        }
    } else {
        crate::wrapper::parameter_id(device, index)?
    };
    let initial = device.params.iter().find(|p| p.id == id).map(|p| p.value);
    Some((ci, Some(di), id, initial))
}

// Append without a redundant vertex, retaining two different values at a jump.
fn push_point(points: &mut Vec<AutomationPoint>, point: AutomationPoint) {
    if let Some(last) = points.last_mut()
        && last.beat == point.beat
        && last.value == point.value
    {
        last.curve = point.curve;
    } else {
        points.push(point);
    }
}

// A bounded cubic fit in normalized plug-in parameter space. Only the segment's start
// is appended; the caller supplies its end, which may also start a discontinuity.
fn editable_segment(points: &mut Vec<AutomationPoint>, at: f64, length: f64, sample: &dyn Fn(f64) -> f64, depth: u8) {
    let first = sample(0.);
    let last = sample(1.);
    let delta = last - first;
    let fitted = if delta.abs() < 1e-12 {
        Some(suhina_model::automation::Curve::STRAIGHT)
    } else {
        let a = (27. * (sample(1. / 3.) - first) / delta - 1.) / 6.;
        let b = (27. * (sample(2. / 3.) - first) / delta - 8.) / 6.;
        suhina_model::automation::Curve::new(1. / 3., (2. * a - b) / 3., 2. / 3., (2. * b - a) / 3.)
    };
    if let Some(curve) = fitted.filter(|curve| {
        (1..64).all(|i| {
            let t = f64::from(i) / 64.;
            (first + delta * curve.shape(t) - sample(t)).abs() <= 1e-4
        })
    }) {
        let mut point = AutomationPoint::new(at, first);
        point.curve = curve;
        push_point(points, point);
    } else if depth < 6 {
        editable_segment(points, at, length / 2., &|t| sample(t / 2.), depth + 1);
        editable_segment(points, at + length / 2., length / 2., &|t| sample(0.5 + t / 2.), depth + 1);
    } else {
        push_point(points, AutomationPoint::new(at, first));
    }
}

fn apply_lanes(
    project: &mut Project,
    links: &BTreeMap<u32, Vec<u32>>,
    curves: &BTreeMap<u32, Curve>,
    placed: &BTreeMap<u32, Vec<&Placement>>,
    ambiguous: &BTreeSet<u32>,
    warnings: &mut Vec<String>,
) {
    let mut total = 0usize;
    for (&address, sources) in links {
        let Some((ci, di, param, initial)) = target(project, address) else {
            // Audio volume has its own clip-local conversion below; pan is deferred.
            if !matches!(address as u16, 0 | 0x11) {
                warnings.push(format!(
                    "Automation target {address:08X} is not mapped; retained in source data without an extra track."
                ));
            }
            continue;
        };
        let items: Vec<_> =
            sources.iter().flat_map(|s| placed.get(s).into_iter().flatten().map(move |p| (*s, *p))).collect();
        if items.is_empty() {
            continue;
        }
        let mut items = items;
        items.sort_by(|a, b| a.1.start.total_cmp(&b.1.start));
        if items.windows(2).any(|p| p[0].1.start + p[0].1.length > p[1].1.start)
            || sources.iter().any(|s| ambiguous.contains(s))
        {
            warnings.push(format!("Overlapping automation for {address:08X} is retained without playback."));
            continue;
        }
        let convert = |v: f64| {
            if di.is_none() {
                if param == "volume" { (11f64.powf(v * 1.25) - 1.) / 10. } else { v * 2. - 1. }
            } else if param == "gain" {
                let gain = (11f64.powf(v * 1.25) - 1.) / 10.;
                if gain > 0. { 20. * gain.log10() } else { -120. }
            } else if param == "balance" {
                v * 2. - 1.
            } else {
                v
            }
        };
        let mut points = Vec::new();
        let mut valid = true;
        for (source, p) in items {
            let Some(c) = curves.get(&source) else {
                valid = false;
                break;
            };
            let end = p.offset + p.length;
            let mut times = vec![p.offset];
            times.extend(c.points.iter().filter(|v| v.beat > p.offset && v.beat < end).map(|v| v.beat));
            times.push(end);
            if points.len() + times.len() * 66 > 10000 {
                valid = false;
                break;
            }
            let last = points.last().map(|p: &AutomationPoint| p.value).or(initial);
            if let Some(last) = last {
                if points.is_empty() && p.start > 0. {
                    points.push(AutomationPoint::new(0., last));
                }
                push_point(&mut points, AutomationPoint::new(p.start, last));
            } else if p.start > 0. {
                // The opaque plug-in state does not expose its initial parameter value.
                warnings.push(format!("Automation {address:08X}: initial value unavailable; the first curve value holds before its first placement."));
            }
            for pair in times.windows(2) {
                let (a, b) = (pair[0], pair[1]);
                let held = c.points.iter().any(|v| v.beat == b && v.hold);
                if di.is_some() && param.parse::<u32>().is_ok() {
                    let sample =
                        |t: f64| convert(value(c, if held && t == 1. { b - (b - a) * 1e-10 } else { a + (b - a) * t }));
                    editable_segment(&mut points, p.start + a - p.offset, b - a, &sample, 0);
                } else {
                    for step in 0..64 {
                        let t = a + (b - a) * f64::from(step) / 64.;
                        push_point(&mut points, AutomationPoint::new(p.start + t - p.offset, convert(value(c, t))));
                    }
                }
                if held {
                    push_point(
                        &mut points,
                        AutomationPoint::new(p.start + b - p.offset, convert(value(c, b - (b - a) * 1e-10))),
                    );
                }
                push_point(&mut points, AutomationPoint::new(p.start + b - p.offset, convert(value(c, b))));
            }
        }
        if !valid || total + points.len() > 1_000_000 {
            warnings
                .push(format!("Automation {address:08X} exceeds supported curve limits; retained without playback."));
            continue;
        }
        total += points.len();
        let mut lane = AutomationLane::new(param);
        lane.points = points.into_iter().collect();
        if let Some(channel) = project.channels.get_mut(ci) {
            if let Some(di) = di {
                if let Some(device) = channel.devices.get_mut(di) {
                    if lane.param == "gain" {
                        // A static silent source must be allowed to rise with its gain envelope.
                        if let Some(mute) = device.params.iter_mut().find(|p| p.id == "mute") {
                            mute.value = 0.;
                        }
                    }
                    device.automation.push(lane);
                }
            } else {
                channel.automation.push(lane);
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn record(mode: u8) -> Vec<u8> {
        let mut d = vec![0; 21];
        d[4..8].copy_from_slice(&64u32.to_le_bytes());
        d[17..21].copy_from_slice(&2u32.to_le_bytes());
        for (delta, value) in [(0f64, 0.25f64), (4., 0.75)] {
            d.extend(delta.to_le_bytes());
            d.extend(value.to_le_bytes());
            d.extend(0f32.to_le_bytes());
            d.extend([mode, 0, 0, 2]);
        }
        d
    }

    #[test]
    fn editable_plugin_curves_keep_few_points_and_follow_the_source_shape() {
        use suhina_model::automation::{Plain, value_at};
        for mode in [0, 1] {
            for tension in [-1f32, -0.5, 0., 0.5, 1.] {
                let mut d = record(mode);
                d[61..65].copy_from_slice(&tension.to_le_bytes());
                let c = Curve { points: curve(&d).unwrap(), min: 0., max: 1. };
                for (start, end) in [(0., 4.), (0.5, 3.5)] {
                    let mut points = Vec::new();
                    editable_segment(&mut points, start, end - start, &|t| value(&c, start + (end - start) * t), 0);
                    push_point(&mut points, AutomationPoint::new(end, value(&c, end)));
                    assert!(points.len() <= 20, "mode={mode}, tension={tension}, points={}", points.len());
                    if tension == 0. {
                        assert_eq!(points.len(), 2);
                    }
                    for i in 0..=4096 {
                        let at = start + (end - start) * f64::from(i) / 4096.;
                        let imported = value_at(&points, at, false, &Plain).unwrap();
                        assert!((imported - value(&c, at)).abs() < 1.1e-4, "mode={mode}, tension={tension}, at={at}");
                    }
                }
            }
        }
    }

    #[test]
    fn curve_rejects_truncation_nonfinite_points_and_active_lfo() {
        let bytes = record(0);
        for end in 0..bytes.len() {
            assert!(curve(&bytes[..end]).is_none());
        }
        assert!(curve(&bytes).is_some());
        let mut invalid = bytes.clone();
        invalid[29..37].copy_from_slice(&f64::NAN.to_le_bytes());
        assert!(curve(&invalid).is_none());
        let mut active = bytes;
        active.extend([0; 88]);
        active[69 + 48 + 4..69 + 48 + 8].copy_from_slice(&(-1i32).to_le_bytes());
        assert!(curve(&active).is_none());
    }

    #[test]
    fn curved_segments_keep_direction_midpoint_and_endpoints() {
        for mode in [0, 1] {
            for tension in [-0.5f32, 0.5, f32::MIN_POSITIVE] {
                let mut d = record(mode);
                d[61..65].copy_from_slice(&tension.to_le_bytes());
                let c = Curve { points: curve(&d).unwrap(), min: 0., max: 1. };
                assert_eq!(value(&c, 0.), 0.25);
                assert_eq!(value(&c, 4.), 0.75);
                let mid = value(&c, 2.);
                assert!(mid.is_finite() && (0.25..0.75).contains(&mid));
                if mode == 1 {
                    assert!((mid - 0.5).abs() < 1e-5);
                } else if tension == 0.5 {
                    assert!(mid > 0.5);
                } else if tension == -0.5 {
                    assert!(mid < 0.5);
                }
            }
        }
    }

    #[test]
    fn holds_change_at_boundary_and_linear_curves_follow_trimmed_time() {
        let held = Curve { points: curve(&record(2)).unwrap(), min: 0., max: 1. };
        assert_eq!(value(&held, 3.999), 0.25);
        assert_eq!(value(&held, 4.), 0.75);
        let linear = Curve { points: curve(&record(0)).unwrap(), min: 0.2, max: 0.8 };
        assert!((value(&linear, 1.) - 0.425).abs() < 1e-10);
        assert!((value(&linear, 3.) - 0.575).abs() < 1e-10);
    }
}
