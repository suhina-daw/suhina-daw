// Copyright (C) 2026 The Suhina contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! Checks every device must pass, whatever it does. Each device crate calls [`check`] from a
//! test; the checks cover the [`Device`] contract, not the device's sound. Instruments also go
//! through note checks: a fixed performance played in blocks of every size, `reset` while notes
//! sound, hostile events and every sample rate. MIDI effects go through event checks: the same
//! events in spans of every size, at every parameter's ends, `reset`, hostile events,
//! all-notes-off and too little room to write, and every note they start is ended, also with
//! room for no more events than they get. Effects and instruments are prepared again at other
//! rates and must then sound as a fresh device does, and an effect's impulse must arrive when
//! its [`Device::latency`] says.
//!
//! The registry runs these checks on every built-in device; a device crate runs them on its own
//! with [`conformance_test!`](crate::conformance_test).

use crate::{Device, MusicalScale, NoteEvent, ParamKind, ProcessContext, Role, controller};

/// Frames of test signal per check.
const FRAMES: usize = 8_192;
/// Largest difference two renders of the same input may have and still count as equal.
const TOLERANCE: f32 = 1e-6;

/// Runs every check on devices made by `make` and returns what failed (empty when all pass).
pub fn check(make: &dyn Fn() -> Box<dyn Device>) -> Vec<String> {
    let mut failures = Vec::new();
    check_info(make().as_ref(), &mut failures);
    check_params(make().as_mut(), &mut failures);
    check_hostile_values(make, &mut failures);
    check_block_sizes(make, &mut failures);
    check_reset(make, &mut failures);
    check_sample_rates(make, &mut failures);
    match make().info().role {
        Role::Instrument => {
            check_notes(make, &mut failures);
            check_prepare_again(make, &mut failures);
        }
        Role::MidiEffect => check_events(make, &mut failures),
        Role::Effect => {
            check_prepare_again(make, &mut failures);
            check_latency(make, &mut failures);
        }
    }
    failures
}

/// A `#[test]` that runs [`check`] on the devices an expression makes and fails with the list of
/// what failed. For a device crate's tests:
///
/// ```ignore
/// suhina_device_api::conformance_test!(Utility::new());
/// ```
#[macro_export]
macro_rules! conformance_test {
    ($make:expr) => {
        #[test]
        fn passes_the_device_conformance_checks() {
            let failures = $crate::conformance::check(&|| ::std::boxed::Box::new($make));
            assert!(failures.is_empty(), "{failures:#?}");
        }
    };
}

/// Interleaved stereo test signal: noise plus a low sine, different on each side, with a DC
/// offset, so filters, stereo processing and DC removal all have something to do.
pub fn test_signal(frames: usize) -> Vec<f32> {
    let mut seed = 0x2545_f491_u32;
    let mut out = Vec::with_capacity(frames * 2);
    for n in 0..frames {
        let sine = (std::f32::consts::TAU * 110.0 * n as f32 / 48_000.0).sin();
        for side in [0.3f32, -0.2] {
            seed = seed.wrapping_mul(1_664_525).wrapping_add(1_013_904_223);
            let noise = (seed >> 8) as f32 / (1u32 << 24) as f32 * 2.0 - 1.0;
            out.push(0.4 * sine * side.signum() + 0.3 * noise + 0.05 + side * 0.1);
        }
    }
    out
}

fn check_info(device: &dyn Device, failures: &mut Vec<String>) {
    let info = device.info();
    if info.id.is_empty() || info.name.is_empty() {
        failures.push("device id and name must not be empty".into());
    }
    for (i, p) in info.params.iter().enumerate() {
        if p.id.is_empty() || info.params.iter().take(i).any(|q| q.id == p.id) {
            failures.push(format!("parameter {i}: id {:?} is empty or used twice", p.id));
        }
        if p.min.partial_cmp(&p.max).is_none_or(|o| o.is_gt()) || p.clamp(p.default) != p.default {
            failures.push(format!("parameter {:?}: default {} is outside {}..{}", p.id, p.default, p.min, p.max));
        }
        if p.group.len() > 64 || p.group.trim() != p.group {
            failures.push(format!("parameter {:?}: group {:?} is longer than 64 bytes or padded", p.id, p.group));
        }
        if let ParamKind::Choice(options) = p.kind
            && options.is_empty()
        {
            failures.push(format!("parameter {:?}: a choice needs options", p.id));
        }
    }
}

