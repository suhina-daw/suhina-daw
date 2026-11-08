// Copyright (C) 2026 The Suhina contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! The MIDI tools of the clip editor: `note.setMuted`, `note.humanize`, `note.fitToScale`,
//! `note.invert`, `note.reverse`, `note.legato`, `note.stretch`, `note.randomizeVelocity` and
//! `note.rampVelocity` (`docs/spec/ui-clip-editor.md` › Muting, › Note tools (the clip
//! panel's tools)).
//!
//! Every one of them edits through [`crate::notes::edit_notes`], so a clip that shares its content
//! with a duplicate gets a copy of it first (copy on write, ADR 0001) and the whole call is one
//! undo step. A call that changes nothing marks no undo step at all.

use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{SystemTime, UNIX_EPOCH};

use serde_json::{Value, json};
use suhina_model::{Note, Notes, Scale, ScaleMode};

use crate::commands::Args;
use crate::notes::{
    MAX_BEAT, MAX_NOTES_PER_COMMAND, after_removal, covered_go, edit_notes, indices, midi_velocity, notes_of,
    replace_under, replaces, velocity_of, with_replaced,
};
use crate::{CommandError, Session};

/// The default of `note.humanize`'s `ms`: how far a start may move, in milliseconds.
const DEFAULT_HUMANIZE_MS: f64 = 10.0;
/// The most `note.humanize` may move a start by, in milliseconds (`ms`) and in beats (`time`).
const MAX_HUMANIZE_MS: f64 = 50.0;
const MAX_HUMANIZE_BEATS: f64 = 1.0;
/// The default of `note.humanize`'s `velocity`, in MIDI steps.
const DEFAULT_HUMANIZE_STEPS: f64 = 8.0;
/// The default of `note.randomizeVelocity`'s `amount`, in MIDI steps.
const DEFAULT_RANDOMIZE_STEPS: f64 = 16.0;
/// The smallest and largest `note.stretch` factor.
const MIN_FACTOR: f64 = 0.01;
const MAX_FACTOR: f64 = 100.0;
/// The shortest length `note.legato` leaves a note: a hundredth of a beat.
const MIN_LENGTH: f64 = 0.01;
/// Starts this close are one start.
const EPSILON: f64 = 1e-9;

/// Suhina's own random number generator for the note tools: splitmix64, written out here so the
/// same seed gives the same draws on every machine and every run, which is what lets the CLI and
/// MCP repeat a call.
struct Rng {
    state: u64,
}

impl Rng {
    fn new(seed: u64) -> Self {
        Self { state: seed }
    }

    fn next_u64(&mut self) -> u64 {
        self.state = self.state.wrapping_add(0x9E37_79B9_7F4A_7C15);
        let mut z = self.state;
        z = (z ^ (z >> 30)).wrapping_mul(0xBF58_476D_1CE4_E5B9);
        z = (z ^ (z >> 27)).wrapping_mul(0x94D0_49BB_1331_11EB);
        z ^ (z >> 31)
    }

    /// A number in `[0, 1)`: 53 bits, the width of an `f64`'s mantissa, so every draw is a
    /// different value.
    fn next_f64(&mut self) -> f64 {
        (self.next_u64() >> 11) as f64 * (1.0 / (1u64 << 53) as f64)
    }

    /// A number in `[-1, 1)`.
    fn next_signed(&mut self) -> f64 {
        self.next_f64() * 2.0 - 1.0
    }

    /// A normally distributed draw within ±`limit`: most draws small, few large, none past the
    /// limit (a standard deviation of half the limit, the tails cut off), as a person's timing
    /// varies (#1214). Box–Muller over two uniform draws.
    fn next_within(&mut self, limit: f64) -> f64 {
        if limit <= 0.0 {
            // Still two draws, so a limit of 0 keeps the rest of the sequence where it was.
            self.next_f64();
            self.next_f64();
            return 0.0;
        }
        let u1 = 1.0 - self.next_f64();
        let u2 = self.next_f64();
        let gauss = (-2.0 * u1.ln()).sqrt() * (std::f64::consts::TAU * u2).cos();
        (gauss * limit / 2.0).clamp(-limit, limit)
    }
}

