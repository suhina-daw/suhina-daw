// Copyright (C) 2026 The Suhina contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! The randomizer: new values for a voice's parameters from a seed, section by section. A value
//! moves from where it is, by at most its section's share of its range, and a few rules keep the
//! result audible (`docs/spec/fm-synth.md` › Randomizer).

use suhina_device_api::{ParamKind, Scale};

use crate::core;
use crate::params::{self, ENV_FIELDS, GLOBAL, HARMONICS, OP_EXT_FIELDS, OP_PARAMS, PARAMS, SECTIONS, env, opx};

/// Sections in [`SECTIONS`] order.
const ENVELOPES: usize = 0;
const LEVELS: usize = 1;
const WAVES: usize = 2;
const FILTER: usize = 3;
const LFO: usize = 4;
const PITCH_ENVELOPE: usize = 5;
const MODULATION: usize = 6;
const STEREO: usize = 7;
const RATIOS: usize = 8;
const ALGORITHM: usize = 9;

/// The level a heard operator is lifted to when nothing else would sound.
const AUDIBLE_LEVEL: f64 = 80.0;
/// The peak in decibels of that operator's time envelope.
const AUDIBLE_PEAK_DB: f64 = -6.0;
/// The lowest and highest a randomized filter goes, so it does not close the sound off.
const FILTER_RANGE: (f64, f64) = (300.0, 12_000.0);
/// An operator's fields in the rates and levels model, the first eight of its parameters.
const RATE_FIELDS: usize = 8;
const RATE_1: usize = 0;
const RATE_4: usize = 3;
/// The slowest a randomized heard operator starts and ends, so a note speaks and stops: DX7 rates
/// (higher is faster) and seconds.
const SLOWEST_RATE_1: f64 = 30.0;
const SLOWEST_RATE_4: f64 = 40.0;
const LONGEST_ATTACK: f64 = 2.0;
const LONGEST_RELEASE: f64 = 5.0;

fn section_of_env(field: usize) -> bool {
    !matches!(field, env::LOOP | env::REPEAT | env::LOOP_TIME)
}

/// The section parameter `index` belongs to, if the randomizer touches it. Level 4 is left out, as
/// a heard operator rests at it after its release, and so is the device's Time, which would slow
/// every envelope at once.
fn section(index: usize) -> Option<usize> {
    if index < GLOBAL {
        return match index % OP_PARAMS {
            0..=6 | 8..=15 => Some(ENVELOPES),
            params::op::LEVEL => Some(LEVELS),
            18 | params::op::DETUNE => Some(RATIOS),
            _ => None,
        };
    }
    if index < params::PERFORMANCE {
        return match index - GLOBAL {
            0..=7 => Some(PITCH_ENVELOPE),
            8 => Some(ALGORITHM),
            9 => Some(WAVES),
            11..=14 | 16 | 17 => Some(LFO),
            _ => None,
        };
    }
    match index {
        params::PAN | params::PAN_BY_KEY | params::PAN_RANDOM | params::SPREAD => Some(STEREO),
        params::TONE => Some(FILTER),
        i if (params::FILTER..params::FILTER_ENV).contains(&i) => {
            (i - params::FILTER != params::filter::ON).then_some(FILTER)
        }
        i if (params::FILTER_ENV..params::LFO).contains(&i) => section_of_env(i - params::FILTER_ENV).then_some(FILTER),
        i if (params::LFO..params::LFO_ENV).contains(&i) => {
            let field = i - params::LFO;
            matches!(
                field,
                params::lfo::WAVE | params::lfo::RATE | params::lfo::BY_KEY | params::lfo::BY_VEL | params::lfo::AMOUNT
            )
            .then_some(LFO)
        }
        i if (params::LFO_ENV..params::PITCH).contains(&i) => section_of_env(i - params::LFO_ENV).then_some(LFO),
        i if (params::PITCH..params::PITCH_ENV).contains(&i) => {
            (i - params::PITCH == params::pitch::AMOUNT).then_some(PITCH_ENVELOPE)
        }
        i if (params::PITCH_ENV..params::MATRIX).contains(&i) => {
            section_of_env(i - params::PITCH_ENV).then_some(PITCH_ENVELOPE)
        }
        i if (params::MATRIX..params::OP_EXT).contains(&i) => Some(MODULATION),
        i if (params::OP_EXT..params::DRAWN).contains(&i) => {
            let field = (i - params::OP_EXT) % OP_EXT_FIELDS;
            match field {
                opx::WAVE | opx::FEEDBACK | opx::PHASE => Some(WAVES),
                opx::LEVEL_BY_KEY => Some(LEVELS),
                opx::FREQ_BY_VEL => Some(MODULATION),
                f if (opx::ENV..opx::ENV + ENV_FIELDS).contains(&f) => {
                    section_of_env(f - opx::ENV).then_some(ENVELOPES)
                }
                _ => None,
            }
        }
        i if (params::DRAWN..params::PANEL).contains(&i) => Some(WAVES),
        _ => None,
    }
}