fn check_params(device: &mut dyn Device, failures: &mut Vec<String>) {
    let params = device.info().params;
    for (i, p) in params.iter().enumerate() {
        if device.param(i) != Some(p.default) {
            failures.push(format!("parameter {:?} starts at {:?}, not its default", p.id, device.param(i)));
        }
        for value in [f64::NAN, f64::INFINITY, -1e300, 1e300, p.min, p.max, (p.min + p.max) / 2.0] {
            device.set_param(i, value);
            if device.param(i) != Some(p.clamp(value)) {
                failures.push(format!("parameter {:?} set to {value} reads back {:?}", p.id, device.param(i)));
            }
        }
    }
    device.set_param(params.len(), 1.0);
    device.set_param(usize::MAX, 1.0);
    if device.param(params.len()).is_some() {
        failures.push("an unknown parameter index returns a value".into());
    }
}

/// Settings that push every parameter to its ends, in turn, while the device runs.
fn extreme_settings(device: &mut dyn Device, step: usize) {
    for (i, p) in device.info().params.iter().enumerate() {
        let value = match (step + i) % 4 {
            0 => p.min,
            1 => p.max,
            2 => p.default,
            _ => f64::NAN,
        };
        device.set_param(i, value);
    }
}

fn check_hostile_values(make: &dyn Fn() -> Box<dyn Device>, failures: &mut Vec<String>) {
    let mut device = make();
    let mut signal = test_signal(FRAMES);
    for (step, block) in signal.chunks_mut(512).enumerate() {
        extreme_settings(device.as_mut(), step);
        device.process(&ProcessContext::default(), block);
    }
    if let Some(n) = signal.iter().position(|s| !s.is_finite() || s.abs() > 1e4) {
        failures.push(format!("extreme parameters produce {:?} at sample {n}", signal.get(n)));
    }
    let mut silly = [f32::NAN, f32::INFINITY, 1e30, -1e30, 0.5];
    device.process(&ProcessContext { tempo: f64::NAN, beat: -1e300, playing: true }, &mut silly);
    let mut after = test_signal(FRAMES);
    device.process(&ProcessContext::default(), &mut after);
    if after.iter().skip(FRAMES).any(|s| !s.is_finite()) {
        failures.push("non-finite input leaves the device producing non-finite output".into());
    }
    let mut odd = [0.1f32, 0.2, 0.7];
    device.reset();
    device.process(&ProcessContext::default(), &mut odd);
    if odd.get(2) != Some(&0.7) {
        failures.push("a trailing odd sample was changed".into());
    }
}

/// Renders the test signal through a fresh device in blocks of `sizes` (repeated), changing
/// parameters at fixed frames.
fn render(make: &dyn Fn() -> Box<dyn Device>, sizes: &[usize]) -> Vec<f32> {
    let mut device = make();
    let mut signal = test_signal(FRAMES);
    let tempo = 120.0;
    let mut frame = 0usize;
    for &size in sizes.iter().cycle() {
        if frame >= FRAMES || size == 0 {
            break;
        }
        let size = size.min(FRAMES - frame);
        // Every 1024 frames the parameters change; blocks are split there, as the host does.
        let size = size.min(1_024 - frame % 1_024);
        if frame.is_multiple_of(1_024) {
            extreme_settings(device.as_mut(), frame / 1_024);
        }
        let beat = frame as f64 / 48_000.0 * tempo / 60.0;
        if let Some(block) = signal.get_mut(frame * 2..(frame + size) * 2) {
            device.process(&ProcessContext { tempo, beat, playing: true }, block);
        }
        frame += size;
    }
    signal
}

fn largest_difference(a: &[f32], b: &[f32]) -> f32 {
    a.iter().zip(b).map(|(x, y)| (x - y).abs()).fold(0.0, f32::max)
}

fn check_block_sizes(make: &dyn Fn() -> Box<dyn Device>, failures: &mut Vec<String>) {
    let reference = render(make, &[512]);
    for sizes in [&[1][..], &[7, 64, 333], &[1_024], &[3, 1, 250, 900]] {
        let other = render(make, sizes);
        let diff = largest_difference(&reference, &other);
        if diff > TOLERANCE {
            failures.push(format!("blocks of {sizes:?} differ from blocks of 512 by {diff}"));
        }
    }
}