/// A seed for a call that names none: the clock, mixed with a counter so two calls in the same
/// nanosecond still differ, and never 0 (0 is a seed a caller may give).
fn fresh_seed() -> u64 {
    static CALLS: AtomicU64 = AtomicU64::new(0);
    let calls = CALLS.fetch_add(1, Ordering::Relaxed);
    let clock = match SystemTime::now().duration_since(UNIX_EPOCH) {
        Ok(since) => since.as_nanos() as u64,
        // A clock before 1970 is no reason to fail: the counter alone still differs per call.
        Err(_) => 0,
    };
    let seed = clock ^ calls.wrapping_mul(0x9E37_79B9_7F4A_7C15);
    if seed == 0 { 1 } else { seed }
}

/// The seed the call uses: `params.seed` when given, a fresh one otherwise.
fn seed_of(params: &Value) -> Result<u64, CommandError> {
    match Args::new(params).value("seed") {
        None => Ok(fresh_seed()),
        Some(value) => value.as_u64().ok_or_else(|| "seed must be a whole number, 0 or more".into()),
    }
}

/// Whether the command acts on every note of the clip (`notes` left out).
fn whole_clip(params: &Value) -> bool {
    !Args::new(params).has("notes")
}

/// The notes the command acts on: the named ones, or every note of the clip, in rising order.
fn chosen(params: &Value, notes: &Notes) -> Result<Vec<usize>, CommandError> {
    if !whole_clip(params) {
        return indices(params, notes);
    }
    let count = notes.len();
    if count > MAX_NOTES_PER_COMMAND {
        return Err(format!("at most {MAX_NOTES_PER_COMMAND} notes in one command, got {count}").into());
    }
    Ok((0..count).collect())
}

/// Whether the command names every note of a clip that has none: there is nothing to change, and
/// an empty MIDI clip gets no note lane and no undo step from it.
fn nothing_to_do(session: &Session, params: &Value) -> Result<bool, CommandError> {
    Ok(whole_clip(params) && notes_of(&session.project, params)?.is_empty())
}

/// Sets a note's velocity from a MIDI value, 1 … 127 (fractional values are kept). Leaves the
/// note alone when the value is the one it already has: reading a velocity as MIDI and writing it
/// back is not always bit for bit the same, and a call that changes nothing must not touch the
/// project, or its undo history.
fn apply_velocity(note: &mut Note, midi: f64) -> bool {
    let midi = midi.clamp(1.0, 127.0);
    if (midi - f64::from(midi_velocity(note.velocity))).abs() < 1e-9 {
        return false;
    }
    note.velocity = velocity_of(midi);
    true
}

/// A MIDI velocity `params` names: a whole number from 1 to 127.
fn velocity_param(params: &Value, key: &str) -> Result<f64, CommandError> {
    params
        .get(key)
        .and_then(Value::as_u64)
        .filter(|velocity| (1..=127).contains(velocity))
        .map(|velocity| velocity as f64)
        .ok_or_else(|| format!("{key} must be a whole number from 1 to 127").into())
}

/// The key root `params.root` names: a key name such as "C", "F#" or "Bb", or 0 to 11; C when
/// it is missing.
pub(crate) fn root_of(params: &Value) -> Result<u8, CommandError> {
    let unknown = || CommandError::from("root must be a key name such as C, F# or Bb, or a whole number 0 to 11");
    match Args::new(params).value("root") {
        None => Ok(0),
        Some(Value::String(name)) => {
            let mut chars = name.chars();
            let Some(letter) = chars.next() else { return Err(unknown()) };
            let base: i64 = match letter.to_ascii_uppercase() {
                'C' => 0,
                'D' => 2,
                'E' => 4,
                'F' => 5,
                'G' => 7,
                'A' => 9,
                'B' => 11,
                _ => return Err(format!("unknown root {name:?}; expected a key name such as C, F# or Bb").into()),
            };
            let mut semitone = base;
            for accidental in chars {
                match accidental {
                    '#' => semitone += 1,
                    'b' => semitone -= 1,
                    _ => {
                        return Err(format!("unknown root {name:?}; expected a key name such as C, F# or Bb").into());
                    }
                }
            }
            Ok(semitone.rem_euclid(12) as u8)
        }
        Some(value) => match value.as_u64().filter(|root| *root <= 11) {
            Some(root) => Ok(root as u8),
            None => Err(format!("root must be a key name such as C, F# or Bb, or 0 to 11; got {value}").into()),
        },
    }
}