struct Random(u32);

impl Random {
    fn new(seed: u32) -> Self {
        // Xorshift cannot start from zero; the mix spreads near seeds apart.
        let mut random = Self(seed.wrapping_mul(0x9e37_79b9) | 1);
        for _ in 0..4 {
            random.next();
        }
        random
    }

    fn next(&mut self) -> u32 {
        let mut x = self.0;
        x ^= x << 13;
        x ^= x >> 17;
        x ^= x << 5;
        self.0 = x;
        x
    }

    /// From 0 to 1.
    fn unit(&mut self) -> f64 {
        f64::from(self.next()) / f64::from(u32::MAX)
    }
}

/// `value` moved by up to `share` of the parameter's range.
fn moved(index: usize, value: f64, share: f64, random: &mut Random) -> f64 {
    let Some(info) = PARAMS.get(index) else { return value };
    let (min, max) = (info.min, info.max);
    match info.kind {
        ParamKind::Toggle | ParamKind::Choice(_) => {
            if random.unit() < share {
                min + (random.unit() * (max - min + 1.0)).floor().min(max - min)
            } else {
                value
            }
        }
        ParamKind::Integer => {
            let step = (random.unit() * 2.0 - 1.0) * share * (max - min);
            (value + step).round().clamp(min, max)
        }
        ParamKind::Continuous(Scale::Logarithmic) if min > 0.0 => {
            let span = (max / min).ln();
            let position = (value.max(min) / min).ln() + (random.unit() * 2.0 - 1.0) * share * span;
            (min * position.clamp(0.0, span).exp()).clamp(min, max)
        }
        ParamKind::Continuous(_) => (value + (random.unit() * 2.0 - 1.0) * share * (max - min)).clamp(min, max),
    }
}

/// Whether operator `operator` (0 is operator 1) uses its time envelope.
fn uses_time(values: &[f64], operator: usize) -> bool {
    values.get(params::OP_EXT + operator * OP_EXT_FIELDS + opx::ENV_MODE).is_some_and(|v| *v >= 0.5)
}

/// Whether `index` is the envelope model its operator does not use, which nothing would hear.
fn unused_envelope(values: &[f64], index: usize) -> bool {
    if index < GLOBAL {
        return index % OP_PARAMS < RATE_FIELDS && uses_time(values, index / OP_PARAMS);
    }
    if (params::OP_EXT..params::DRAWN).contains(&index) {
        let (operator, field) = ((index - params::OP_EXT) / OP_EXT_FIELDS, (index - params::OP_EXT) % OP_EXT_FIELDS);
        return field >= opx::ENV && !uses_time(values, operator);
    }
    false
}

/// New values for what differs from `values` (every parameter in order, as a whole voice). `seed`
/// picks the result; `amount` scales the sections' percentages over the stored Amount. Sections
/// and their locks are read from `values`. The seed moves on, so the next call without one differs.
pub fn randomize(values: &[f64], seed: Option<u32>, amount: Option<f64>) -> Vec<(usize, f64)> {
    let value = |index: usize| values.get(index).copied().unwrap_or(0.0);
    let seed_now = seed.unwrap_or(value(params::RANDOM_SEED) as u32);
    let mut random = Random::new(seed_now);
    let amount = amount.unwrap_or_else(|| value(params::RANDOM_AMOUNT)).clamp(0.0, 1.0);
    let shares: [Option<f64>; SECTIONS.len()] = std::array::from_fn(|s| {
        let at = params::RANDOM_SECTIONS + s * 2;
        (value(at + 1) >= 0.5).then_some(value(at))
    });

    let mut out: Vec<f64> = values.to_vec();
    for index in 0..params::PANEL {
        let Some(s) = section(index) else { continue };
        let Some(Some(share)) = shares.get(s).copied() else { continue };
        if s == ENVELOPES && unused_envelope(values, index) {
            continue;
        }
        // The partials of an operator count only while it plays its drawn wave.
        if (params::DRAWN..params::PANEL).contains(&index) {
            let operator = (index - params::DRAWN) / HARMONICS;
            let wave = value(params::OP_EXT + operator * OP_EXT_FIELDS + opx::WAVE);
            if wave as usize != crate::wave::DRAWN {
                continue;
            }
        }
        // Algorithm is all or nothing, as its neighbours are not alike.
        let share = if s == ALGORITHM { share } else { share * amount };
        if let Some(slot) = out.get_mut(index) {
            *slot = moved(index, value(index), share, &mut random);
        }
    }
    keep_audible(&mut out);
    if shares.get(ENVELOPES).copied().flatten().is_some() {
        keep_notes_ending(&mut out);
    }

    if seed.is_none()
        && let Some(slot) = out.get_mut(params::RANDOM_SEED)
    {
        *slot = f64::from((seed_now + 1) % 100_000);
    }
    out.iter().enumerate().filter(|(i, v)| values.get(*i).copied() != Some(**v)).map(|(i, v)| (i, *v)).collect()
}