fn check_reset(make: &dyn Fn() -> Box<dyn Device>, failures: &mut Vec<String>) {
    let fresh = {
        let mut device = make();
        let mut signal = test_signal(FRAMES);
        device.process(&ProcessContext::default(), &mut signal);
        signal
    };
    let mut device = make();
    let mut warmup = test_signal(FRAMES);
    warmup.reverse();
    device.process(&ProcessContext::default(), &mut warmup);
    device.reset();
    let mut signal = test_signal(FRAMES);
    device.process(&ProcessContext::default(), &mut signal);
    let diff = largest_difference(&fresh, &signal);
    if diff > TOLERANCE {
        failures.push(format!("after reset the output differs from a fresh device by {diff}"));
    }
}

fn check_sample_rates(make: &dyn Fn() -> Box<dyn Device>, failures: &mut Vec<String>) {
    let mut device = make();
    for sr in [8_000.0, 22_050.0, 44_100.0, 96_000.0, 192_000.0, 384_000.0] {
        if let Err(e) = device.prepare(sr) {
            failures.push(format!("prepare({sr}) failed: {e}"));
        }
        let mut signal = test_signal(1_024);
        device.process(&ProcessContext::default(), &mut signal);
        if signal.iter().any(|s| !s.is_finite()) {
            failures.push(format!("non-finite output at {sr} Hz"));
        }
    }
    for bad in [0.0, -1.0, f64::NAN, 1e12] {
        if device.prepare(bad).is_ok() {
            failures.push(format!("prepare({bad}) succeeded"));
        }
    }
    let mut signal = test_signal(1_024);
    device.process(&ProcessContext::default(), &mut signal);
    if signal.iter().any(|s| !s.is_finite()) {
        failures.push("a rejected sample rate left the device broken".into());
    }
}

/// What a device sends out for `frames` frames: an effect the test signal processed, an
/// instrument the test notes played, in blocks of 512.
fn output(device: &mut dyn Device, frames: usize) -> Vec<f32> {
    if device.info().role == Role::Instrument {
        return play(device, &test_notes(), &[512], frames);
    }
    let mut signal = test_signal(frames);
    for block in signal.chunks_mut(1_024) {
        device.process(&ProcessContext::default(), block);
    }
    signal
}

/// Preparing again, at another rate and then at the first, leaves a device as it was made: the
/// same output as a fresh one, whatever it ran before.
fn check_prepare_again(make: &dyn Fn() -> Box<dyn Device>, failures: &mut Vec<String>) {
    let fresh = output(make().as_mut(), FRAMES);
    let mut device = make();
    for sr in [96_000.0, 44_100.0, 48_000.0] {
        output(device.as_mut(), 2_048);
        if let Err(e) = device.prepare(sr) {
            failures.push(format!("prepare({sr}) after another rate failed: {e}"));
            return;
        }
    }
    let diff = largest_difference(&fresh, &output(device.as_mut(), FRAMES));
    if diff > TOLERANCE {
        failures.push(format!(
            "prepared at 96 kHz, 44.1 kHz and again at 48 kHz, the output differs from a fresh device's by {diff}"
        ));
    }
}

/// How far from [`Device::latency`] an effect's first sound of an impulse may fall, in frames:
/// oversampling filters ring a frame or two early.
const LATENCY_SLACK: u32 = 4;
/// A frame is the impulse's first sound when it is louder than this (-40 dB under the impulse).
const IMPULSE_HEARD: f32 = 0.005;

/// At its defaults an effect reports the delay it adds: an impulse is first heard within
/// [`LATENCY_SLACK`] frames of [`Device::latency`], at 44.1, 48 and 96 kHz. A device reporting
/// too little puts its tracks late; one reporting too much puts them early.
fn check_latency(make: &dyn Fn() -> Box<dyn Device>, failures: &mut Vec<String>) {
    for sr in [44_100.0, 48_000.0, 96_000.0] {
        let mut device = make();
        if device.prepare(sr).is_err() {
            continue;
        }
        let latency = device.latency();
        let frames = (latency as usize).saturating_add(FRAMES);
        let mut signal = vec![0.0f32; frames * 2];
        if let Some(first) = signal.get_mut(..2) {
            first.copy_from_slice(&[0.5, 0.5]);
        }
        for block in signal.chunks_mut(1_024) {
            device.process(&ProcessContext::default(), block);
        }
        let heard = signal.iter().position(|s| s.abs() > IMPULSE_HEARD).map(|n| (n / 2) as u32);
        match heard {
            Some(at) if at.abs_diff(latency) <= LATENCY_SLACK => {}
            Some(at) => {
                failures.push(format!("at {sr} Hz an impulse is first heard at frame {at}, but latency() is {latency}"))
            }
            None => failures.push(format!("at {sr} Hz an impulse is never heard at the defaults")),
        }
    }
}