/// The mode `params[key]` names, one of the model's ([`ScaleMode::name`]); `None` when missing.
pub(crate) fn mode_of(params: &Value, key: &str) -> Result<Option<ScaleMode>, CommandError> {
    let name = match Args::new(params).value(key) {
        None => return Ok(None),
        Some(Value::String(name)) => name.as_str(),
        Some(_) => return Err(format!("{key} must be a text such as major or blues").into()),
    };
    ScaleMode::from_name(name).map(Some).ok_or_else(|| {
        let names: Vec<&str> = ScaleMode::ALL.iter().map(|mode| mode.name()).collect();
        format!("unknown {key} {name:?}; expected one of {}", names.join(", ")).into()
    })
}

/// The scale `note.fitToScale` fits to: `root` and `scale` when either is given (C and major for
/// the one left out), else the clip's own scale, else C major.
fn fit_scale(session: &Session, params: &Value) -> Result<Scale, CommandError> {
    let named = Args::new(params).has("root") || Args::new(params).has("scale");
    let own = if named { None } else { crate::scales::scale_of_clip(&session.project, params)? };
    let scale = match own {
        Some(scale) => Some(scale),
        None => Scale::new(root_of(params)?, mode_of(params, "scale")?.unwrap_or_default()),
    };
    scale.ok_or_else(|| "root must be 0 to 11".into())
}

/// `note.setMuted`: mutes the named notes (`muted: true`), unmutes them (`false`), or flips each
/// one when `muted` is left out. The result names every note and the state it now has.
pub(crate) fn set_muted(session: &mut Session, params: &Value) -> Result<Value, CommandError> {
    let muted = match Args::new(params).value("muted") {
        None => None,
        Some(value) => Some(value.as_bool().ok_or("muted must be true or false")?),
    };
    edit_notes(&mut session.project, params, |notes| {
        let named = indices(params, notes)?;
        let mut states = Vec::with_capacity(named.len());
        for &index in &named {
            let note = notes.get_mut(index).ok_or("a note is missing")?;
            note.muted = muted.unwrap_or(!note.muted);
            states.push(note.muted);
        }
        Ok(json!({ "notes": named, "muted": states }))
    })
}

/// How far `note.humanize` may move a start, in beats: `time` in beats, or `ms` in milliseconds
/// at the project's tempo (10 ms when neither is given), never both.
fn humanize_beats(session: &Session, params: &Value) -> Result<f64, CommandError> {
    let args = Args::new(params);
    let time = args.opt_f64("time")?;
    let ms = args.opt_f64("ms")?;
    match (time, ms) {
        (Some(_), Some(_)) => Err("give time (beats) or ms (milliseconds), not both".into()),
        (Some(beats), None) => {
            if !(0.0..=MAX_HUMANIZE_BEATS).contains(&beats) {
                return Err(format!("time must be 0 to {MAX_HUMANIZE_BEATS} beats, got {beats}").into());
            }
            Ok(beats)
        }
        (None, ms) => {
            let ms = ms.unwrap_or(DEFAULT_HUMANIZE_MS);
            if !(0.0..=MAX_HUMANIZE_MS).contains(&ms) {
                return Err(format!("ms must be 0 to {MAX_HUMANIZE_MS} milliseconds, got {ms}").into());
            }
            let bpm = session.project.transport.tempo_bpm;
            let bpm = if bpm.is_finite() && bpm > 0.0 { bpm } else { 120.0 };
            Ok(ms / 1000.0 * bpm / 60.0)
        }
    }
}