/// Every operator heard directly starts soon and falls silent soon after its key goes up.
fn keep_notes_ending(out: &mut [f64]) {
    let algorithm = out.get(GLOBAL + 8).map_or(0, |v| (*v as usize).saturating_sub(1));
    for operator in (0..6).filter(|operator| core::is_carrier(algorithm, 5 - operator)) {
        let time = uses_time(out, operator);
        let base = operator * OP_PARAMS;
        let envelope = params::OP_EXT + operator * OP_EXT_FIELDS + opx::ENV;
        let mut limit = |index: usize, change: &dyn Fn(f64) -> f64| {
            if let Some(slot) = out.get_mut(index) {
                *slot = change(*slot);
            }
        };
        if time {
            limit(envelope + env::ATTACK, &|v| v.min(LONGEST_ATTACK));
            limit(envelope + env::RELEASE, &|v| v.min(LONGEST_RELEASE));
        } else {
            limit(base + RATE_1, &|v| v.max(SLOWEST_RATE_1));
            limit(base + RATE_4, &|v| v.max(SLOWEST_RATE_4));
            limit(base + RATE_FIELDS - 1, &|_| 0.0);
        }
    }
}

/// At least one operator heard directly sounds, and the filter stays open enough.
fn keep_audible(out: &mut [f64]) {
    let algorithm = out.get(GLOBAL + 8).map_or(0, |v| (*v as usize).saturating_sub(1));
    let level_of = |operator: usize| operator * OP_PARAMS + params::op::LEVEL;
    let on_of = |operator: usize| operator * OP_PARAMS + params::op::ON;
    let sounding = (0..6).any(|operator| {
        core::is_carrier(algorithm, 5 - operator)
            && out.get(on_of(operator)).is_some_and(|v| *v >= 0.5)
            && out.get(level_of(operator)).is_some_and(|v| *v >= AUDIBLE_LEVEL / 2.0)
    });
    if !sounding && let Some(operator) = (0..6).find(|operator| core::is_carrier(algorithm, 5 - operator)) {
        if let Some(slot) = out.get_mut(level_of(operator)) {
            *slot = AUDIBLE_LEVEL;
        }
        if let Some(slot) = out.get_mut(on_of(operator)) {
            *slot = 1.0;
        }
        let peak = params::OP_EXT + operator * OP_EXT_FIELDS + opx::ENV + env::PEAK;
        if let Some(slot) = out.get_mut(peak) {
            *slot = slot.max(AUDIBLE_PEAK_DB);
        }
    }
    if let Some(slot) = out.get_mut(params::FILTER + params::filter::FREQ) {
        *slot = slot.clamp(FILTER_RANGE.0, 20_000.0);
    }
    if out.get(params::FILTER + params::filter::TYPE).is_some_and(|t| *t as usize == 1)
        && let Some(slot) = out.get_mut(params::FILTER + params::filter::FREQ)
    {
        *slot = slot.min(FILTER_RANGE.1);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn defaults() -> Vec<f64> {
        PARAMS.iter().map(|p| p.default).collect()
    }

    fn index(id: &str) -> usize {
        PARAMS.iter().position(|p| p.id == id).unwrap_or(usize::MAX)
    }

    #[test]
    fn a_seed_gives_the_same_result_and_another_seed_another() {
        let v = defaults();
        assert_eq!(randomize(&v, Some(7), None), randomize(&v, Some(7), None));
        assert_ne!(randomize(&v, Some(7), None), randomize(&v, Some(8), None));
    }

    #[test]
    fn a_value_moves_within_its_range_and_its_share() {
        let v = defaults();
        for (i, value) in randomize(&v, Some(3), Some(1.0)) {
            let info = &PARAMS[i];
            assert!(value >= info.min && value <= info.max, "{}: {value}", info.id);
        }
        // Half the range at most, for a continuous one at 50 %.
        let mut small = v.clone();
        let at = params::RANDOM_SECTIONS + STEREO * 2;
        small[at] = 0.5;
        let spread = index("spread");
        for seed in 0..50 {
            for (i, value) in randomize(&small, Some(seed), Some(1.0)) {
                if i == spread {
                    assert!(value <= 0.5, "{value}");
                }
            }
        }
    }

    #[test]
    fn ratios_and_algorithm_stay_unless_asked() {
        let v = defaults();
        for seed in 0..20 {
            let changes = randomize(&v, Some(seed), Some(1.0));
            assert!(!changes.iter().any(|(i, _)| *i == index("algorithm") || *i == index("op1.coarse")), "{seed}");
        }
        let mut asked = v.clone();
        asked[params::RANDOM_SECTIONS + ALGORITHM * 2 + 1] = 1.0;
        assert!((0..20).any(|seed| randomize(&asked, Some(seed), None).iter().any(|(i, _)| *i == index("algorithm"))));
    }

    #[test]
    fn a_section_turned_off_keeps_its_values() {
        let mut v = defaults();
        v[params::RANDOM_SECTIONS + FILTER * 2 + 1] = 0.0;
        let changes = randomize(&v, Some(11), Some(1.0));
        assert!(!changes.iter().any(|(i, _)| *i == index("filterRes") || *i == index("filterFreq")));
    }

    #[test]
    fn something_heard_directly_always_sounds() {
        let mut v = defaults();
        v[params::RANDOM_SECTIONS + ALGORITHM * 2 + 1] = 1.0;
        for op in 0..6 {
            v[op * OP_PARAMS + params::op::LEVEL] = 0.0;
        }
        for seed in 0..50 {
            let mut out = v.clone();
            for (i, value) in randomize(&v, Some(seed), Some(0.1)) {
                out[i] = value;
            }
            let algorithm = out[GLOBAL + 8] as usize - 1;
            assert!(
                (0..6).any(|op| core::is_carrier(algorithm, 5 - op)
                    && out[op * OP_PARAMS + params::op::LEVEL] >= AUDIBLE_LEVEL / 2.0
                    && out[op * OP_PARAMS + params::op::ON] >= 0.5),
                "seed {seed}"
            );
        }
    }

    #[test]
    fn a_note_still_ends_after_randomizing() {
        let mut v = defaults();
        v[index("op1.level4")] = 60.0;
        v[index("op1.rate4")] = 0.0;
        v[index("op2.level4")] = 60.0;
        for seed in 0..50 {
            let mut out = v.clone();
            for (i, value) in randomize(&v, Some(seed), Some(1.0)) {
                out[i] = value;
            }
            // Operator 1 is heard in the first algorithm, operator 2 is not.
            assert_eq!(out[index("op1.level4")], 0.0, "seed {seed}");
            assert!(out[index("op1.rate4")] >= SLOWEST_RATE_4, "seed {seed}");
            assert_eq!(out[index("op2.level4")], 60.0, "seed {seed}");
            assert_eq!(out[index("time")], 0.0, "seed {seed}");
        }
        v[index("op1.envMode")] = 1.0;
        v[index("op1.env.release")] = 60.0;
        let release = randomize(&v, Some(1), Some(1.0)).into_iter().find(|(i, _)| *i == index("op1.env.release"));
        assert!(release.is_some_and(|(_, value)| value <= LONGEST_RELEASE));
    }

    #[test]
    fn only_the_envelope_an_operator_uses_moves() {
        let mut v = defaults();
        v[index("op1.envMode")] = 1.0;
        let changes = randomize(&v, Some(5), Some(1.0));
        assert!(!changes.iter().any(|(i, _)| *i == index("op1.rate2") || *i == index("op2.env.decay")));
        assert!(changes.iter().any(|(i, _)| *i == index("op1.env.decay")));
    }

    #[test]
    fn an_envelope_section_turned_off_keeps_a_level_4() {
        let mut v = defaults();
        v[params::RANDOM_SECTIONS + ENVELOPES * 2 + 1] = 0.0;
        v[index("op1.level4")] = 60.0;
        assert!(!randomize(&v, Some(2), Some(1.0)).iter().any(|(i, _)| *i == index("op1.level4")));
    }

    #[test]
    fn without_a_seed_the_stored_one_moves_on() {
        let v = defaults();
        let changes = randomize(&v, None, None);
        assert!(changes.iter().any(|(i, value)| *i == params::RANDOM_SEED && *value == 2.0));
    }

    #[test]
    fn broken_values_are_harmless() {
        let short = vec![f64::NAN; 10];
        let _ = randomize(&short, None, Some(f64::NAN));
    }
}