/// A short performance for instruments, as (frame, event): overlapping notes on and off frames
/// that fall inside blocks, the sustain pedal, the bend wheel, the modulation wheel and pressure.
pub fn test_notes() -> Vec<(usize, NoteEvent)> {
    use NoteEvent::*;
    vec![
        (0, On { key: 60, velocity: 0.8 }),
        (100, On { key: 64, velocity: 0.5 }),
        (517, On { key: 67, velocity: 1.0 }),
        (1_000, PitchBend(0.3)),
        (1_500, Controller { number: controller::MODULATION, value: 0.7 }),
        (2_000, Off { key: 60, velocity: 0.5 }),
        (2_600, Controller { number: controller::SUSTAIN, value: 1.0 }),
        (2_700, Off { key: 64, velocity: 0.5 }),
        (3_000, Pressure(0.6)),
        (3_500, Off { key: 67, velocity: 0.2 }),
        (4_100, Controller { number: controller::SUSTAIN, value: 0.0 }),
        (4_500, On { key: 48, velocity: 0.9 }),
        (4_501, On { key: 48, velocity: 0.9 }),
        (5_000, PitchBend(-1.0)),
        (6_000, AllNotesOff),
    ]
}

/// Plays `notes` on `device` into `frames` frames of silence, in blocks of `sizes` (repeated),
/// split where an event falls, as the host does.
fn play(device: &mut dyn Device, notes: &[(usize, NoteEvent)], sizes: &[usize], frames: usize) -> Vec<f32> {
    let mut out = vec![0.0f32; frames * 2];
    let mut frame = 0usize;
    let mut next = 0usize;
    for &size in sizes.iter().cycle() {
        if frame >= frames || size == 0 {
            break;
        }
        while let Some((at, event)) = notes.get(next)
            && *at <= frame
        {
            device.event(*event);
            next += 1;
        }
        let until = notes.get(next).map_or(frames, |(at, _)| (*at).min(frames));
        let size = size.min(until - frame).max(1).min(frames - frame);
        if let Some(block) = out.get_mut(frame * 2..(frame + size) * 2) {
            device.process(&ProcessContext::default(), block);
        }
        frame += size;
    }
    out
}