/// How close after a chord's first note another note starts and still belongs to the chord, for
/// Humanise's `chords`: a played chord, or one humanised before, is never exactly together.
const CHORD_MS: f64 = 30.0;

/// `note.humanize`: moves each note's start by a normally distributed draw within ±`time` beats
/// (or ±`ms` milliseconds) and its velocity by one within ±`velocity` MIDI steps, each kept within
/// its range (#1214). With `chords` (the default) the notes of a chord, those that start within
/// [`CHORD_MS`] of its first, move together by one draw, which keeps the chord's first note at 0 or
/// later, so the notes keep their places in the chord. A
/// note's end stays where it was, unless that would run into the next note on the same key. The
/// same `seed` gives the same result, so a call can be repeated.
pub(crate) fn humanize(session: &mut Session, params: &Value) -> Result<Value, CommandError> {
    let beats = humanize_beats(session, params)?;
    let steps = Args::new(params).opt_f64("velocity")?.unwrap_or(DEFAULT_HUMANIZE_STEPS);
    if !(0.0..=127.0).contains(&steps) {
        return Err(format!("velocity must be 0 to 127 MIDI steps, got {steps}").into());
    }
    let chords = match Args::new(params).value("chords") {
        None => true,
        Some(value) => value.as_bool().ok_or("chords must be true or false")?,
    };
    let seed = seed_of(params)?;
    if nothing_to_do(session, params)? {
        return Ok(json!({ "notes": [], "seed": seed }));
    }
    let bpm = session.project.transport.tempo_bpm;
    let bpm = if bpm.is_finite() && bpm > 0.0 { bpm } else { 120.0 };
    let window = if chords { CHORD_MS / 1000.0 * bpm / 60.0 } else { 0.0 };
    edit_notes(&mut session.project, params, move |notes| {
        let named = chosen(params, notes)?;
        let mut rng = Rng::new(seed);
        let chord_of = chords_of(notes, &named, window);
        // Each chord's offset once drawn, so its other notes move with the first.
        let mut offsets: Vec<Option<f64>> = vec![None; chord_of.len()];
        let mut moved = Vec::with_capacity(named.len());
        for (position, &index) in named.iter().enumerate() {
            let note = notes.get(index).ok_or("a note is missing")?;
            let mut humanized = *note;
            let drawn = rng.next_within(beats);
            let (chord, first) = chord_of.get(position).copied().unwrap_or((position, note.time));
            let offset = match offsets.get(chord).copied().flatten() {
                Some(offset) => offset,
                None => {
                    // The chord's first note goes no earlier than 0, so the chord stays whole.
                    let offset = drawn.max(-first);
                    if let Some(slot) = offsets.get_mut(chord) {
                        *slot = Some(offset);
                    }
                    offset
                }
            };
            let time = (note.time + offset).clamp(0.0, MAX_BEAT);
            if time != note.time {
                // The end stays where it was: the note gets longer or shorter by what it moved.
                let end = note.time + note.duration;
                humanized.time = time;
                humanized.duration = (end - time).max(MIN_LENGTH);
            }
            let midi = f64::from(midi_velocity(note.velocity)) + rng.next_within(steps);
            apply_velocity(&mut humanized, midi);
            moved.push((index, humanized));
        }
        let touched: Vec<usize> = moved.iter().filter(|(i, n)| notes.get(*i) != Some(n)).map(|(i, _)| *i).collect();
        for (index, note) in moved {
            if let Some(slot) = notes.get_mut(index) {
                *slot = note;
            }
        }
        keep_off_the_next(notes, &touched);
        Ok(json!({ "notes": named, "seed": seed }))
    })
}