fn check_notes(make: &dyn Fn() -> Box<dyn Device>, failures: &mut Vec<String>) {
    let notes = test_notes();
    let reference = play(make().as_mut(), &notes, &[512], FRAMES);
    if !reference.iter().any(|s| s.abs() > 1e-3) {
        failures.push("the test notes make no sound".into());
    }
    if let Some(n) = reference.iter().position(|s| !s.is_finite() || s.abs() > 1e4) {
        failures.push(format!("the test notes produce {:?} at sample {n}", reference.get(n)));
    }
    for sizes in [&[1][..], &[7, 64, 333], &[1_024], &[3, 1, 250, 900]] {
        let other = play(make().as_mut(), &notes, sizes, FRAMES);
        let diff = largest_difference(&reference, &other);
        if diff > TOLERANCE {
            failures.push(format!("notes in blocks of {sizes:?} differ from blocks of 512 by {diff}"));
        }
    }

    // Reset while notes sound, with the pedal down and the wheels moved, then play again.
    let mut device = make();
    let held = [
        (0, NoteEvent::Controller { number: controller::SUSTAIN, value: 1.0 }),
        (0, NoteEvent::PitchBend(0.8)),
        (0, NoteEvent::On { key: 30, velocity: 1.0 }),
        (10, NoteEvent::On { key: 90, velocity: 1.0 }),
    ];
    play(device.as_mut(), &held, &[256], 4_096);
    device.reset();
    let after = play(device.as_mut(), &notes, &[512], FRAMES);
    let diff = largest_difference(&reference, &after);
    if diff > TOLERANCE {
        failures.push(format!("after reset with notes sounding the output differs from a fresh device by {diff}"));
    }

    // Hostile events: keys out of range, numbers that are not finite, far more notes than
    // any polyphony.
    let mut hostile = Vec::new();
    for (i, value) in [f32::NAN, f32::INFINITY, f32::NEG_INFINITY, -5.0, 5.0, 1e30, 0.5].into_iter().enumerate() {
        hostile.push((i, NoteEvent::On { key: 200, velocity: value }));
        hostile.push((i, NoteEvent::On { key: 61, velocity: value }));
        hostile.push((i, NoteEvent::Off { key: 255, velocity: value }));
        hostile.push((i, NoteEvent::PitchBend(value)));
        hostile.push((i, NoteEvent::Pressure(value)));
        for number in [0, controller::MODULATION, controller::SUSTAIN, controller::PORTAMENTO_TIME, 127, 255] {
            hostile.push((i, NoteEvent::Controller { number, value }));
        }
    }
    for round in 0..3 {
        for key in 0..=127u8 {
            hostile.push((10 + round * 200 + usize::from(key), NoteEvent::On { key, velocity: 1.0 }));
        }
    }
    let mut device = make();
    let loud = play(device.as_mut(), &hostile, &[512], FRAMES);
    if let Some(n) = loud.iter().position(|s| !s.is_finite() || s.abs() > 1e4) {
        failures.push(format!("hostile events produce {:?} at sample {n}", loud.get(n)));
    }

    // At the default settings, every note ends after all notes are released.
    let mut device = make();
    play(device.as_mut(), &notes, &[512], FRAMES);
    device.event(NoteEvent::AllNotesOff);
    let mut released = vec![0.0f32; 480_000 * 2];
    for block in released.chunks_mut(1_024) {
        device.process(&ProcessContext::default(), block);
    }
    let end = released.len().saturating_sub(2_048);
    if released.iter().skip(end).any(|s| s.abs() > 1e-4) {
        failures.push("notes still sound ten seconds after all notes are released".into());
    }

    let mut device = make();
    for sr in [8_000.0, 22_050.0, 44_100.0, 96_000.0, 192_000.0, 384_000.0] {
        if device.prepare(sr).is_err() {
            continue;
        }
        let out = play(device.as_mut(), &notes, &[512], 2_048);
        if out.iter().any(|s| !s.is_finite() || s.abs() > 1e4) {
            failures.push(format!("notes at {sr} Hz produce non-finite or runaway output"));
        }
    }
}

/// The test notes with scale changes among them, for MIDI effects: notes that start under one
/// scale and end under another, and a clip without a scale.
pub fn test_events() -> Vec<(usize, NoteEvent)> {
    let major = MusicalScale::new(0, &[0, 2, 4, 5, 7, 9, 11]);
    let pentatonic = MusicalScale::new(9, &[0, 3, 5, 7, 10]);
    let mut events = test_notes();
    events.extend([
        (50, NoteEvent::Scale(major)),
        (1_200, NoteEvent::On { key: 61, velocity: 0.4 }),
        (2_400, NoteEvent::Scale(pentatonic)),
        (2_401, NoteEvent::On { key: 0, velocity: 0.7 }),
        (2_402, NoteEvent::On { key: 127, velocity: 0.7 }),
        (2_900, NoteEvent::Off { key: 61, velocity: 0.1 }),
        (3_300, NoteEvent::Scale(None)),
        (3_301, NoteEvent::Off { key: 0, velocity: 0.3 }),
        (3_302, NoteEvent::Off { key: 127, velocity: 0.3 }),
        (5_500, NoteEvent::On { key: 70, velocity: 0.0 }),
        (6_500, NoteEvent::On { key: 72, velocity: 0.6 }),
        (7_000, NoteEvent::Off { key: 72, velocity: 0.6 }),
    ]);
    events.sort_by_key(|(at, _)| *at);
    events
}

/// The room [`run_events`] gives a span of `n` events: twice as many, as a chain gives when its
/// block has space, or only as many, the least a chain gives (ADR 0004 › 14).
const TWICE: fn(usize) -> usize = |n| n * 2;
const ONCE: fn(usize) -> usize = |n| n;