/// For each of `named` (by its place in it): the chord it belongs to and that chord's first start.
/// Notes are taken in time; one that starts within `window` beats of a chord's first note joins
/// it, any other starts a chord of its own. With a `window` of 0 every note is its own chord, but
/// notes that start at the very same beat still move together.
fn chords_of(notes: &Notes, named: &[usize], window: f64) -> Vec<(usize, f64)> {
    let time = |position: usize| named.get(position).and_then(|i| notes.get(*i)).map_or(0.0, |n| n.time);
    let mut order: Vec<usize> = (0..named.len()).collect();
    order.sort_by(|a, b| time(*a).total_cmp(&time(*b)));
    let mut chord_of = vec![(0, 0.0); named.len()];
    let mut current: Option<(usize, f64)> = None;
    for position in order {
        let start = time(position);
        let chord = match current {
            Some((chord, first)) if start - first <= window + EPSILON => (chord, first),
            _ => (position, start),
        };
        current = Some(chord);
        if let Some(slot) = chord_of.get_mut(position) {
            *slot = chord;
        }
    }
    chord_of
}

/// Shortens a note that now runs into the next note on its key, where either of the two was moved
/// (`touched`): its end goes to the next one's start, a hundredth of a beat long at least.
fn keep_off_the_next(notes: &mut Notes, touched: &[usize]) {
    if touched.is_empty() {
        return;
    }
    let mut order: Vec<usize> = (0..notes.len()).collect();
    order.sort_by(|a, b| {
        let (Some(x), Some(y)) = (notes.get(*a), notes.get(*b)) else { return std::cmp::Ordering::Equal };
        x.key.cmp(&y.key).then(x.time.total_cmp(&y.time))
    });
    for pair in order.windows(2) {
        let &[a, b] = pair else { continue };
        if !(touched.contains(&a) || touched.contains(&b)) {
            continue;
        }
        let (Some(first), Some(next)) = (notes.get(a).copied(), notes.get(b).copied()) else { continue };
        if first.key != next.key || first.time + first.duration <= next.time + EPSILON {
            continue;
        }
        if let Some(slot) = notes.get_mut(a) {
            slot.duration = (next.time - first.time).max(MIN_LENGTH);
        }
    }
}

/// `note.fitToScale`: moves each note to the nearest key of a scale over a root (the clip's own
/// scale when neither is given); a tie goes to the lower key.
pub(crate) fn fit_to_scale(session: &mut Session, params: &Value) -> Result<Value, CommandError> {
    let scale = fit_scale(session, params)?;
    if nothing_to_do(session, params)? {
        return Ok(json!({ "notes": [], "moved": 0 }));
    }
    edit_notes(&mut session.project, params, |notes| {
        let named = chosen(params, notes)?;
        let mut moved = 0;
        for &index in &named {
            let note = notes.get(index).ok_or("a note is missing")?;
            let key = scale.nearest(note.key);
            if key != note.key {
                if let Some(slot) = notes.get_mut(index) {
                    slot.key = key;
                }
                moved += 1;
            }
        }
        let replaced =
            if replaces(params) && moved > 0 { replace_under(notes, &named, covered_go(params)) } else { Vec::new() };
        Ok(json!({ "notes": after_removal(&named, &replaced), "moved": moved, "replaced": replaced }))
    })
}

/// `note.invert`: mirrors the keys around the middle of the range the acted notes cover (the
/// lowest key plus the highest, halved, rounded down), each kept within 0 to 127.
pub(crate) fn invert(session: &mut Session, params: &Value) -> Result<Value, CommandError> {
    if nothing_to_do(session, params)? {
        return Ok(json!({ "notes": [] }));
    }
    edit_notes(&mut session.project, params, |notes| {
        let named = chosen(params, notes)?;
        let (mut lowest, mut highest) = (u8::MAX, 0u8);
        for &index in &named {
            let note = notes.get(index).ok_or("a note is missing")?;
            lowest = lowest.min(note.key);
            highest = highest.max(note.key);
        }
        // Mirrored around the middle of the range: the lowest and highest keys swap, every key
        // stays inside the range, and inverting twice gives the notes back.
        let sum = i64::from(lowest) + i64::from(highest);
        for &index in &named {
            let note = notes.get(index).ok_or("a note is missing")?;
            let key = u8::try_from(sum - i64::from(note.key)).map_err(|_| "a key is out of range")?;
            if key != note.key
                && let Some(slot) = notes.get_mut(index)
            {
                slot.key = key;
            }
        }
        let replaced = if replaces(params) { replace_under(notes, &named, covered_go(params)) } else { Vec::new() };
        Ok(json!({ "notes": after_removal(&named, &replaced), "replaced": replaced }))
    })
}