/// Runs `events` through a MIDI effect over `frames` frames, in spans of `sizes` (repeated),
/// setting the parameters to their ends every 1024 frames as [`render`] does, with the span split
/// there, with `room(n)` events of room for a span of `n` events. Returns what it sent on, at
/// frames from the start.
fn run_events(
    device: &mut dyn Device,
    events: &[(usize, NoteEvent)],
    sizes: &[usize],
    frames: usize,
    room: fn(usize) -> usize,
) -> Vec<(usize, NoteEvent)> {
    let mut out = Vec::new();
    let mut frame = 0usize;
    let mut next = 0usize;
    let mut input = Vec::new();
    for &size in sizes.iter().cycle() {
        if frame >= frames || size == 0 {
            break;
        }
        let size = size.min(frames - frame).min(1_024 - frame % 1_024);
        if frame.is_multiple_of(1_024) {
            extreme_settings(device, frame / 1_024);
        }
        input.clear();
        while let Some(&(at, event)) = events.get(next)
            && at < frame + size
        {
            input.push((at.saturating_sub(frame), event));
            next += 1;
        }
        let mut output = vec![(0, NoteEvent::AllNotesOff); room(input.len())];
        let written = device.process_events(&ProcessContext::default(), size, &input, &mut output);
        out.extend(output.iter().take(written).map(|&(at, event)| (at + frame, event)));
        frame += size;
    }
    out
}

/// What is wrong with a MIDI effect's output `out` over `frames` frames: frames out of order or
/// range, values out of range, and notes started and never ended.
fn event_faults(out: &[(usize, NoteEvent)], frames: usize) -> Vec<String> {
    let mut faults = Vec::new();
    // Whether each key sounds; a key started again while it sounds is still one note to end.
    let mut sounding = [false; 128];
    let mut last = 0usize;
    for &(at, event) in out {
        if at < last || at >= frames {
            faults.push(format!("{event:?} at frame {at} is out of order or past the end"));
        }
        last = at;
        let fine = |v: f32| (0.0..=1.0).contains(&v);
        match event {
            NoteEvent::On { key, velocity } | NoteEvent::Off { key, velocity } if key > 127 || !fine(velocity) => {
                faults.push(format!("{event:?} at frame {at} is out of range"));
            }
            _ => follow(&mut sounding, event),
        }
    }
    for (key, _) in sounding.iter().enumerate().filter(|(_, sounds)| **sounds) {
        faults.push(format!("key {key} still sounds after every key is released"));
    }
    faults
}

/// Follows `event` in `sounding`, the keys that sound: a note-on with a velocity above 0 starts
/// its key, any other note-on or a note-off ends it, all-notes-off ends every key.
fn follow(sounding: &mut [bool; 128], event: NoteEvent) {
    match event {
        NoteEvent::On { key, velocity } => {
            if let Some(slot) = sounding.get_mut(usize::from(key)) {
                *slot = velocity > 0.0;
            }
        }
        NoteEvent::Off { key, .. } => {
            if let Some(slot) = sounding.get_mut(usize::from(key)) {
                *slot = false;
            }
        }
        NoteEvent::AllNotesOff => *sounding = [false; 128],
        _ => {}
    }
}

/// `events` with every key still held at the end released one by one at the last frame, so a
/// note the effect leaves hanging shows (an all-notes-off would hide it).
fn released(mut events: Vec<(usize, NoteEvent)>) -> Vec<(usize, NoteEvent)> {
    let mut held = [false; 128];
    for &(_, event) in &events {
        follow(&mut held, event);
    }
    for key in (0..=127u8).filter(|&key| held.get(usize::from(key)) == Some(&true)) {
        events.push((FRAMES - 1, NoteEvent::Off { key, velocity: 0.5 }));
    }
    events
}

/// Runs the MIDI-effect checks on the device `make` builds: the test events, then hostile ones,
/// all-notes-off and too little output room, each in several block sizes; adds a failure per
/// fault it finds.
fn check_events(make: &dyn Fn() -> Box<dyn Device>, failures: &mut Vec<String>) {
    let mut events = test_events();
    events.retain(|(_, event)| !matches!(event, NoteEvent::AllNotesOff));
    let events = released(events);
    let reference = run_events(make().as_mut(), &events, &[512], FRAMES, TWICE);
    for fault in event_faults(&reference, FRAMES) {
        failures.push(format!("the test events: {fault}"));
    }
    if !reference.iter().any(|(_, event)| matches!(event, NoteEvent::On { .. })) {
        failures.push("the test events send no note on".into());
    }
    for sizes in [&[1][..], &[7, 64, 333], &[1_024], &[3, 1, 250, 900]] {
        let other = run_events(make().as_mut(), &events, sizes, FRAMES, TWICE);
        if other != reference {
            failures.push(format!("events in spans of {sizes:?} differ from spans of 512"));
        }
    }

    // Reset while notes are held, then the same events again.
    let mut device = make();
    let held = [(0, NoteEvent::On { key: 30, velocity: 1.0 }), (10, NoteEvent::On { key: 90, velocity: 1.0 })];
    run_events(device.as_mut(), &held, &[256], 1_024, TWICE);
    device.reset();
    if run_events(device.as_mut(), &events, &[512], FRAMES, TWICE) != reference {
        failures.push("after reset with notes held the events differ from a fresh device's".into());
    }

    // Hostile events, and an output with no room to spare.
    let mut hostile = Vec::new();
    for (i, value) in [f32::NAN, f32::INFINITY, f32::NEG_INFINITY, -5.0, 5.0, 1e30, 0.5].into_iter().enumerate() {
        hostile.push((i, NoteEvent::On { key: 200, velocity: value }));
        hostile.push((i, NoteEvent::On { key: 61, velocity: value }));
        hostile.push((i, NoteEvent::Off { key: 255, velocity: value }));
        hostile.push((i, NoteEvent::PitchBend(value)));
        hostile.push((i, NoteEvent::Controller { number: 255, value }));
    }
    for round in 0..3 {
        for key in 0..=127u8 {
            hostile.push((10 + round * 200 + usize::from(key), NoteEvent::On { key, velocity: 1.0 }));
        }
    }
    let hostile = released(hostile);
    // Values out of range may pass on as they came (whatever gets them accepts any value);
    // the order of the events and the ends of the notes are what count. With room for no more
    // than it gets, a device leaves out note-ons before note-offs, so no note hangs either.
    for (what, room) in [("hostile events", TWICE), ("hostile events with room for as many", ONCE)] {
        let out = run_events(make().as_mut(), &hostile, &[512], FRAMES, room);
        for fault in event_faults(&out, FRAMES).into_iter().filter(|fault| !fault.contains("out of range")) {
            failures.push(format!("{what}: {fault}"));
        }
    }
    for fault in event_faults(&run_events(make().as_mut(), &events, &[512], FRAMES, ONCE), FRAMES) {
        failures.push(format!("the test events with room for as many: {fault}"));
    }

    // All-notes-off ends every note the device started, and the keys play again after it.
    let mut all_off: Vec<(usize, NoteEvent)> =
        [0u8, 40, 61, 64, 127].iter().map(|&key| (100, NoteEvent::On { key, velocity: 0.8 })).collect();
    all_off.push((2_000, NoteEvent::AllNotesOff));
    all_off.extend([0u8, 61, 127].iter().map(|&key| (3_000, NoteEvent::On { key, velocity: 0.8 })));
    let all_off = released(all_off);
    let out = run_events(make().as_mut(), &all_off, &[512], FRAMES, TWICE);
    for fault in event_faults(&out, FRAMES) {
        failures.push(format!("all-notes-off: {fault}"));
    }
    if !out.iter().any(|&(at, event)| at >= 3_000 && matches!(event, NoteEvent::On { .. })) {
        failures.push("after all-notes-off no note plays again".into());
    }
    let mut device = make();
    for (input, room) in [(&hostile[..], 0), (&hostile[..], 1), (&events[..], 3)] {
        let mut output = vec![(0, NoteEvent::AllNotesOff); room];
        if device.process_events(&ProcessContext::default(), FRAMES, input, &mut output) > room {
            failures.push(format!("{room} events of room reported more written"));
        }
    }
    let mut output = [(0, NoteEvent::AllNotesOff); 4];
    if device.process_events(&ProcessContext::default(), 0, &[], &mut output) != 0 {
        failures.push("no events in sends events out".into());
    }
}

#[cfg(test)]
#[path = "conformance_tests.rs"]
mod tests;