/// `note.reverse`: turns the acted notes round in time inside the span from the earliest start to
/// the latest end, each keeping its length.
pub(crate) fn reverse(session: &mut Session, params: &Value) -> Result<Value, CommandError> {
    if nothing_to_do(session, params)? {
        return Ok(json!({ "notes": [] }));
    }
    edit_notes(&mut session.project, params, |notes| {
        let named = chosen(params, notes)?;
        // Mirrored inside the span from the earliest start to the latest end: a note ending at
        // the span's end starts at its start, and the notes stay where they were in the clip.
        let (mut from, mut to) = (f64::INFINITY, f64::NEG_INFINITY);
        for &index in &named {
            let note = notes.get(index).ok_or("a note is missing")?;
            from = from.min(note.time);
            to = to.max(note.time + note.duration);
        }
        let mut times = Vec::with_capacity(named.len());
        for &index in &named {
            let note = notes.get(index).ok_or("a note is missing")?;
            times.push((index, (from + to - (note.time + note.duration)).clamp(0.0, MAX_BEAT)));
        }
        for (index, time) in times {
            if let Some(note) = notes.get_mut(index) {
                note.time = time;
            }
        }
        let replaced = if replaces(params) { replace_under(notes, &named, covered_go(params)) } else { Vec::new() };
        Ok(json!({ "notes": after_removal(&named, &replaced), "replaced": replaced }))
    })
}

/// `note.legato`: makes each named note last until the next named note starts, on any key, never
/// shorter than a hundredth of a beat. Notes that start together end together, and the last ones
/// keep their length.
pub(crate) fn legato(session: &mut Session, params: &Value) -> Result<Value, CommandError> {
    if nothing_to_do(session, params)? {
        return Ok(json!({ "notes": [], "changed": 0 }));
    }
    edit_notes(&mut session.project, params, |notes| {
        let named = chosen(params, notes)?;
        // Time, then index: the same note always takes the same place in the order.
        let mut order = Vec::with_capacity(named.len());
        for &index in &named {
            let note = notes.get(index).ok_or("a note is missing")?;
            order.push((note.time, index));
        }
        order.sort_by(|a, b| a.0.total_cmp(&b.0).then(a.1.cmp(&b.1)));
        let until = Args::new(params).opt_f64("until")?;
        let mut lengthened = Vec::new();
        for &(time, index) in &order {
            // The first start after this one; none for the last notes, which keep their length.
            let next = order.partition_point(|n| n.0 <= time + EPSILON);
            let next = order.get(next).map(|n| n.0).or(until.filter(|end| *end > time + EPSILON));
            let Some(next) = next else { continue };
            let length = (next - time).max(MIN_LENGTH);
            if !length.is_finite() || length > MAX_BEAT {
                return Err(format!("note {index} would be {length} beats long, after {MAX_BEAT}").into());
            }
            let note = notes.get_mut(index).ok_or("a note is missing")?;
            if length != note.duration {
                note.duration = length;
                lengthened.push(index);
            }
        }
        let changed = lengthened.len();
        let replaced = if replaces(params) && changed > 0 {
            replace_under(notes, &lengthened, covered_go(params))
        } else {
            Vec::new()
        };
        Ok(with_replaced(params, json!({ "notes": after_removal(&named, &replaced), "changed": changed }), &replaced))
    })
}

/// `note.stretch`: stretches the acted notes in time about the earliest of their starts, starts
/// and lengths alike; a result outside the model's range fails the whole command.
pub(crate) fn stretch(session: &mut Session, params: &Value) -> Result<Value, CommandError> {
    let factor = Args::new(params).opt_f64("factor")?.ok_or("give factor, how much to stretch by (0.01 to 100)")?;
    if !(MIN_FACTOR..=MAX_FACTOR).contains(&factor) {
        return Err(format!("factor must be {MIN_FACTOR} to {MAX_FACTOR}, got {factor}").into());
    }
    if nothing_to_do(session, params)? {
        return Ok(json!({ "notes": [] }));
    }
    edit_notes(&mut session.project, params, |notes| {
        let named = chosen(params, notes)?;
        // Multiplying by one is exact for nothing but zero: leave the notes as they are, so a
        // factor of 1 changes nothing and marks no undo step.
        if factor == 1.0 {
            return Ok(json!({ "notes": named }));
        }
        let base =
            named.iter().filter_map(|index| notes.get(*index)).map(|note| note.time).fold(f64::INFINITY, f64::min);
        let mut stretched = Vec::with_capacity(named.len());
        for &index in &named {
            let note = notes.get(index).ok_or("a note is missing")?;
            let time = base + (note.time - base) * factor;
            let length = note.duration * factor;
            if !(0.0..=MAX_BEAT).contains(&time) || !(length > 0.0 && length <= MAX_BEAT) {
                return Err(format!(
                    "note {index} would start at beat {time} and be {length} beats long; both must be within 0 to {MAX_BEAT}, and a length above 0"
                )
                .into());
            }
            stretched.push((index, time, length));
        }
        for (index, time, length) in stretched {
            if let Some(note) = notes.get_mut(index) {
                note.time = time;
                note.duration = length;
            }
        }
        let replaced = if replaces(params) { replace_under(notes, &named, covered_go(params)) } else { Vec::new() };
        Ok(json!({ "notes": after_removal(&named, &replaced), "replaced": replaced }))
    })
}

/// `note.randomizeVelocity`: moves each velocity by a random draw in ±`amount` MIDI steps, each
/// kept within 1 to 127. The same `seed` gives the same result.
pub(crate) fn randomize_velocity(session: &mut Session, params: &Value) -> Result<Value, CommandError> {
    let steps = Args::new(params).opt_f64("amount")?.unwrap_or(DEFAULT_RANDOMIZE_STEPS);
    if !(0.0..=127.0).contains(&steps) {
        return Err(format!("amount must be 0 to 127 MIDI steps, got {steps}").into());
    }
    let seed = seed_of(params)?;
    if nothing_to_do(session, params)? {
        return Ok(json!({ "notes": [], "seed": seed }));
    }
    edit_notes(&mut session.project, params, move |notes| {
        let named = chosen(params, notes)?;
        let mut rng = Rng::new(seed);
        for &index in &named {
            let note = notes.get_mut(index).ok_or("a note is missing")?;
            let midi = f64::from(midi_velocity(note.velocity)) + rng.next_signed() * steps;
            apply_velocity(note, midi);
        }
        Ok(json!({ "notes": named, "seed": seed }))
    })
}

/// `note.rampVelocity`: spreads the velocities of the acted notes evenly from `from` to `to` in
/// time order (ties by key); one note takes `to`.
pub(crate) fn ramp_velocity(session: &mut Session, params: &Value) -> Result<Value, CommandError> {
    let from = velocity_param(params, "from")?;
    let to = velocity_param(params, "to")?;
    if nothing_to_do(session, params)? {
        return Ok(json!({ "notes": [], "count": 0 }));
    }
    edit_notes(&mut session.project, params, |notes| {
        let named = chosen(params, notes)?;
        let mut order = Vec::with_capacity(named.len());
        for &index in &named {
            let note = notes.get(index).ok_or("a note is missing")?;
            order.push((note.time, note.key, index));
        }
        order.sort_by(|a, b| a.0.total_cmp(&b.0).then(a.1.cmp(&b.1)));
        let count = order.len();
        for (position, (_, _, index)) in order.iter().enumerate() {
            let midi = if count <= 1 { to } else { from + (to - from) * position as f64 / (count - 1) as f64 };
            let note = notes.get_mut(*index).ok_or("a note is missing")?;
            apply_velocity(note, midi.round());
        }
        Ok(json!({ "notes": named, "count": count }))
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_generator_is_stable_for_a_seed() {
        // The first values of splitmix64 for seed 42, so a change to the generator is visible.
        let mut rng = Rng::new(42);
        assert_eq!(rng.next_u64(), 13_679_457_532_755_275_413);
        assert_eq!(rng.next_u64(), 2_949_826_092_126_892_291);
        assert_eq!(rng.next_u64(), 5_139_283_748_462_763_858);

        let mut rng = Rng::new(42);
        assert!((rng.next_f64() - 0.741_564_878_771_823_3).abs() < 1e-15);
        assert!((rng.next_f64() - 0.159_910_392_876_920_1).abs() < 1e-15);
    }

    #[test]
    fn the_same_seed_gives_the_same_draws_and_a_different_one_does_not() {
        let draws = |seed| {
            let mut rng = Rng::new(seed);
            (0..8).map(|_| rng.next_signed()).collect::<Vec<f64>>()
        };
        assert_eq!(draws(7), draws(7));
        assert_ne!(draws(7), draws(8));
        assert!(draws(0).iter().all(|draw| (-1.0..1.0).contains(draw)));
        assert_eq!(Rng::new(0).next_u64(), 0xE220_A839_7B1D_CDAF);
    }

    #[test]
    fn a_fresh_seed_is_never_zero_and_differs_per_call() {
        let first = fresh_seed();
        let second = fresh_seed();
        assert_ne!(first, 0);
        assert_ne!(second, 0);
        assert_ne!(first, second);
    }

    #[test]
    fn a_root_is_a_key_name_or_a_number() {
        let root = |value: Value| root_of(&json!({ "root": value }));
        assert_eq!(root(json!("C")), Ok(0));
        assert_eq!(root(json!("c")), Ok(0));
        assert_eq!(root(json!("F#")), Ok(6));
        assert_eq!(root(json!("Bb")), Ok(10));
        assert_eq!(root(json!("Cb")), Ok(11));
        assert_eq!(root(json!("B#")), Ok(0));
        assert_eq!(root(json!(11)), Ok(11));
        assert!(root(json!("H")).is_err());
        assert!(root(json!(12)).is_err());
        assert!(root(json!(-1)).is_err());
        assert!(root(json!("")).is_err());
    }

    #[test]
    fn a_mode_is_one_of_the_twelve() {
        assert_eq!(mode_of(&json!({}), "scale"), Ok(None));
        assert_eq!(mode_of(&json!({ "scale": "blues" }), "scale"), Ok(Some(ScaleMode::Blues)));
        assert_eq!(mode_of(&json!({ "mode": "harmonicMinor" }), "mode"), Ok(Some(ScaleMode::HarmonicMinor)));
        assert!(mode_of(&json!({ "scale": "majorish" }), "scale").is_err());
        assert!(mode_of(&json!({ "scale": 5 }), "scale").is_err());
    }

    #[test]
    fn the_nearest_key_of_a_scale_takes_the_lower_on_a_tie() {
        let c = Scale::new(0, ScaleMode::Major).unwrap();
        assert_eq!(c.nearest(60), 60);
        // 61 sits between 60 and 62: the lower wins.
        assert_eq!(c.nearest(61), 60);
        assert_eq!(c.nearest(63), 62);
        // Keys stay within 0 to 127.
        assert_eq!(c.nearest(127), 127);
        assert_eq!(c.nearest(0), 0);
        // A root of G: 65 (F) is not in G major, and sits between 64 (E) and 66 (F#), so the
        // lower wins.
        let g = Scale::new(7, ScaleMode::Major).unwrap();
        assert_eq!(g.nearest(65), 64);
        assert_eq!(g.nearest(66), 66);
    }
}
