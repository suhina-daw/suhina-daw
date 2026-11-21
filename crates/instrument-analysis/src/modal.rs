// Copyright (C) 2026 The Suhina contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! Bounded Fourier approximation of separated exponentially decaying resonances.
//! Preparation only. Assumptions and primary sources: instrument-modal-fitting.md.

use crate::{Error, MAX_SECONDS, RATE, notes, sustain};
use rustfft::{FftPlanner, num_complex::Complex64};
use std::{f64::consts::TAU, ops::Range};
use suhina_dsp::{
    Resampler,
    modal::{
        glide::{GLIDE_POINTS, Glide, MAX_GLIDE_CENTS, MAX_GLIDE_SECONDS},
        model::{MAX_MODEL_MODES, MAX_OTHER_GAIN, ModelMode, ModelRecording, Stereo},
        profile::{MAX_PROFILE_MODES, ModalProfile, ProfileMode},
        sustain::{NOISE_BANDS, Sustain, noise_center_hz},
        tail::{MAX_TAIL_DECAY, MIN_TAIL_DECAY, Tail},
    },
};

/// Version 3 measures and removes vibrato first and adds the held layer; version 4 measures
/// fast partials with short windows and a stereo recording's other channel.
pub const VERSION: u32 = 4;
const MIN_FRAME: usize = 1024;
const MAX_FRAME: usize = 8192;
const MAX_FRAMES: usize = 12;
/// Short windows for partials that die within one or two envelope windows: the smallest size and
/// how many consecutive windows from the start.
const SHORT_FRAME: usize = 512;
const SHORT_FRAMES: usize = 48;
/// A neighbour weaker than this share of a candidate's detection level does not bound its short
/// window (-20 dB).
const NEIGHBOUR_SHARE: f64 = 0.1;
/// A fast first stage: how far above the slow decay a short window must stand, and in how many.
const EARLY_EXCESS_DB: f64 = 1.0;
const EARLY_MIN_WINDOWS: usize = 3;
/// Splitting a chord: candidate fundamentals, partials per note, how near a partial lies, the
/// inharmonicities tried, partials a note needs, how many notes and how weak the last may be
/// against the first.
/// The tail: Hann frames and their hop, at most this many, measured from this time, while
/// within this power range of a band's largest, from at least this many frames falling at least
/// this many decibels.
const TAIL_FRAME: usize = 2048;
const TAIL_HOP: usize = 1024;
const TAIL_MAX_FRAMES: usize = 192;
const TAIL_START: f64 = 0.1;
const TAIL_RANGE: f64 = 1e-5;
const TAIL_MIN_FRAMES: usize = 4;
const TAIL_MIN_DROP_DB: f64 = 6.0;
/// A band's tail must hold at least this share of the power the modes play in it.
const TAIL_MIN_SHARE: f64 = 1e-3;
/// And at least this share of what the modes play in every band together (-60 dB).
const TAIL_MIN_WHOLE: f64 = 1e-6;
/// How many bins around a mode the tail leaves out, and how many open bins a band needs.
const TAIL_MASK_BINS: f64 = 2.0;
const TAIL_MIN_OPEN_BINS: usize = 2;
/// The frames a band's floor is the median of, and how far above it a frame must stand.
const TAIL_FLOOR_FRAMES: usize = 8;
const TAIL_ABOVE_FLOOR: f64 = 10.0;
const NOTE_LOW_HZ: f64 = 27.5;
const NOTE_HIGH_HZ: f64 = 4200.0;
const NOTE_PARTIALS: usize = 32;
const NOTE_TOLERANCE: f64 = 0.01;
const NOTE_INHARMONICITY: [f64; 4] = [0.0, 1e-4, 3e-4, 1e-3];
const NOTE_MIN_PARTIALS: usize = 3;
const MAX_NOTES: usize = 8;
const NOTE_MIN_SHARE: f64 = 0.1;
/// A partial counts at most this many times its note's fundamental; each partial number adds
/// this much relative distance when modes are shared out.
const NOTE_SMOOTHNESS: f64 = 2.0;
const NOTE_ORDER_COST: f64 = 2e-4;
const MAX_CANDIDATES: usize = 192;
/// Candidates in all once a chord's notes add their own partials.
const MAX_CHORD_CANDIDATES: usize = 384;
/// Where the residual is searched for hidden partials: after the 0.25 s attack.
const RESIDUAL_OFFSET: usize = (ATTACK_SECONDS * RATE as f64) as usize;
/// Partials along the string (instrument-modal-fitting.md): the fundamental's distance from the
/// reference key, the inharmonicity bound, the search width and the weakest partial kept.
const FUNDAMENTAL_RATIO: f64 = 1.041_299; // 70 cents
const MAX_INHARMONICITY: f64 = 0.01;
const PARTIAL_SEARCH: f64 = 0.01;
const PARTIAL_FLOOR: f64 = 1e-4;
const PARTIAL_PROMINENCE: f64 = 2.0;
const MAX_PARTIALS: usize = 128;
const CHUNK: usize = 256;
const REFINEMENTS: usize = 20;
const DB: f64 = 20.0 / std::f64::consts::LN_10;
/// A mode's span ends this far below its own peak, where leakage and noise dominate.
const SPAN_FLOOR_DB: f64 = 60.0;
/// A mode's span ends where its level rises this much above its lowest so far.
const RERISE_DB: f64 = 6.0;
const MAX_ESTIMATION_DB: f64 = 2.5;
const MAX_VALIDATION_DB: f64 = 3.0;
const MIN_DROP_DB: f64 = 1.0;
/// The fitted fall must exceed the larger envelope error this many times.
const DROP_TO_ERROR: f64 = 3.0;
/// A mode whose fitted level changes by at most this across its span, without clearly
/// decaying, is sustained.
const SUSTAIN_DB: f64 = 3.0;
/// T60 stored for a sustained mode: the profile's maximum; note-off releases it.
const SUSTAINED_SECONDS: f64 = 60.0;
/// A failing span this long is fitted once more on its last two thirds.
const LATE_RETRY_WINDOWS: usize = 9;
/// A partial that fails every fit is kept loose with at least this many windows, this fall over
/// them and at most this RMS envelope error (see instrument-modal-fitting.md › Loose partials).
const LOOSE_MIN_WINDOWS: usize = 6;
const LOOSE_DROP_DB: f64 = 6.0;
const LOOSE_MAX_DB: f64 = 15.0;
/// Each third of a loose span's windows is on average at least this much below the one before:
/// a burst followed by a flat floor is not a decay.
const LOOSE_THIRD_DB: f64 = 1.0;
/// Prompt sound and aftersound: how deep the null lies under the peak, how smooth the aftersound
/// must be, how well the pair must describe the windows, and the aftersound's largest share of
/// the prompt sound at the strike.
const ANTI_NULL_DB: f64 = 10.0;
const ANTI_SMOOTH_DB: f64 = 3.0;
const ANTI_MAX_DB: f64 = 6.0;
const ANTI_MAX_SHARE: f64 = 0.5;
/// How far the short windows look for a null, and how far above the aftersound a window before
/// it must stand to count for the prompt sound.
const ANTI_SECONDS: f64 = 6.0;
const ANTI_EARLY: f64 = 3.0;
/// The aftersound is fitted while within this many decibels of its loudest window.
const ANTI_LATE_DB: f64 = 20.0;
/// Work steps of the null search with the shortest window (6 s of 512-sample windows).
const ANTI_WORK: usize = (ANTI_SECONDS * RATE as f64) as usize / SHORT_FRAME;
/// A held partial at an uneven level: windows it needs, how far below the span's median a window
/// counts as a dip and the largest share of dips (see instrument-modal-fitting.md › Held
/// partials at an uneven level).
const HELD_MIN_WINDOWS: usize = 5;
const HELD_DIP_DB: f64 = 20.0;
const HELD_MAX_DIPS: f64 = 0.25;
/// A held part ends at the last window within this much of its opening level: the release.
const HELD_RELEASE_DB: f64 = 6.0;
/// The held part starts after the attack, as the held layer's measurements do.
const HELD_FROM_SECONDS: f64 = 0.15;
/// A partial follows the note's trend only when its loudest window is within this much of the
/// note's loudest level.
const TREND_MIN_DB: f64 = 30.0;
/// How far a detection peak must stand above the median of its neighbours 3..8 bins away (20 dB),
/// the 10 dB (within 30 dB of the strongest) a sound gets when no peak stands that far out, and
/// the 6 dB (within 20 dB) when none stands even that far.
const PEAK_PROMINENCE: f64 = 10.0;
const PEAK_PROMINENCE_FALLBACK: f64 = 3.162_277_66;
const PEAK_RANGE_FALLBACK: f64 = 0.031_622_776_6;
const PEAK_PROMINENCE_LAST: f64 = 1.995_262_315;
const PEAK_RANGE_LAST: f64 = 0.1;
/// A glide: at least this many voiced frames, starting at least this far from the note it
/// settles on, settled once within this much of it, and lasting at least this long.
const GLIDE_MIN_FRAMES: usize = 8;
const GLIDE_SEMITONES: f64 = 1.0;
const GLIDE_SETTLED_SEMITONES: f64 = 0.3;
const GLIDE_MIN_SECONDS: f64 = 0.02;
/// While a glide is still a semitone or more away, four of the note's partials hold at most a
/// tenth (-10 dB) of the power they hold after it.
const GLIDE_CHECK_PARTIALS: usize = 4;
const GLIDE_QUIET: f64 = 0.1;
/// The note's trend is fitted while its level is within this much of its loudest window.
const TREND_FLOOR_DB: f64 = 40.0;
/// Window of the whole level a held note is told by (46 ms).
const HELD_LEVEL_FRAME: usize = 1024;
/// The last resort: windows it needs and how far its first third must stand above its last.
const FALLBACK_MIN_WINDOWS: usize = 4;
const FALLBACK_DROP_DB: f64 = 3.0;
/// A fast part is kept when it lowers the span's RMS envelope error by this much, or leaves at
/// most this fraction of a single exponential's error of at least this much.
const TWO_STAGE_GAIN_DB: f64 = 1.0;
const TWO_STAGE_RATIO: f64 = 1.0 / 3.0;
const TWO_STAGE_MIN_SINGLE_DB: f64 = 0.25;
/// Fine envelope for beating: 1024-sample windows every 256 samples.
const BEAT_FRAME: usize = 1024;
const BEAT_HOP: usize = 256;
const MIN_BEAT_SECONDS: f64 = 0.1;
const MAX_BEAT_SECONDS: f64 = 5.0;
const MIN_BEAT_CORRELATION: f64 = 0.5;
const MIN_BEAT_RIPPLE_DB: f64 = 1.0;
/// Attack: the first 0.25 s, kept when it holds at least 0.1 % of the signal's energy there.
const ATTACK_SECONDS: f64 = 0.25;
/// The highest source rate an attack keeps (a model set's attacks are at most 48 kHz).
const MAX_ATTACK_RATE: u32 = 48_000;
const MIN_ATTACK_ENERGY: f64 = 1e-3;
/// Velocity a fitted recording stands for until the person says otherwise.
pub const DEFAULT_VELOCITY: f32 = 0.8;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Outcome {
    Fitted,
    Silent,
    TooShort,
    NoDecayingModes,
}

/// Errors measure one mode's amplitude trajectory, not waveform fidelity or realism.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct ModeEvidence {
    pub frequency_hz: f64,
    pub decay_seconds: f64,
    pub estimation_rms_db: f64,
    pub validation_rms_db: f64,
    pub estimation_windows: usize,
    pub validation_windows: usize,
    /// The level did not fall: a bowed, blown or held partial stored with the longest T60.
    pub sustained: bool,
    /// T60 of a faster first part, and its share of the magnitude; 0 for none.
    pub fast_decay_seconds: f64,
    pub fast_share: f64,
    /// Rate of a beat in the envelope and the beating partner's share; 0 for none.
    pub beat_hz: f64,
    pub beat_share: f64,
    /// Radians at the strike.
    pub phase: f64,
    /// Kept although its envelope failed the error limits: in the recording, not the profile.
    pub loose: bool,
}

#[derive(Clone, Debug, PartialEq)]
pub struct Report {
    pub version: u32,
    pub range: Range<usize>,
    /// Zero-based channel with greatest AC energy, which the modes are fitted on; a stereo
    /// recording's other channel is measured against it (version 4).
    pub channel: u16,
    pub frame_size: usize,
    pub analyzed_seconds: f64,
    pub outcome: Outcome,
    pub profile: Option<ModalProfile>,
    pub modes: Vec<ModeEvidence>,
    /// Up to 32 modes with phases, two-part decays, partners and the attack, for a revision-4
    /// set. `modes` lists the profile's evidence; `recording_modes` this recording's.
    pub recording: Option<ModelRecording>,
    pub recording_modes: Vec<ModeEvidence>,
    /// A chord split into one recording per note (version 4); empty for fewer than two notes.
    pub notes: Vec<ModelRecording>,
}

struct Work<F> {
    done: usize,
    total: usize,
    progress: F,
}
impl<F: FnMut(usize, usize) -> bool> Work<F> {
    fn step(&mut self) -> Result<(), Error> {
        if !(self.progress)(self.done, self.total) {
            return Err(Error::Cancelled);
        }
        self.done = self.done.saturating_add(1).min(self.total);
        Ok(())
    }
    fn finish(&mut self) -> Result<(), Error> {
        if (self.progress)(self.total, self.total) { Ok(()) } else { Err(Error::Cancelled) }
    }
}

/// Fit one isolated struck source, retaining the caller's chosen reference key.
/// All preparation is bounded, deterministic, offline and cooperatively cancellable.
pub fn fit(
    samples: &[f32],
    sample_rate: u32,
    channels: u16,
    range: Range<usize>,
    reference_key: u8,
    progress: impl FnMut(usize, usize) -> bool,
) -> Result<Report, Error> {
    if reference_key > 127 {
        return Err(Error::InvalidSettings);
    }
    if !(1..=2).contains(&channels)
        || !(1000..=384000).contains(&sample_rate)
        || !samples.len().is_multiple_of(usize::from(channels))
    {
        return Err(Error::InvalidFormat);
    }
    let channels = usize::from(channels);
    if range.start >= range.end || range.end > samples.len() / channels {
        return Err(Error::InvalidRange);
    }
    let length = (range.end - range.start).min(sample_rate as usize * MAX_SECONDS);
    let range = range.start..range.start + length;
    let first = range.start.checked_mul(channels).ok_or(Error::InvalidRange)?;
    let last = range.end.checked_mul(channels).ok_or(Error::InvalidRange)?;
    let input = samples.get(first..last).ok_or(Error::InvalidRange)?;
    let total = 2 * length.div_ceil(CHUNK)
        + MAX_CHORD_CANDIDATES * (REFINEMENTS + MAX_FRAMES + 2 * SHORT_FRAMES + ANTI_WORK + 4)
        + MAX_MODEL_MODES
        + notes::MAX_NOTE_FRAMES
        + 8;
    let mut work = Work { done: 0, total, progress };
    let mut report = Report {
        version: VERSION,
        range,
        channel: 0,
        frame_size: 0,
        analyzed_seconds: 0.0,
        outcome: Outcome::NoDecayingModes,
        profile: None,
        modes: Vec::new(),
        recording: None,
        recording_modes: Vec::new(),
        notes: Vec::new(),
    };
    let mut sums = [0.0_f64; 2];
    let mut squares = [0.0_f64; 2];
    let mut peaks = [0.0_f64; 2];
    let mut minima = [f64::INFINITY; 2];
    let mut maxima = [f64::NEG_INFINITY; 2];
    for chunk in input.chunks(CHUNK * channels) {
        work.step()?;
        for frame in chunk.chunks_exact(channels) {
            for (channel, &sample) in frame.iter().enumerate() {
                if !sample.is_finite() {
                    return Err(Error::NonFinite);
                }
                if let (Some(sum), Some(square), Some(peak)) =
                    (sums.get_mut(channel), squares.get_mut(channel), peaks.get_mut(channel))
                {
                    let value = f64::from(sample);
                    *sum += value;
                    *square += value * value;
                    *peak = peak.max(value.abs());
                    if let (Some(low), Some(high)) = (minima.get_mut(channel), maxima.get_mut(channel)) {
                        *low = low.min(value);
                        *high = high.max(value);
                    }
                }
            }
        }
    }
    let energy = |c: usize| squares[c] - sums[c] * sums[c] / length as f64;
    let channel = usize::from(channels == 2 && energy(1) > energy(0));
    report.channel = channel as u16;
    // A constant channel is silent even when the one-pass energy keeps rounding residue: summing
    // tens of thousands of equal f32 values leaves more than the energy threshold in f64.
    let span = maxima[channel] - minima[channel];
    if peaks[channel] <= 1e-12
        || span <= peaks[channel] * 1e-9
        || energy(channel) <= peaks[channel].powi(2) * length as f64 * 1e-14
    {
        report.outcome = Outcome::Silent;
        work.finish()?;
        return Ok(report);
    }
    let mean = sums[channel] / length as f64;
    let scale = (maxima[channel] - mean).abs().max((minima[channel] - mean).abs()).max(1e-12);
    let prepare = Prepare { input, channels, sample_rate, length, scale };
    let mut mono = prepare.channel(channel, mean, &mut work)?;
    // The other channel of a stereo recording, at the chosen channel's scale so their levels
    // compare; none when it matches the chosen one.
    let mut other = if channels == 2 {
        let index = 1 - channel;
        let prepared = prepare.channel(index, sums[index] / length as f64, &mut work)?;
        prepared.iter().zip(&mono).any(|(a, b)| (a - b).abs() > 1e-6).then_some(prepared)
    } else {
        None
    };
    // Long silent tails must not turn a quick strike into widely separated empty windows.
    // Use the residual's AC span, rather than a DC offset introduced by mean subtraction.
    let tail_mean = mono.iter().rev().take(MIN_FRAME).sum::<f64>() / mono.len().clamp(1, MIN_FRAME) as f64;
    let mono_peak = mono.iter().map(|x| (x - tail_mean).abs()).fold(0.0_f64, f64::max);
    let useful = mono.iter().rposition(|x| (x - tail_mean).abs() > mono_peak * 1e-5).map_or(0, |i| i + 1);
    mono.truncate(useful);
    // A glide into the note smears every partial it moves: measure it densely, resample the
    // recording flat along it and keep it for playback (version 4).
    work.step()?;
    let glide = note_glide(&mono, sample_rate, &mut work)?;
    if let Some(glide) = &glide {
        mono = sustain::flatten_by(&mono, |t| glide.cents_at(t));
        other = other.map(|other| sustain::flatten_by(&other, |t| glide.cents_at(t)));
    }
    // Vibrato smears every partial: measure the pitch and resample it flat before fitting.
    work.step()?;
    let pitch = sustain::pitch(&mono);
    let vibrato = pitch.as_ref().and_then(sustain::vibrato);
    if let (Some(pitch), Some(vibrato)) = (&pitch, &vibrato) {
        mono = sustain::flatten(&mono, pitch, vibrato);
        other = other.map(|other| sustain::flatten(&other, pitch, vibrato));
    }
    if let Some(other) = other.as_mut() {
        other.resize(mono.len(), 0.0);
    }
    work.step()?;
    let mut frame_size = MIN_FRAME;
    while frame_size < MAX_FRAME && frame_size * 2 * MAX_FRAMES <= mono.len() {
        frame_size *= 2;
    }
    let frames = (mono.len() / frame_size).min(MAX_FRAMES);
    if frames < 6 {
        report.outcome = Outcome::TooShort;
        work.finish()?;
        return Ok(report);
    }
    report.frame_size = frame_size;
    report.analyzed_seconds = (frames * frame_size) as f64 / f64::from(RATE);
    let window = hann(frame_size);
    // Detection resolves the partials of a low note that a short envelope window would merge.
    let mut detection_size = frame_size;
    while detection_size < MAX_FRAME && detection_size * 2 <= mono.len() {
        detection_size *= 2;
    }
    let detection = mono.get(..detection_size).ok_or(Error::InvalidRange)?;
    let detection_window = hann(detection_size);
    let mut bins: Vec<Complex64> =
        detection.iter().zip(&detection_window).map(|(x, w)| Complex64::new(x * w, 0.0)).collect();
    let fft = FftPlanner::<f64>::new().plan_fft_forward(detection_size);
    let mut scratch = vec![Complex64::default(); fft.get_inplace_scratch_len()];
    work.step()?;
    fft.process_with_scratch(&mut bins, &mut scratch);
    work.step()?;
    let reference_hz = 440.0 * 2.0_f64.powf((f64::from(reference_key) - 69.0) / 12.0);
    let low_hz = 40.0_f64.max(reference_hz / 256.0);
    let high_hz = 10000.0_f64.min(f64::from(sample_rate) * 0.4).min(reference_hz * 256.0);
    let bin_hz = f64::from(RATE) / detection_size as f64;
    // Two candidates inside one envelope window's main lobe would measure the same energy.
    let separation_bins = 2.0 * detection_size as f64 / frame_size as f64;
    let spectrum: Vec<f64> = bins.iter().take(detection_size / 2).map(|x| x.norm()).collect();
    let highest = spectrum.iter().copied().fold(0.0_f64, f64::max);
    let mut peaks = prominent_peaks(&spectrum, bin_hz, low_hz, high_hz);
    peaks.sort_by(|a, b| b.1.total_cmp(&a.1).then_with(|| a.0.cmp(&b.0)));
    let mut candidates = Vec::with_capacity(MAX_CANDIDATES);
    for (bin, _) in peaks {
        if candidates.iter().all(|other: &usize| other.abs_diff(bin) as f64 >= separation_bins.max(3.0)) {
            candidates.push(bin);
        }
        if candidates.len() == MAX_CANDIDATES {
            break;
        }
    }
    // The prominent peaks first: the string's other partials are predicted from them.
    let mut frequencies = Vec::with_capacity(MAX_CANDIDATES);
    for bin in candidates {
        let hz = refine(
            detection,
            &detection_window,
            (bin as f64 - 0.75) * bin_hz,
            (bin as f64 + 0.75) * bin_hz,
            &mut work,
        )?;
        if (low_hz..=high_hz).contains(&hz) {
            frequencies.push(hz);
        }
    }
    let string = Spectrum { values: &spectrum, bin_hz, highest, separation_hz: separation_bins.max(3.0) * bin_hz };
    for bin in string_partials(&frequencies, reference_hz, high_hz, &string, MAX_CANDIDATES) {
        let hz = refine(
            detection,
            &detection_window,
            (bin as f64 - 0.75) * bin_hz,
            (bin as f64 + 0.75) * bin_hz,
            &mut work,
        )?;
        if (low_hz..=high_hz).contains(&hz) {
            frequencies.push(hz);
        }
    }
    // Short windows, smallest first, for partials the envelope windows cannot follow.
    let mut short_windows = Vec::new();
    let mut size = SHORT_FRAME;
    while size < frame_size {
        short_windows.push(hann(size));
        size *= 2;
    }
    // A held note (its whole level holds after the attack, a release may follow): every
    // partial is first tried as held, so a bowed note's partials are not split between held and
    // decaying by their own unevenness.
    let level = held_level(&mono, frame_size, frames);
    let held_note = level.held;
    let fitter = Fitter { mono: &mono, window: &window, frames, short_windows: &short_windows, level };
    let mut accepted: Vec<Fitted> = Vec::with_capacity(MAX_CANDIDATES);
    for (position, &hz) in frequencies.iter().enumerate() {
        if let Some(fitted) = fitter.fit(hz, gap(&frequencies, position, &string), &mut work)? {
            accepted.push(fitted);
        }
    }
    // A chord: each note's own string series, as for the reference note above. Partials of
    // different notes lie close together, so these take the free peak nearest each prediction.
    let first_notes = chord_notes(&accepted);
    if first_notes.len() >= 2 {
        let start = frequencies.len();
        for note in &first_notes {
            for bin in note_partials(&frequencies, note.f0, high_hz, &string, MAX_CHORD_CANDIDATES) {
                let hz = refine(
                    detection,
                    &detection_window,
                    (bin as f64 - 0.75) * bin_hz,
                    (bin as f64 + 0.75) * bin_hz,
                    &mut work,
                )?;
                if (low_hz..=high_hz).contains(&hz) {
                    frequencies.push(hz);
                }
            }
        }
        for position in start..frequencies.len() {
            let Some(&hz) = frequencies.get(position) else { continue };
            if let Some(fitted) = fitter.fit(hz, gap(&frequencies, position, &string), &mut work)? {
                accepted.push(fitted);
            }
        }
    }
    // Analysis by synthesis: a partial a strong neighbour hid in the detection spectrum stands
    // out once the fitted modes are subtracted from the detection window.
    if !accepted.is_empty() {
        // After the attack, whose broadband knock would hide them again, when the interval is
        // long enough.
        let offset = if mono.len() >= RESIDUAL_OFFSET + detection_size { RESIDUAL_OFFSET } else { 0 };
        let (rendered, _) = render_modes(&accepted, offset + detection_size, false);
        let heard = mono.get(offset..offset + detection_size).unwrap_or(detection);
        let residual: Vec<f64> = heard.iter().zip(rendered.get(offset..).unwrap_or(&[])).map(|(x, m)| x - m).collect();
        let mut bins: Vec<Complex64> =
            residual.iter().zip(&detection_window).map(|(x, w)| Complex64::new(x * w, 0.0)).collect();
        work.step()?;
        fft.process_with_scratch(&mut bins, &mut scratch);
        let left: Vec<f64> = bins.iter().take(detection_size / 2).map(|x| x.norm()).collect();
        let start = frequencies.len();
        for bin in residual_peaks(&left, &frequencies, &string, MAX_CHORD_CANDIDATES) {
            let hz = refine(
                &residual,
                &detection_window,
                (bin as f64 - 0.75) * bin_hz,
                (bin as f64 + 0.75) * bin_hz,
                &mut work,
            )?;
            if (low_hz..=high_hz).contains(&hz) {
                frequencies.push(hz);
            }
        }
        for position in start..frequencies.len() {
            let Some(&hz) = frequencies.get(position) else { continue };
            if let Some(fitted) = fitter.fit(hz, gap(&frequencies, position, &string), &mut work)? {
                accepted.push(fitted);
            }
        }
    }
    // A later pass can refine onto a peak an earlier one fitted: one mode per peak (within a
    // detection bin), the more energetic.
    accepted.sort_by(|a, b| a.hz.total_cmp(&b.hz));
    accepted.dedup_by(|later, kept| {
        let same = (later.hz - kept.hz).abs() < bin_hz;
        if same && later.energy() > kept.energy() {
            std::mem::swap(later, kept);
        }
        same
    });
    // Schema-1 profile: the 16 largest slow magnitudes of validated modes, as before.
    let mut largest: Vec<&Fitted> = accepted.iter().filter(|f| !f.evidence.loose).collect();
    // A sound whose partials are all loose (a breathy stab, a fading wind note) still has them:
    // its profile takes the loose modes rather than none.
    if largest.is_empty() {
        largest = accepted.iter().collect();
    }
    largest.sort_by(|a, b| b.slow.total_cmp(&a.slow).then_with(|| a.hz.total_cmp(&b.hz)));
    largest.truncate(MAX_PROFILE_MODES);
    largest.sort_by(|a, b| a.hz.total_cmp(&b.hz));
    let sum = largest.iter().map(|f| f.slow).sum::<f64>().max(1.0);
    let modes: Vec<ProfileMode> = largest
        .iter()
        .map(|f| ProfileMode {
            ratio: f.hz / reference_hz,
            magnitude: f.slow / sum,
            decay_seconds: f.evidence.decay_seconds,
        })
        .collect();
    if !modes.is_empty() {
        report.profile =
            Some(ModalProfile::new(reference_key, reference_hz, &modes).map_err(|_| Error::InvalidSettings)?);
        report.modes = largest.iter().map(|f| f.evidence).collect();
        report.outcome = Outcome::Fitted;
        // Beats and stereo placements for every accepted mode: a chord's notes draw on all of
        // them, the recording on the 64 most energetic.
        accepted.sort_by(|a, b| a.hz.total_cmp(&b.hz));
        let beat_window = hann(BEAT_FRAME);
        for fitted in &mut accepted {
            work.step()?;
            if let Some((rate, share)) = beat(&mono, &beat_window, fitted) {
                fitted.evidence.beat_hz = rate;
                fitted.evidence.beat_share = share;
            }
        }
        // Each mode on both channels through the detection window: the other channel's gain and
        // phase offset against the chosen one.
        let other_detection = other.as_ref().and_then(|other| other.get(..detection_size));
        if let Some(other_detection) = other_detection {
            let measured: Vec<_> = accepted
                .iter()
                .map(|f| {
                    (
                        projection_complex(detection, &detection_window, f.hz),
                        projection_complex(other_detection, &detection_window, f.hz),
                    )
                })
                .collect();
            let largest = measured.iter().map(|((a, _), _)| *a).fold(0.0_f64, f64::max);
            for (fitted, &((a, phase_a), (b, phase_b))) in accepted.iter_mut().zip(&measured) {
                if a > largest * 1e-9 && a > 0.0 {
                    fitted.other = ((b / a).clamp(0.0, f64::from(MAX_OTHER_GAIN)), wrap(phase_b - phase_a));
                }
            }
        }
        let stereo = other_detection.and(other.as_deref()).map(|other| (other, channel as u8));
        let notes = chord_notes(&accepted);
        let every_mode = accepted;
        let accepted = most_energetic(&every_mode);
        work.step()?;
        let sum: f64 = accepted.iter().map(Fitted::magnitude).sum();
        let held_sum: f64 = accepted.iter().filter(|f| f.evidence.sustained).map(Fitted::magnitude).sum();
        let held = held_note || (sum > 0.0 && held_sum >= 0.5 * sum);
        let layer = if held || vibrato.is_some() {
            let inputs: Vec<sustain::ModeInput> = accepted
                .iter()
                .map(|f| sustain::ModeInput {
                    hz: f.hz,
                    partner_hz: (f.evidence.beat_hz > 0.0).then_some(f.hz + f.evidence.beat_hz),
                    magnitude: f.magnitude(),
                    sustained: f.evidence.sustained,
                })
                .collect();
            sustain::layer(&mono, &inputs, pitch.as_ref(), vibrato.as_ref(), held, sum)
        } else {
            None
        };
        work.step()?;
        let whole = Residual { against: &accepted, share: 1.0, layer };
        work.step()?;
        let missing = tail(&accepted, &mono, stereo.map(|(other, _)| other));
        let sum: f64 = accepted.iter().map(Fitted::magnitude).sum();
        let own_tail = missing.as_ref().and_then(|m| m.tail(sum, 1.0));
        // The attack at the source's own rate when it is at most 48 kHz and the recording was
        // not resampled flat for a vibrato: the knock keeps its top octave and its edge.
        let attack_frames = ((ATTACK_SECONDS * f64::from(sample_rate)) as usize).min(length);
        let full_rate = sample_rate <= MAX_ATTACK_RATE && vibrato.is_none() && glide.is_none();
        let cut_channel = |index: usize| -> Vec<f64> {
            let mean = sums.get(index).copied().unwrap_or(0.0) / length as f64;
            input
                .chunks_exact(channels)
                .take(attack_frames)
                .map(|frame| (f64::from(frame.get(index).copied().unwrap_or(0.0)) - mean) / scale)
                .collect()
        };
        let full_reference = if full_rate { cut_channel(channel) } else { Vec::new() };
        let full_other = if full_rate && stereo.is_some() { cut_channel(1 - channel) } else { Vec::new() };
        let attack_source = if full_rate {
            AttackSource {
                rate: sample_rate,
                reference: &full_reference,
                other: stereo.is_some().then_some(full_other.as_slice()),
            }
        } else {
            AttackSource { rate: RATE, reference: &mono, other: stereo.map(|(other, _)| other) }
        };
        let source = Source { scale, stereo, attack: attack_source };
        report.recording = recording(&accepted, &whole, &source, reference_key, reference_hz, own_tail)
            .map(|recording| ModelRecording { glide, ..recording });
        report.recording_modes = accepted.iter().map(|f| f.evidence).collect();
        // A chord: one recording per note, its attack the residual against every note's modes.
        if notes.len() >= 2 {
            let parts: Vec<Vec<Fitted>> = split_modes(&every_mode, &notes);
            let kept: Vec<Vec<Fitted>> = parts.iter().map(|modes| most_energetic(modes)).collect();
            let every: Vec<Fitted> = kept.iter().flatten().cloned().collect();
            let shared = Residual { against: &every, share: 1.0 / notes.len() as f64, layer: None };
            // What all the notes together miss; each note's independent noise takes an equal
            // share of its power.
            work.step()?;
            let missing = tail(&every, &mono, stereo.map(|(other, _)| other));
            let power_share = 1.0 / (notes.len() as f64).sqrt();
            report.notes = kept
                .iter()
                .zip(&notes)
                .filter_map(|(modes, note)| {
                    let key = note.key;
                    let hz = 440.0 * 2.0_f64.powf((f64::from(key) - 69.0) / 12.0);
                    let sum: f64 = modes.iter().map(Fitted::magnitude).sum();
                    let note_tail = missing.as_ref().and_then(|m| m.tail(sum, power_share));
                    recording(modes, &shared, &source, key, hz, note_tail)
                })
                .collect();
            if report.notes.len() < 2 {
                report.notes.clear();
            }
        }
    }
    work.finish()?;
    Ok(report)
}

/// The detection spectrum the string's partials are looked for in.
struct Spectrum<'a> {
    values: &'a [f64],
    bin_hz: f64,
    highest: f64,
    /// Closest two candidates may be, in Hz.
    separation_hz: f64,
}

/// Detection bins of the string's partials that prominence missed: from the fundamental nearest
/// `reference_hz` and an inharmonicity fitted to the `found` frequencies, each predicted partial
/// below `high_hz` with a peak near it and no candidate there yet, until `total` candidates.
fn string_partials(
    found: &[f64],
    reference_hz: f64,
    high_hz: f64,
    spectrum: &Spectrum<'_>,
    total: usize,
) -> Vec<usize> {
    let Some(f0) = found
        .iter()
        .copied()
        .filter(|f| (1.0 / FUNDAMENTAL_RATIO..=FUNDAMENTAL_RATIO).contains(&(f / reference_hz)))
        .min_by(|a, b| (a / reference_hz).ln().abs().total_cmp(&(b / reference_hz).ln().abs()))
    else {
        return Vec::new();
    };
    // The measured first partial is itself stretched, f1 = f0 sqrt(1 + B), so against it
    // (r/k)^2 - 1 = B (k^2 - (r/k)^2) with r = f_k / f1: least squares through the origin.
    let b = string_inharmonicity(found, f0);
    let values = spectrum.values;
    let mut taken: Vec<f64> = found.to_vec();
    let mut bins = Vec::new();
    for k in 1..=MAX_PARTIALS {
        if taken.len() >= total {
            break;
        }
        let k = k as f64;
        let hz = k * f0 * ((1.0 + b * k * k) / (1.0 + b)).sqrt();
        if !hz.is_finite() || hz > high_hz {
            break;
        }
        let near = spectrum.separation_hz.max(PARTIAL_SEARCH * hz);
        if taken.iter().any(|f| (f - hz).abs() < near) {
            continue;
        }
        let low = (hz * (1.0 - PARTIAL_SEARCH) / spectrum.bin_hz).floor().max(2.0) as usize;
        let high =
            ((hz * (1.0 + PARTIAL_SEARCH) / spectrum.bin_hz).ceil() as usize).min(values.len().saturating_sub(10));
        let Some((bin, value)) =
            (low..=high).filter_map(|b| values.get(b).map(|v| (b, *v))).max_by(|a, b| a.1.total_cmp(&b.1))
        else {
            continue;
        };
        let (Some(&before), Some(&after)) = (values.get(bin - 1), values.get(bin + 1)) else { continue };
        if value < spectrum.highest * PARTIAL_FLOOR || value <= before || value <= after {
            continue;
        }
        let mut nearby = [0.0_f64; 12];
        for (slot, delta) in nearby.iter_mut().zip([-8_isize, -7, -6, -5, -4, -3, 3, 4, 5, 6, 7, 8]) {
            *slot = bin.checked_add_signed(delta).and_then(|b| values.get(b)).copied().unwrap_or(value);
        }
        nearby.sort_by(f64::total_cmp);
        if value < nearby[6] * PARTIAL_PROMINENCE {
            continue;
        }
        taken.push(bin as f64 * spectrum.bin_hz);
        bins.push(bin);
    }
    bins
}

/// One accepted mode: frequency, slow and fast magnitudes at the strike (prepared units), and
/// its other-channel gain and phase offset in a stereo recording.
#[derive(Clone)]
struct Fitted {
    hz: f64,
    slow: f64,
    fast: f64,
    evidence: ModeEvidence,
    other: (f64, f64),
}

impl Fitted {
    fn magnitude(&self) -> f64 {
        self.slow + self.fast
    }

    fn energy(&self) -> f64 {
        self.magnitude().powi(2) * self.evidence.decay_seconds
    }

    /// The parts this mode renders: (frequency, T60, amplitude) at the strike.
    fn parts(&self) -> [(f64, f64, f64); 3] {
        let e = &self.evidence;
        let partner = if e.beat_hz > 0.0 { e.beat_share } else { 0.0 };
        [
            (self.hz, e.decay_seconds, self.slow * (1.0 - partner)),
            (self.hz, e.fast_decay_seconds, self.fast),
            (self.hz + e.beat_hz, e.decay_seconds, self.slow * partner),
        ]
    }
}

fn wrap(phase: f64) -> f64 {
    let wrapped = (phase + std::f64::consts::PI).rem_euclid(TAU) - std::f64::consts::PI;
    wrapped.clamp(-std::f64::consts::PI, std::f64::consts::PI)
}

/// A beat in the mode's fine envelope after its fitted decay: (rate Hz, partner share).
fn beat(mono: &[f64], window: &[f64], fitted: &Fitted) -> Option<(f64, f64)> {
    let e = &fitted.evidence;
    let ln1000 = 1000.0_f64.ln();
    let model = |t: f64| {
        let slow = fitted.slow * (-ln1000 * t / e.decay_seconds).exp();
        let fast =
            if e.fast_decay_seconds > 0.0 { fitted.fast * (-ln1000 * t / e.fast_decay_seconds).exp() } else { 0.0 };
        slow + fast
    };
    let mut residual = Vec::with_capacity(mono.len() / BEAT_HOP + 1);
    let mut start = 0;
    while let Some(frame) = mono.get(start..start + BEAT_FRAME) {
        let t = (start + BEAT_FRAME / 2) as f64 / f64::from(RATE);
        let level = model(t);
        // Only while the model is within 40 dB of the strike: later, noise and leakage rule.
        if level < fitted.magnitude() * 0.01 {
            break;
        }
        let measured = projection(frame, window, fitted.hz);
        if measured > 1e-9 {
            residual.push((measured / level).ln() * DB);
        }
        start += BEAT_HOP;
    }
    let n = residual.len();
    if n < 32 {
        return None;
    }
    // Remove the best straight line too: a slightly wrong T60 leaves a drift, not a beat.
    let points: Vec<(f64, f64)> = residual.iter().enumerate().map(|(i, r)| (i as f64, *r)).collect();
    let (intercept, slope) = line(&points)?;
    for (i, r) in residual.iter_mut().enumerate() {
        *r -= intercept + slope * i as f64;
    }
    let power = residual.iter().map(|r| r * r).sum::<f64>() / n as f64;
    let ripple = power.sqrt();
    if ripple < MIN_BEAT_RIPPLE_DB {
        return None;
    }
    let hop_seconds = BEAT_HOP as f64 / f64::from(RATE);
    let first = (MIN_BEAT_SECONDS / hop_seconds).ceil() as usize;
    let last = ((MAX_BEAT_SECONDS / hop_seconds) as usize).min(n / 2);
    if first >= last {
        return None;
    }
    let correlation = |lag: usize| {
        let sum: f64 = residual.iter().zip(residual.iter().skip(lag)).map(|(a, b)| a * b).sum();
        sum / (n - lag) as f64 / power
    };
    // The strongest peak that rises at least 0.3 above the valley before it: a smooth misfit
    // decorrelates monotonically and has none.
    let mut valley = f64::INFINITY;
    let mut best: Option<(usize, f64)> = None;
    let mut previous = correlation(first);
    for lag in first + 1..last {
        let value = correlation(lag);
        let next = correlation(lag + 1);
        valley = valley.min(previous);
        if value >= previous
            && value >= next
            && value >= MIN_BEAT_CORRELATION
            && value - valley >= 0.3
            && best.is_none_or(|(_, b)| value > b)
        {
            best = Some((lag, value));
        }
        previous = value;
    }
    let (lag, _) = best?;
    let rate = 1.0 / (lag as f64 * hop_seconds);
    if rate > 0.03 * fitted.hz {
        return None;
    }
    let q = 10.0_f64.powf(2.0 * std::f64::consts::SQRT_2 * ripple / 20.0);
    Some((rate, ((q - 1.0) / (2.0 * q)).clamp(0.0, 0.5)))
}

/// The revision-4 recording: normalized modes, level in source units and the attack residual.
/// What a recording's attack is the residual against: the modes subtracted from the source, the
/// share of the residual it keeps, and the held layer whose onsets the modes swell in by (only
/// when the modes are the recording's own).
struct Residual<'a> {
    against: &'a [Fitted],
    share: f64,
    layer: Option<Sustain>,
}

/// The prepared source a recording is cut from: its scale against the source's full scale, a
/// stereo recording's other channel with the fitted channel's index, and the attack's signals.
struct Source<'a> {
    scale: f64,
    stereo: Option<(&'a [f64], u8)>,
    /// What the attack is cut from: the fitted channel (and the other one) at `rate`, the
    /// source's own rate when it allows, else the prepared 22050 Hz channels.
    attack: AttackSource<'a>,
}

/// The signals an attack is the residual of, at their rate, from the strike.
#[derive(Clone, Copy)]
struct AttackSource<'a> {
    rate: u32,
    reference: &'a [f64],
    other: Option<&'a [f64]>,
}

/// The recording of `accepted` at `key` (`reference_hz` its frequency), its attack the residual
/// against `residual.against`, its stereo part from the source's other channel and its `tail`.
fn recording(
    accepted: &[Fitted],
    residual: &Residual<'_>,
    source: &Source<'_>,
    key: u8,
    reference_hz: f64,
    tail: Option<Tail>,
) -> Option<ModelRecording> {
    let (scale, stereo) = (source.scale, source.stereo);
    let layer = residual.layer;
    let sum: f64 = accepted.iter().map(Fitted::magnitude).sum();
    if !(sum.is_finite() && sum > 0.0) {
        return None;
    }
    let cut = source.attack;
    let length = ((ATTACK_SECONDS * f64::from(cut.rate)) as usize).min(cut.reference.len());
    let ln1000 = 1000.0_f64.ln();
    let mut attack = Vec::with_capacity(length);
    let mut attack_other = Vec::with_capacity(if stereo.is_some() { length } else { 0 });
    let (mut residual_energy, mut signal_energy) = (0.0, 0.0);
    let half = length / 2;
    // A held mode swells in by its onset and rise: the attack holds what the modes miss, not the
    // opposite of their swell.
    let onset = |index: usize, t: f64| {
        let Some(layer) = &layer else { return 1.0 };
        let at = |a: &[f32; MAX_MODEL_MODES]| f64::from(a.get(index).copied().unwrap_or(0.0));
        let (start, rise) = (at(&layer.onset), at(&layer.rise));
        if t < start {
            0.0
        } else if rise > 0.0 {
            1.0 - (-(t - start) * std::f64::consts::LN_10 / rise).exp()
        } else {
            1.0
        }
    };
    for (n, x) in cut.reference.iter().take(length).enumerate() {
        let t = n as f64 / f64::from(cut.rate);
        let (mut model, mut model_other) = (0.0, 0.0);
        for (index, fitted) in residual.against.iter().enumerate() {
            let envelope = onset(index, t);
            let (gain, offset) = fitted.other;
            for (hz, t60, amplitude) in fitted.parts() {
                if amplitude != 0.0 && t60 > 0.0 {
                    let level = envelope * amplitude * (-ln1000 * t / t60).exp();
                    let angle = TAU * hz * t + fitted.evidence.phase;
                    model += level * angle.sin();
                    model_other += level * gain * (angle + offset).sin();
                }
            }
        }
        let fade = if n < half {
            1.0
        } else {
            0.5 * (1.0 + (std::f64::consts::PI * (n - half) as f64 / (length - half).max(1) as f64).cos())
        };
        let left = (x - model) * fade;
        residual_energy += left * left;
        signal_energy += x * x;
        attack.push((left * residual.share / sum) as f32);
        if let Some(y) = cut.other.filter(|_| stereo.is_some()).and_then(|other| other.get(n)) {
            let right = (y - model_other) * fade;
            residual_energy += right * right;
            signal_energy += y * y;
            attack_other.push((right * residual.share / sum) as f32);
        }
    }
    // Both attack channels are kept or dropped together.
    if residual_energy < MIN_ATTACK_ENERGY * signal_energy {
        attack.clear();
        attack_other.clear();
    }
    attack_other.resize(attack.len(), 0.0);
    let modes: Vec<ModelMode> = accepted
        .iter()
        .map(|f| {
            let e = &f.evidence;
            let fast = e.fast_decay_seconds > 0.0 && e.fast_share > 0.0;
            let partner = e.beat_hz > 0.0 && e.beat_share > 0.0;
            ModelMode {
                ratio: f.hz / reference_hz,
                magnitude: f.magnitude() / sum,
                decay_seconds: e.decay_seconds,
                fast_decay_seconds: if fast { e.fast_decay_seconds } else { 0.0 },
                fast_share: if fast { e.fast_share } else { 0.0 },
                beat_ratio: if partner { 1.0 + e.beat_hz / f.hz } else { 1.0 },
                beat_share: if partner { e.beat_share } else { 0.0 },
                phase: e.phase,
            }
        })
        .collect();
    let sustain = layer.map(|mut layer| {
        layer.attack_seconds = (attack.len() as f64 / f64::from(cut.rate)).min(ATTACK_SECONDS) as f32;
        layer
    });
    let recording = ModelRecording {
        key,
        source: 0,
        velocity: DEFAULT_VELOCITY,
        level: sum * scale,
        modes,
        attack_rate: if attack.is_empty() { 0 } else { cut.rate },
        attack,
        sustain: sustain.filter(|s| !s.is_empty() && s.validate(accepted.len()).is_ok()),
        stereo: stereo.map(|(_, reference)| Stereo {
            reference,
            gains: accepted.iter().map(|f| f.other.0 as f32).collect(),
            offsets: accepted.iter().map(|f| f.other.1 as f32).collect(),
            attack: attack_other,
        }),
        tail: tail.filter(|t| !t.is_empty() && t.validate().is_ok()),
        glide: None,
    };
    recording.validate().ok().map(|()| recording)
}

/// What the modes miss after the attack, per noise band and channel: each band's summed
/// missing power at the strike (fitted channel, other channel) and its T60.
struct Missing {
    power: [(f64, f64); NOISE_BANDS],
    decay: [f64; NOISE_BANDS],
    /// What turns a band's summed frame power into an amplitude density per bin.
    density: [f64; NOISE_BANDS],
}

impl Missing {
    /// The tail of a recording whose magnitudes sum to `sum`, its amplitudes times `share`.
    fn tail(&self, sum: f64, share: f64) -> Option<Tail> {
        if !(sum.is_finite() && sum > 0.0) {
            return None;
        }
        let mut tail = Tail::NONE;
        for band in 0..NOISE_BANDS {
            let (Some(&(own, other)), Some(&decay), Some(&density)) =
                (self.power.get(band), self.decay.get(band), self.density.get(band))
            else {
                continue;
            };
            if decay <= 0.0 {
                continue;
            }
            let level = |power: f64| ((power * density).sqrt() * share / sum).min(1.0) as f32;
            if let (Some(a), Some(b), Some(d)) =
                (tail.level.get_mut(band), tail.other.get_mut(band), tail.decay.get_mut(band))
            {
                *a = level(own);
                *b = level(other);
                *d = decay as f32;
            }
        }
        (!tail.is_empty()).then_some(tail)
    }
}

/// The modes rendered over the whole interval (fitted channel, and with `other` the other channel
/// by their stereo placements), against the recording in Hann frames: per noise band, the power
/// the recording has beyond them, its level at the strike and its decay (`Tail` in the spec).
fn tail(modes: &[Fitted], mono: &[f64], other: Option<&[f64]>) -> Option<Missing> {
    let length = mono.len();
    let frames = if length >= TAIL_FRAME { ((length - TAIL_FRAME) / TAIL_HOP + 1).min(TAIL_MAX_FRAMES) } else { 0 };
    if frames < 4 {
        return None;
    }
    let span = (frames - 1) * TAIL_HOP + TAIL_FRAME;
    let (model, model_other) = render_modes(modes, span, other.is_some());
    let window = hann(TAIL_FRAME);
    let fft = FftPlanner::<f64>::new().plan_fft_forward(TAIL_FRAME);
    let mut scratch = vec![Complex64::default(); fft.get_inplace_scratch_len()];
    let spectrum = |signal: &[f64], start: usize, scratch: &mut Vec<Complex64>| -> Vec<f64> {
        let mut bins: Vec<Complex64> = (0..TAIL_FRAME)
            .map(|i| {
                Complex64::new(
                    signal.get(start + i).copied().unwrap_or(0.0) * window.get(i).copied().unwrap_or(0.0),
                    0.0,
                )
            })
            .collect();
        fft.process_with_scratch(&mut bins, scratch);
        bins.iter().take(TAIL_FRAME / 2).map(|b| b.norm_sqr()).collect()
    };
    let bin_hz = f64::from(RATE) / TAIL_FRAME as f64;
    let edge = 2.0_f64.powf(1.0 / 3.0);
    let ranges: Vec<(usize, usize)> = (0..NOISE_BANDS)
        .map(|band| {
            let center = noise_center_hz(band);
            (
                (center / edge / bin_hz).ceil() as usize,
                ((center * edge / bin_hz).floor() as usize).min(TAIL_FRAME / 2 - 1),
            )
        })
        .collect();
    // Bins near a mode or its partner are left out: a mode's small level error is not noise.
    let mut masked = vec![false; TAIL_FRAME / 2];
    for fitted in modes {
        for (hz, _, amplitude) in fitted.parts() {
            if amplitude == 0.0 {
                continue;
            }
            let center = hz / bin_hz;
            let low = (center - TAIL_MASK_BINS).ceil().max(0.0) as usize;
            let high = ((center + TAIL_MASK_BINS).floor().max(0.0) as usize).min(masked.len().saturating_sub(1));
            for flag in masked.iter_mut().take(high + 1).skip(low) {
                *flag = true;
            }
        }
    }
    // Per frame and band: the missing power of each channel, over the band's open bins scaled to
    // all its bins.
    let mut missing = vec![[(0.0_f64, 0.0_f64); NOISE_BANDS]; frames];
    // What the modes play in each band of the fitted channel, every bin.
    let mut played_power = vec![[0.0_f64; NOISE_BANDS]; frames];
    let channels: [(&[f64], &[f64]); 2] = [(mono, &model), (other.unwrap_or(&[]), &model_other)];
    for (frame, (row, played_row)) in missing.iter_mut().zip(played_power.iter_mut()).enumerate() {
        let start = frame * TAIL_HOP;
        for (channel, (signal, rendered)) in channels.iter().enumerate() {
            if signal.is_empty() {
                continue;
            }
            let (heard, played) = (spectrum(signal, start, &mut scratch), spectrum(rendered, start, &mut scratch));
            if channel == 0 {
                for (cell, &(low, high)) in played_row.iter_mut().zip(&ranges) {
                    *cell = (low..=high).filter_map(|k| played.get(k)).sum();
                }
            }
            for (cell, &(low, high)) in row.iter_mut().zip(&ranges) {
                let open: Vec<usize> = (low..=high).filter(|k| !masked.get(*k).copied().unwrap_or(true)).collect();
                if open.len() < TAIL_MIN_OPEN_BINS {
                    continue;
                }
                // The median bin, not the mean: the noise floor between peaks, so a partial the
                // modes miss is not played back as hiss. A single periodogram bin of noise is
                // exponentially distributed, its median ln 2 of its mean.
                let mut differences: Vec<f64> = open
                    .iter()
                    .map(|&k| (heard.get(k).copied().unwrap_or(0.0) - played.get(k).copied().unwrap_or(0.0)).max(0.0))
                    .collect();
                let floor = median(&mut differences) / std::f64::consts::LN_2;
                let power = floor * (high + 1).saturating_sub(low) as f64;
                if channel == 0 { cell.0 = power } else { cell.1 = power }
            }
        }
    }
    if other.is_none() {
        for row in &mut missing {
            for cell in row.iter_mut() {
                cell.1 = cell.0;
            }
        }
    }
    let window_power = window.iter().map(|w| w * w).sum::<f64>();
    let mut out = Missing { power: [(0.0, 0.0); NOISE_BANDS], decay: [0.0; NOISE_BANDS], density: [0.0; NOISE_BANDS] };
    let first =
        (0..frames).find(|f| (f * TAIL_HOP + TAIL_FRAME / 2) as f64 / f64::from(RATE) >= TAIL_START).unwrap_or(frames);
    for band in 0..NOISE_BANDS {
        let series: Vec<(f64, f64, f64)> = missing
            .iter()
            .enumerate()
            .skip(first)
            .filter_map(|(f, row)| {
                row.get(band).map(|c| ((f * TAIL_HOP + TAIL_FRAME / 2) as f64 / f64::from(RATE), c.0, c.1))
            })
            .collect();
        let largest = series.iter().map(|(_, a, b)| a + b).fold(0.0_f64, f64::max);
        if largest <= 0.0 {
            continue;
        }
        // The recording's own floor in this band (its last frames), which a decay must not be
        // fitted into: frames count while they stand 10 dB above it.
        let mut last: Vec<f64> = series.iter().rev().take(TAIL_FLOOR_FRAMES).map(|(_, a, b)| a + b).collect();
        let floor = median(&mut last);
        let points: Vec<(f64, f64)> = series
            .iter()
            .take_while(|(_, a, b)| a + b >= largest * TAIL_RANGE && a + b >= floor * TAIL_ABOVE_FLOOR)
            .filter(|(_, a, b)| a + b > 0.0)
            .map(|(t, a, b)| (*t, (a + b).ln()))
            .collect();
        if points.len() < TAIL_MIN_FRAMES {
            continue;
        }
        // Far under what the modes play in the band, it is their own small error, not a tail.
        let fitted_frames = first..first + points.len();
        let heard: f64 = series.iter().take(points.len()).map(|(_, a, _)| a).sum();
        let rows = played_power.get(fitted_frames).unwrap_or(&[]);
        let modal: f64 = rows.iter().filter_map(|r| r.get(band)).sum();
        // And far under the whole sound, a band with no modes of its own holds only their
        // sidelobes' error.
        let whole: f64 = rows.iter().map(|r| r.iter().sum::<f64>()).sum();
        if heard < TAIL_MIN_SHARE * modal || heard < TAIL_MIN_WHOLE * whole {
            continue;
        }
        let Some((_, slope)) = line(&points) else { continue };
        // Power falls twice as fast as amplitude.
        let decay = -2.0 * 1000.0_f64.ln() / slope;
        let span_seconds = points.last().map_or(0.0, |p| p.0) - points.first().map_or(0.0, |p| p.0);
        let drop_db = -slope * span_seconds * 10.0 / std::f64::consts::LN_10;
        if !(slope < 0.0
            && (f64::from(MIN_TAIL_DECAY)..=f64::from(MAX_TAIL_DECAY)).contains(&decay)
            && drop_db >= TAIL_MIN_DROP_DB)
        {
            continue;
        }
        // Each channel's level at the strike along the common slope.
        let at_strike = |pick: fn(&(f64, f64, f64)) -> f64| {
            let logs: Vec<f64> = series
                .iter()
                .take(points.len())
                .filter(|s| pick(s) > 0.0)
                .map(|s| pick(s).ln() - slope * s.0)
                .collect();
            if logs.is_empty() { 0.0 } else { (logs.iter().sum::<f64>() / logs.len() as f64).exp() }
        };
        let own = at_strike(|s| s.1);
        let other_power = at_strike(|s| s.2);
        let bins = ranges.get(band).map_or(1, |(low, high)| high.saturating_sub(*low) + 1) as f64;
        if let (Some(p), Some(d), Some(n)) =
            (out.power.get_mut(band), out.decay.get_mut(band), out.density.get_mut(band))
        {
            *p = (own, other_power);
            *d = decay;
            // Mean power per bin, as a one-sided density, as the held layer measures its noise.
            *n = 2.0 / (bins * f64::from(RATE) * window_power);
        }
    }
    out.decay.iter().any(|d| *d > 0.0).then_some(out)
}

/// The middle value (the mean of the two middle ones for an even count); 0 when empty.
fn median(values: &mut [f64]) -> f64 {
    values.sort_by(f64::total_cmp);
    let n = values.len();
    match n {
        0 => 0.0,
        _ if n % 2 == 1 => values.get(n / 2).copied().unwrap_or(0.0),
        _ => (values.get(n / 2 - 1).copied().unwrap_or(0.0) + values.get(n / 2).copied().unwrap_or(0.0)) / 2.0,
    }
}

/// `modes` rendered over `length` samples from the strike: the fitted channel and, with `other`,
/// the other channel by their stereo placements. Each part is a rotating, decaying phasor.
fn render_modes(modes: &[Fitted], length: usize, other: bool) -> (Vec<f64>, Vec<f64>) {
    let (mut model, mut model_other) = (vec![0.0; length], vec![0.0; if other { length } else { 0 }]);
    let ln1000 = 1000.0_f64.ln();
    for fitted in modes {
        let (gain, offset) = fitted.other;
        for (hz, t60, amplitude) in fitted.parts() {
            if !(amplitude != 0.0 && t60 > 0.0 && hz < f64::from(RATE) * 0.5) {
                continue;
            }
            let radius = (-ln1000 / (t60 * f64::from(RATE))).exp();
            let (step_sin, step_cos) = (TAU * hz / f64::from(RATE)).sin_cos();
            let (rotate_re, rotate_im) = (radius * step_cos, radius * step_sin);
            let phase = fitted.evidence.phase;
            let (mut re, mut im) = (amplitude * phase.cos(), amplitude * phase.sin());
            let (offset_sin, offset_cos) = offset.sin_cos();
            for (n, slot) in model.iter_mut().enumerate() {
                *slot += im;
                if let Some(o) = model_other.get_mut(n) {
                    *o += gain * (im * offset_cos + re * offset_sin);
                }
                (re, im) = (re * rotate_re - im * rotate_im, re * rotate_im + im * rotate_re);
                if re.abs() + im.abs() < 1e-12 {
                    break;
                }
            }
        }
    }
    (model, model_other)
}

/// Detection bins of the residual's peaks (the detection window less the fitted modes): local
/// maxima within 60 dB of the recording's strongest that stand 6 dB above the median of their
/// neighbours 3..8 bins away in the residual and are not within the candidate separation of a
/// candidate, strongest first, until `total` candidates.
fn residual_peaks(residual: &[f64], found: &[f64], spectrum: &Spectrum<'_>, total: usize) -> Vec<usize> {
    let mut peaks: Vec<(usize, f64)> = (2..residual.len().saturating_sub(9))
        .filter_map(|bin| {
            let (value, before, after) = (*residual.get(bin)?, *residual.get(bin - 1)?, *residual.get(bin + 1)?);
            if value < spectrum.highest * 1e-3 || value <= before || value <= after {
                return None;
            }
            let mut nearby = [0.0_f64; 12];
            for (slot, delta) in nearby.iter_mut().zip([-8_isize, -7, -6, -5, -4, -3, 3, 4, 5, 6, 7, 8]) {
                *slot = bin.checked_add_signed(delta).and_then(|b| residual.get(b)).copied().unwrap_or(value);
            }
            nearby.sort_by(f64::total_cmp);
            (value >= nearby[6] * PARTIAL_PROMINENCE).then_some((bin, value))
        })
        .collect();
    peaks.sort_by(|a, b| b.1.total_cmp(&a.1).then_with(|| a.0.cmp(&b.0)));
    let mut taken: Vec<f64> = found.to_vec();
    let mut bins = Vec::new();
    for (bin, _) in peaks {
        if taken.len() >= total {
            break;
        }
        let hz = bin as f64 * spectrum.bin_hz;
        if taken.iter().all(|f| (f - hz).abs() >= spectrum.separation_hz) {
            taken.push(hz);
            bins.push(bin);
        }
    }
    bins
}

/// The 64 modes with most energy (`magnitude^2 * T60`), in frequency order.
fn most_energetic(modes: &[Fitted]) -> Vec<Fitted> {
    let mut chosen = modes.to_vec();
    chosen.sort_by(|a, b| b.energy().total_cmp(&a.energy()).then_with(|| a.hz.total_cmp(&b.hz)));
    chosen.truncate(MAX_MODEL_MODES);
    chosen.sort_by(|a, b| a.hz.total_cmp(&b.hz));
    chosen
}

/// One note of a chord: its fundamental, the inharmonicity its partials follow and its key.
struct Note {
    f0: f64,
    inharmonicity: f64,
    key: u8,
}

/// The key nearest `hz`.
fn nearest_key(hz: f64) -> u8 {
    (69.0 + 12.0 * (hz / 440.0).log2()).round().clamp(0.0, 127.0) as u8
}

/// Which partial of a note at `f0` with inharmonicity `b` lies within 1 % of `hz`, if any (the
/// nearest of the two around it).
fn partial_number(f0: f64, b: f64, hz: f64) -> Option<usize> {
    partial_match(f0, b, hz).map(|(k, _)| k)
}

/// [`partial_number`] with the relative distance to that partial.
fn partial_match(f0: f64, b: f64, hz: f64) -> Option<(usize, f64)> {
    if !(f0 > 0.0 && hz > 0.0) {
        return None;
    }
    let guess = (hz / f0).round().max(1.0) as usize;
    (guess.saturating_sub(1).max(1)..=guess + 1)
        .filter(|k| *k <= NOTE_PARTIALS)
        .map(|k| {
            let kf = k as f64;
            (k, (hz / (kf * f0 * ((1.0 + b * kf * kf) / (1.0 + b)).sqrt()) - 1.0).abs())
        })
        .filter(|(_, error)| *error <= NOTE_TOLERANCE)
        .min_by(|a, b| a.1.total_cmp(&b.1))
}

/// The notes of a chord among the accepted modes (`Splitting a chord into notes` in the spec);
/// fewer than two when it is one note.
fn chord_notes(modes: &[Fitted]) -> Vec<Note> {
    // What each mode has left once the notes found so far took their share.
    let mut remaining: Vec<f64> = modes.iter().map(Fitted::magnitude).collect();
    let mut notes: Vec<Note> = Vec::new();
    let mut first_score = None;
    while notes.len() < MAX_NOTES {
        let mut best: Option<(f64, f64, f64, f64)> = None;
        for f0 in modes.iter().map(|m| m.hz).filter(|hz| (NOTE_LOW_HZ..=NOTE_HIGH_HZ).contains(hz)) {
            if notes.iter().any(|note| note.key == nearest_key(f0)) {
                continue;
            }
            // A partial counts at most twice the candidate's own fundamental: a weak note an
            // octave or two below cannot claim the chord's strong partials as its own.
            let root = modes.iter().filter(|m| (m.hz / f0 - 1.0).abs() <= NOTE_TOLERANCE).map(Fitted::magnitude);
            let cap = NOTE_SMOOTHNESS * root.fold(0.0_f64, f64::max);
            for b in NOTE_INHARMONICITY {
                let (mut score, mut partials, mut fundamental) = (0.0, 0, false);
                for (mode, left) in modes.iter().zip(&remaining) {
                    if let Some(k) = partial_number(f0, b, mode.hz) {
                        partials += 1;
                        fundamental |= k == 1;
                        score += left.min(cap) / k as f64;
                    }
                }
                if fundamental && partials >= NOTE_MIN_PARTIALS && best.is_none_or(|(s, ..)| score > s) {
                    best = Some((score, f0, b, cap));
                }
            }
        }
        let Some((score, f0, inharmonicity, cap)) = best else { break };
        let first = *first_score.get_or_insert(score);
        if !(score > 0.0 && score >= NOTE_MIN_SHARE * first) {
            break;
        }
        // The note takes what it can explain of each partial, at most its cap; the rest stays
        // for a note that shares the partial (an octave above, say).
        for (mode, left) in modes.iter().zip(remaining.iter_mut()) {
            if partial_number(f0, inharmonicity, mode.hz).is_some() {
                *left -= left.min(cap);
            }
        }
        notes.push(Note { f0, inharmonicity, key: nearest_key(f0) });
    }
    notes.sort_by(|a, b| a.f0.total_cmp(&b.f0));
    notes
}

/// Each mode to the notes that predict it within the tolerance, shared equally when there are
/// several; a mode none predicts to the note whose partial series lies nearest it (the relative
/// distance to its nearest partial plus a small amount per partial number).
fn split_modes(modes: &[Fitted], notes: &[Note]) -> Vec<Vec<Fitted>> {
    let mut parts = vec![Vec::new(); notes.len()];
    for mode in modes {
        // A partial several notes predict (an octave's) is shared out equally with its phase:
        // played together they give it back whole.
        let sharing: Vec<usize> = notes
            .iter()
            .enumerate()
            .filter(|(_, note)| partial_number(note.f0, note.inharmonicity, mode.hz).is_some())
            .map(|(index, _)| index)
            .collect();
        if sharing.len() >= 2 {
            let share = 1.0 / sharing.len() as f64;
            for index in sharing {
                if let Some(part) = parts.get_mut(index) {
                    part.push(Fitted { slow: mode.slow * share, fast: mode.fast * share, ..mode.clone() });
                }
            }
            continue;
        }
        let index = notes
            .iter()
            .enumerate()
            .filter_map(|(index, note)| series_distance(note, mode.hz).map(|d| (index, d)))
            .min_by(|a, b| a.1.total_cmp(&b.1))
            .map_or(0, |(index, _)| index);
        if let Some(part) = parts.get_mut(index) {
            part.push(mode.clone());
        }
    }
    parts
}

/// How far `hz` lies from `note`'s partial series: the relative distance to its nearest partial
/// below the 33rd, plus `NOTE_ORDER_COST` per partial number; none above it.
fn series_distance(note: &Note, hz: f64) -> Option<f64> {
    let (f0, b) = (note.f0, note.inharmonicity);
    (1..=NOTE_PARTIALS)
        .map(|k| {
            let kf = k as f64;
            (hz / (kf * f0 * ((1.0 + b * kf * kf) / (1.0 + b)).sqrt()) - 1.0).abs() + NOTE_ORDER_COST * kf
        })
        .min_by(f64::total_cmp)
        .filter(|_| hz >= f0 * (1.0 - NOTE_TOLERANCE))
        .or_else(|| (hz < f0).then_some((f0 / hz - 1.0) + 1.0))
}

/// How far the candidate at `position` lies from its nearest other candidate that is at most 20 dB
/// weaker in the detection spectrum, in Hz.
fn gap(frequencies: &[f64], position: usize, spectrum: &Spectrum<'_>) -> f64 {
    let hz = frequencies.get(position).copied().unwrap_or(0.0);
    let level = |f: f64| spectrum.values.get((f / spectrum.bin_hz).round() as usize).copied().unwrap_or(0.0);
    // A neighbour more than 20 dB weaker leaks too little into a short window to matter.
    let floor = level(hz) * NEIGHBOUR_SHARE;
    frequencies
        .iter()
        .enumerate()
        .filter(|(other, f)| *other != position && level(**f) >= floor)
        .map(|(_, other)| (other - hz).abs())
        .fold(f64::INFINITY, f64::min)
}

/// What fitting one candidate needs: the prepared channel, the envelope window and how many
/// of them, and the short windows smallest first.
struct Fitter<'a> {
    mono: &'a [f64],
    window: &'a [f64],
    frames: usize,
    short_windows: &'a [Vec<f64>],
    /// How the recording's whole level moves.
    level: NoteLevel,
}

impl Fitter<'_> {
    /// The mode at `hz`, its nearest other candidate `gap` Hz away: envelope windows first, short
    /// windows when they cannot follow it, and a fast first stage the short windows show.
    fn fit<F: FnMut(usize, usize) -> bool>(
        &self,
        hz: f64,
        gap: f64,
        work: &mut Work<F>,
    ) -> Result<Option<Fitted>, Error> {
        let mono = self.mono;
        // The short window's main lobe (two bins each side) stays clear of the nearest other
        // candidate.
        let needed = 4.0 * f64::from(RATE) / gap;
        let short = self.short_windows.iter().find(|w| w.len() as f64 >= needed);
        // A null the long envelope windows would smooth over: the short windows look first.
        let mut found = None;
        if let Some(short) = short.filter(|_| !self.level.held) {
            // Far enough for a null seconds in: up to 6 s.
            let reach = (ANTI_SECONDS * f64::from(RATE)) as usize / short.len();
            let count = (mono.len() / short.len()).min(reach);
            let (measurements, thetas) = measure(mono, short, count, hz, work)?;
            found = anti_phase(hz, &measurements).map(|e| (e, thetas, short.as_slice()));
        }
        let (measurements, thetas) = measure(mono, self.window, self.frames, hz, work)?;
        if found.is_none() {
            found = estimate(hz, &measurements, self.level).map(|e| (e, thetas, self.window));
        }
        if found.is_none()
            && let Some(short) = short
        {
            let count = (mono.len() / short.len()).min(SHORT_FRAMES);
            let (measurements, thetas) = measure(mono, short, count, hz, work)?;
            found = estimate(hz, &measurements, self.level).map(|e| (e, thetas, short.as_slice()));
        }
        let Some((estimate, thetas, used)) = found else { return Ok(None) };
        let slow = estimate.intercept.exp() / window_bias(used, estimate.slope);
        let mut fast_part = estimate.fast.map(|(intercept, slope)| (intercept.exp() / window_bias(used, slope), slope));
        // A fast first stage inside the first envelope windows, seen by the short windows.
        if fast_part.is_none()
            && used.len() == self.window.len()
            && let Some(short) = short
        {
            let count = (mono.len() / short.len()).min(SHORT_FRAMES);
            let (early, _) = measure(mono, short, count, hz, work)?;
            fast_part = early_stage(&early, slow, estimate.slope, short);
        }
        let fast = fast_part.map_or(0.0, |(amplitude, _)| amplitude);
        // An aftersound in opposite phase, at most half the prompt sound at the strike.
        let slow = if estimate.anti { -slow.min(ANTI_MAX_SHARE * fast) } else { slow };
        // Phase at the strike, from the first window of the mode's span.
        let theta = thetas.get(estimate.first_window).copied().unwrap_or(0.0);
        let phase = wrap(theta - TAU * hz * (estimate.first_window * used.len()) as f64 / f64::from(RATE));
        if !(slow.is_finite() && slow != 0.0 && slow + fast > 0.0 && fast.is_finite()) {
            return Ok(None);
        }
        let mut evidence = estimate.evidence;
        evidence.phase = phase;
        if let Some((_, fast_slope)) = fast_part {
            evidence.fast_decay_seconds = -1000.0_f64.ln() / fast_slope;
            evidence.fast_share = fast / (slow + fast);
        }
        Ok(Some(Fitted { hz, slow, fast, evidence, other: (1.0, 0.0) }))
    }
}

/// Detection peaks (bin, magnitude) between `low_hz` and `high_hz`: local maxima within 60 dB of
/// the strongest bin that stand 20 dB above the median of their neighbours 3..8 bins away, or,
/// when none does, those within 30 dB of it that stand 10 dB above that median, or else those
/// within 20 dB of it that stand 6 dB above it.
fn prominent_peaks(spectrum: &[f64], bin_hz: f64, low_hz: f64, high_hz: f64) -> Vec<(usize, f64)> {
    let highest = spectrum.iter().copied().fold(0.0_f64, f64::max);
    let pick = |prominence: f64, range: f64| {
        let mut peaks = Vec::with_capacity(spectrum.len());
        for bin in 2..spectrum.len().saturating_sub(9) {
            let hz = bin as f64 * bin_hz;
            if hz < low_hz || hz > high_hz {
                continue;
            }
            let Some(&value) = spectrum.get(bin) else { continue };
            let (Some(&before), Some(&after)) = (spectrum.get(bin - 1), spectrum.get(bin + 1)) else { continue };
            if value < highest * range || value <= before || value <= after {
                continue;
            }
            let mut nearby = [0.0_f64; 12];
            for (slot, delta) in nearby.iter_mut().zip([-8_isize, -7, -6, -5, -4, -3, 3, 4, 5, 6, 7, 8]) {
                *slot = bin.checked_add_signed(delta).and_then(|b| spectrum.get(b)).copied().unwrap_or(value);
            }
            nearby.sort_by(f64::total_cmp);
            if value >= nearby[6] * prominence {
                peaks.push((bin, value));
            }
        }
        peaks
    };
    // A breathy flute's breath and flutter widen its partials until none stands 20 dB out; a
    // whistle's wandering pitch smears its one partial over a dozen bins.
    [(PEAK_PROMINENCE, 1e-3), (PEAK_PROMINENCE_FALLBACK, PEAK_RANGE_FALLBACK), (PEAK_PROMINENCE_LAST, PEAK_RANGE_LAST)]
        .into_iter()
        .map(|(prominence, range)| pick(prominence, range))
        .find(|peaks| !peaks.is_empty())
        .unwrap_or_default()
}

/// Detection bins of a chord note's partials that are not candidates yet: its series as in
/// [`string_partials`], but each partial takes the local peak nearest its prediction (within
/// 1 %) that is not within the candidate separation of a candidate, so a partial lying close to
/// another note's is found too; until `total` candidates.
fn note_partials(found: &[f64], f0_hz: f64, high_hz: f64, spectrum: &Spectrum<'_>, total: usize) -> Vec<usize> {
    let Some(f0) = found.iter().copied().min_by(|a, b| (a / f0_hz).ln().abs().total_cmp(&(b / f0_hz).ln().abs()))
    else {
        return Vec::new();
    };
    let b = string_inharmonicity(found, f0);
    let values = spectrum.values;
    let mut taken: Vec<f64> = found.to_vec();
    let mut bins = Vec::new();
    for k in 2..=MAX_PARTIALS {
        if taken.len() >= total {
            break;
        }
        let k = k as f64;
        let hz = k * f0 * ((1.0 + b * k * k) / (1.0 + b)).sqrt();
        if !hz.is_finite() || hz > high_hz {
            break;
        }
        let low = (hz * (1.0 - PARTIAL_SEARCH) / spectrum.bin_hz).floor().max(10.0) as usize;
        let high =
            ((hz * (1.0 + PARTIAL_SEARCH) / spectrum.bin_hz).ceil() as usize).min(values.len().saturating_sub(10));
        let free =
            |bin: usize| taken.iter().all(|f| (f - bin as f64 * spectrum.bin_hz).abs() >= spectrum.separation_hz);
        let peak = (low..=high)
            .filter(|&bin| {
                let (Some(&value), Some(&before), Some(&after)) =
                    (values.get(bin), values.get(bin - 1), values.get(bin + 1))
                else {
                    return false;
                };
                value > before && value >= after && value >= spectrum.highest * PARTIAL_FLOOR && free(bin)
            })
            .min_by(|a, b| {
                (*a as f64 * spectrum.bin_hz - hz).abs().total_cmp(&(*b as f64 * spectrum.bin_hz - hz).abs())
            });
        let Some(bin) = peak else { continue };
        let value = values.get(bin).copied().unwrap_or(0.0);
        let mut nearby = [0.0_f64; 12];
        for (slot, delta) in nearby.iter_mut().zip([-8_isize, -7, -6, -5, -4, -3, 3, 4, 5, 6, 7, 8]) {
            *slot = bin.checked_add_signed(delta).and_then(|b| values.get(b)).copied().unwrap_or(value);
        }
        nearby.sort_by(f64::total_cmp);
        if value < nearby[6] * PARTIAL_PROMINENCE {
            continue;
        }
        taken.push(bin as f64 * spectrum.bin_hz);
        bins.push(bin);
    }
    bins
}

/// The stiff-string inharmonicity `B` of the partials of `f1` among `found`: least squares
/// through the origin of `(r/k)^2 - 1 = B (k^2 - (r/k)^2)` over the frequencies within 5 % of a
/// whole multiple 2..32, from at least three, limited to 0..0.01; otherwise 0.
fn string_inharmonicity(found: &[f64], f1: f64) -> f64 {
    let (mut sum_xy, mut sum_xx, mut points) = (0.0, 0.0, 0);
    for f in found {
        let ratio = f / f1;
        let k = ratio.round();
        if (2.0..=32.0).contains(&k) && (ratio / k - 1.0).abs() < 0.05 {
            let stretch = (ratio / k).powi(2);
            let x = k * k - stretch;
            sum_xy += (stretch - 1.0) * x;
            sum_xx += x * x;
            points += 1;
        }
    }
    if points >= 3 && sum_xx > 0.0 { (sum_xy / sum_xx).clamp(0.0, MAX_INHARMONICITY) } else { 0.0 }
}

/// What preparing one channel of the analyzed input needs.
struct Prepare<'a> {
    input: &'a [f32],
    channels: usize,
    sample_rate: u32,
    length: usize,
    scale: f64,
}

impl Prepare<'_> {
    /// One channel without its mean, divided by the scale and resampled to `RATE`.
    fn channel<F: FnMut(usize, usize) -> bool>(
        &self,
        channel: usize,
        mean: f64,
        work: &mut Work<F>,
    ) -> Result<Vec<f64>, Error> {
        let rate = f64::from(RATE) / f64::from(self.sample_rate);
        let mut resampler = Resampler::new(f64::from(self.sample_rate), f64::from(RATE));
        let mut output = vec![0.0; (CHUNK as f64 * rate).ceil() as usize + 1024];
        let mut prepared = Vec::with_capacity((self.length as f64 * rate).ceil() as usize + 1024);
        let mut block = [0.0; CHUNK];
        for chunk in self.input.chunks(CHUNK * self.channels) {
            work.step()?;
            let frames = chunk.len() / self.channels;
            for (frame, value) in chunk.chunks_exact(self.channels).zip(block.iter_mut()) {
                *value = (f64::from(frame.get(channel).copied().unwrap_or(0.0)) - mean) / self.scale;
            }
            let count = resampler.process(block.get(..frames).ok_or(Error::InvalidRange)?, &mut output);
            prepared.extend(output.iter().take(count).copied());
        }
        let count = resampler.flush(&mut output);
        prepared.extend(output.iter().take(count).copied());
        // Output frame j is the source at time j / RATE (the resampler's latency is only in when
        // frames come out), so time zero, every measured phase and the attack start at the
        // source's first analyzed frame.
        prepared.truncate((self.length as f64 * rate).floor() as usize);
        Ok(prepared)
    }
}

/// `hz` in `count` consecutive windows of `window`'s size from the start: the windows where it
/// is above the floor as (window index, center time, ln amplitude), and every window's phase.
type Measured = (Vec<(usize, f64, f64)>, Vec<f64>);
fn measure<F: FnMut(usize, usize) -> bool>(
    mono: &[f64],
    window: &[f64],
    count: usize,
    hz: f64,
    work: &mut Work<F>,
) -> Result<Measured, Error> {
    let size = window.len();
    let mut measurements = Vec::with_capacity(count);
    let mut thetas = Vec::with_capacity(count);
    for index in 0..count {
        work.step()?;
        let start = index * size;
        let frame = mono.get(start..start + size).ok_or(Error::InvalidRange)?;
        let (amplitude, theta) = projection_complex(frame, window, hz);
        thetas.push(theta);
        if amplitude > 1e-7 {
            measurements.push((index, (start + size / 2) as f64 / f64::from(RATE), amplitude.ln()));
        }
    }
    Ok((measurements, thetas))
}

/// How much a window over a decay with `slope` (per second) under- or overstates the level at
/// its center.
fn window_bias(window: &[f64], slope: f64) -> f64 {
    let center = window.len() as f64 / 2.0;
    window.iter().enumerate().map(|(i, w)| w * (slope * (i as f64 - center) / f64::from(RATE)).exp()).sum::<f64>()
        / window.iter().sum::<f64>()
}

fn projection(frame: &[f64], window: &[f64], hz: f64) -> f64 {
    projection_complex(frame, window, hz).0
}

/// Amplitude of a sine at `hz` in the windowed frame, and its phase at the frame's first sample:
/// for `A sin(omega n + theta)` the sums are `A sin(theta) / 2` and `A cos(theta) / 2` times the
/// window's sum.
fn projection_complex(frame: &[f64], window: &[f64], hz: f64) -> (f64, f64) {
    let (sin, cos) = (TAU * hz / f64::from(RATE)).sin_cos();
    let (mut real, mut imaginary, mut re, mut im, mut sum) = (1.0, 0.0, 0.0, 0.0, 0.0);
    for (&x, &w) in frame.iter().zip(window) {
        re += x * w * real;
        im += x * w * imaginary;
        sum += w;
        (real, imaginary) = (real * cos - imaginary * sin, real * sin + imaginary * cos);
    }
    (2.0 * re.hypot(im) / sum.max(1e-12), re.atan2(im))
}

fn refine<F: FnMut(usize, usize) -> bool>(
    frame: &[f64],
    window: &[f64],
    mut left: f64,
    mut right: f64,
    work: &mut Work<F>,
) -> Result<f64, Error> {
    const RATIO: f64 = 0.6180339887498949;
    let mut a = right - RATIO * (right - left);
    let mut b = left + RATIO * (right - left);
    let mut va = projection(frame, window, a);
    let mut vb = projection(frame, window, b);
    for _ in 0..REFINEMENTS {
        work.step()?;
        if va > vb {
            right = b;
            b = a;
            vb = va;
            a = right - RATIO * (right - left);
            va = projection(frame, window, a);
        } else {
            left = a;
            a = b;
            va = vb;
            b = left + RATIO * (right - left);
            vb = projection(frame, window, b);
        }
    }
    Ok((left + right) * 0.5)
}

fn hann(size: usize) -> Vec<f64> {
    (0..size).map(|i| 0.5 - 0.5 * (TAU * i as f64 / size as f64).cos()).collect()
}

/// The decay of one mode: its slow part, an optional faster first part and where its span starts.
struct Estimate {
    intercept: f64,
    slope: f64,
    evidence: ModeEvidence,
    /// Log-amplitude intercept and slope of a faster first part.
    fast: Option<(f64, f64)>,
    /// Window index where the mode's span starts.
    first_window: usize,
    /// The slow part (an aftersound) starts in opposite phase to the fast part.
    anti: bool,
}

/// Decay of one mode from its (window, time, log amplitude) measurements: fitted on its own
/// span, once more on the span's last two thirds when a long span fails, and with a faster
/// first part when that describes the span clearly better.
fn estimate(hz: f64, values: &[(usize, f64, f64)], level: NoteLevel) -> Option<Estimate> {
    let held_note = level.held;
    let span = envelope_span(values);
    let first_window = span.first()?.0;
    // In a held note, a partial that holds is held, however uneven.
    if held_note && let Some((intercept, slope, evidence)) = held_span(hz, values, 0.0) {
        let first_window = values.iter().find(|v| v.1 >= HELD_FROM_SECONDS)?.0;
        return Some(Estimate { intercept, slope, evidence, fast: None, first_window, anti: false });
    }
    // A null the partial comes back from: prompt sound and aftersound in opposite phase.
    if !held_note && let Some(anti) = anti_phase(hz, values) {
        return Some(anti);
    }
    let single = fit_span(hz, span);
    // The late retry describes what rings on after a fast early decay; it never makes a mode
    // sustained (beating can flatten a decaying partial's late windows).
    let late = if span.len() >= LATE_RETRY_WINDOWS {
        span.get(span.len() / 3..).and_then(|l| fit_span(hz, l)).filter(|(_, _, e)| !e.sustained)
    } else {
        None
    };
    let plain = |(intercept, slope, evidence): (f64, f64, ModeEvidence)| Estimate {
        intercept,
        slope,
        evidence,
        fast: None,
        first_window,
        anti: false,
    };
    let two_stage = late.and_then(|(intercept, slope, evidence)| {
        let early = span.get(..span.len() / 3)?;
        let excess: Vec<(f64, f64)> = early
            .iter()
            .filter_map(|&(_, t, y)| {
                let remainder = y.exp() - (intercept + slope * t).exp();
                (remainder > 0.0).then(|| (t, remainder.ln()))
            })
            .collect();
        if excess.len() < 2 {
            return None;
        }
        let (fast_intercept, fast_slope) = line(&excess)?;
        if fast_slope >= 0.0 {
            return None;
        }
        let fast_decay = -1000.0_f64.ln() / fast_slope;
        if !(0.001..evidence.decay_seconds * 0.5).contains(&fast_decay) {
            return None;
        }
        let rms = span_rms(span, |t| ((intercept + slope * t).exp() + (fast_intercept + fast_slope * t).exp()).ln());
        Some((
            Estimate {
                intercept,
                slope,
                evidence,
                fast: Some((fast_intercept, fast_slope)),
                first_window,
                anti: false,
            },
            rms,
        ))
    });
    match (single, two_stage) {
        (Some(one), Some((two, rms))) => {
            let single_rms = span_rms(span, |t| one.0 + one.1 * t);
            let better = rms + TWO_STAGE_GAIN_DB <= single_rms
                || (single_rms >= TWO_STAGE_MIN_SINGLE_DB && rms <= single_rms * TWO_STAGE_RATIO);
            Some(if better { two } else { plain(one) })
        }
        (Some(one), None) => Some(plain(one)),
        (None, Some((two, _))) => Some(two),
        // Last, a partial at an uneven level that never clearly falls follows the note's trend
        // (a fading wind note's partial), rather than being dropped.
        (None, None) => late
            .or_else(|| loose_span(hz, values))
            .or_else(|| fallback_span(hz, values))
            .or_else(|| {
                // Only a partial that matters: within `TREND_MIN_DB` of the note's loudest level
                // (hum and rumble under a piano note are no part of the note to sustain).
                let loudest = values.iter().map(|v| v.2).fold(f64::NEG_INFINITY, f64::max);
                let matters = (level.loudest - loudest) * DB <= TREND_MIN_DB;
                held_span(hz, values, level.trend).filter(|_| !held_note && matters)
            })
            .map(plain),
    }
}

/// A partial no fit describes, kept loose: the least-squares decay from its loudest of the first
/// three windows until the floor, ignoring rises, when it clearly falls.
fn loose_span(hz: f64, values: &[(usize, f64, f64)]) -> Option<(f64, f64, ModeEvidence)> {
    let head = values.get(..values.len().min(3))?;
    let start = head.iter().enumerate().max_by(|a, b| a.1.2.total_cmp(&b.1.2))?.0;
    let peak = values.get(start)?.2;
    let span: Vec<(f64, f64)> =
        values.get(start..)?.iter().take_while(|v| (peak - v.2) * DB <= SPAN_FLOOR_DB).map(|v| (v.1, v.2)).collect();
    if span.len() < LOOSE_MIN_WINDOWS {
        return None;
    }
    let (intercept, slope) = line(&span)?;
    let decay = -1000.0_f64.ln() / slope;
    let drop_db = -slope * (span.last()?.0 - span.first()?.0) * DB;
    let rms_db =
        (span.iter().map(|(t, y)| ((y - intercept - slope * t) * DB).powi(2)).sum::<f64>() / span.len() as f64).sqrt();
    let third = span.len() / 3;
    let mean = |part: &[(f64, f64)]| part.iter().map(|p| p.1).sum::<f64>() / part.len().max(1) as f64;
    let (first, middle, last) =
        (span.get(..third)?, span.get(third..span.len() - third)?, span.get(span.len() - third..)?);
    let falling =
        (mean(first) - mean(middle)) * DB >= LOOSE_THIRD_DB && (mean(middle) - mean(last)) * DB >= LOOSE_THIRD_DB;
    let clear = slope < 0.0
        && falling
        && (0.001..=SUSTAINED_SECONDS).contains(&decay)
        && drop_db >= LOOSE_DROP_DB
        && rms_db <= LOOSE_MAX_DB;
    clear.then_some((
        intercept,
        slope,
        ModeEvidence {
            frequency_hz: hz,
            decay_seconds: decay,
            estimation_rms_db: rms_db,
            validation_rms_db: rms_db,
            estimation_windows: span.len(),
            validation_windows: 0,
            sustained: false,
            fast_decay_seconds: 0.0,
            fast_share: 0.0,
            beat_hz: 0.0,
            beat_share: 0.0,
            phase: 0.0,
            loose: true,
        },
    ))
}

/// The glide into the note, from a dense pitch track ([`notes::track`]) with its octave errors
/// folded, and every frame moved by octaves to within half an octave of the pitch the note
/// settles on (the median of the last half of its voiced frames): None when the first voiced
/// frames lie within `GLIDE_SEMITONES` of that pitch, when the note's first partials already
/// sound before the glide ends (another note, not a glide), when the note does not hold
/// its pitch for at least its last quarter, or when the glide lasts more than four seconds.
/// Its `GLIDE_POINTS` points are evenly spaced from the strike until the smoothed pitch (each
/// frame the median of itself and its neighbours) last lies `GLIDE_SETTLED_SEMITONES` from the
/// note, the last at 0 cents.
fn note_glide<F: FnMut(usize, usize) -> bool>(
    mono: &[f64],
    sample_rate: u32,
    work: &mut Work<F>,
) -> Result<Option<Glide>, Error> {
    let pcm: Vec<f32> = mono.iter().map(|x| *x as f32).collect();
    let mut frames = notes::track(&pcm, sample_rate, |advance| {
        if advance { work.step().is_ok() } else { (work.progress)(work.done, work.total) }
    })?;
    notes::fold_octaves(&mut frames);
    let half = notes::NOTE_FRAME as f64 / 2.0;
    let voiced: Vec<(f64, f64)> = frames
        .iter()
        .filter_map(|f| f.midi.map(|m| ((f.start as f64 + half) / f64::from(RATE), f64::from(m))))
        .collect();
    if voiced.len() < GLIDE_MIN_FRAMES {
        return Ok(None);
    }
    let pitch_median = |part: &[(f64, f64)]| {
        let mut values: Vec<f64> = part.iter().map(|p| p.1).collect();
        median(&mut values)
    };
    let settled = pitch_median(voiced.get(voiced.len() / 2..).unwrap_or_default());
    // A glide moves less than half an octave: a frame farther from the note is YIN's octave
    // error, folded onto the octave nearest the note.
    let voiced: Vec<(f64, f64)> = voiced.iter().map(|&(t, m)| (t, m + 12.0 * ((settled - m) / 12.0).round())).collect();
    let first = pitch_median(voiced.get(..3).unwrap_or_default());
    if (first - settled).abs() < GLIDE_SEMITONES {
        return Ok(None);
    }
    let smooth: Vec<(f64, f64)> = (0..voiced.len())
        .map(|i| {
            let part = voiced.get(i.saturating_sub(1)..(i + 2).min(voiced.len())).unwrap_or_default();
            (voiced.get(i).map_or(0.0, |p| p.0), pitch_median(part))
        })
        .collect();
    let Some(last_off) = smooth.iter().rposition(|p| (p.1 - settled).abs() > GLIDE_SETTLED_SEMITONES) else {
        return Ok(None);
    };
    let end_index = last_off + 1;
    if end_index * 4 > smooth.len() * 3 {
        return Ok(None);
    }
    let Some(&(end, _)) = smooth.get(end_index) else {
        return Ok(None);
    };
    if !(GLIDE_MIN_SECONDS..=f64::from(MAX_GLIDE_SECONDS)).contains(&end) {
        return Ok(None);
    }
    // A glide moves into its note: the note's own partials are quiet while the pitch is still
    // away from it. When they already sound (another note over the held one, a hammer-on above
    // it), it is no glide. Four partials from the lowest that a semitone moves out of the
    // window's main lobe (2.5 bins), at the frames at least a semitone from the note, against the
    // frames after it.
    let note_hz = 440.0 * 2.0_f64.powf((settled - 69.0) / 12.0);
    let bin_hz = f64::from(RATE) / notes::NOTE_FRAME as f64;
    let lowest = ((2.5 * bin_hz) / ((2.0_f64.powf(1.0 / 12.0) - 1.0) * note_hz)).ceil().max(1.0) as usize;
    let window = hann(notes::NOTE_FRAME);
    let power = |centers: &mut dyn Iterator<Item = f64>| {
        let (mut sum, mut count) = (0.0, 0usize);
        for center in centers {
            let at = (center * f64::from(RATE)) as usize;
            let Some(frame) = at.checked_sub(notes::NOTE_FRAME / 2).and_then(|s| mono.get(s..s + notes::NOTE_FRAME))
            else {
                continue;
            };
            sum += (lowest..lowest + GLIDE_CHECK_PARTIALS)
                .map(|k| sustain::projection(frame, &window, note_hz * k as f64).0.powi(2))
                .sum::<f64>();
            count += 1;
        }
        if count == 0 { None } else { Some(sum / count as f64) }
    };
    let away = power(&mut voiced.iter().filter(|p| p.0 < end && (p.1 - settled).abs() >= GLIDE_SEMITONES).map(|p| p.0));
    let held = power(&mut voiced.iter().filter(|p| p.0 > end).map(|p| p.0));
    let (Some(away), Some(held)) = (away, held) else {
        return Ok(None);
    };
    if away > held * GLIDE_QUIET {
        return Ok(None);
    }
    let curve: Vec<(f64, f64)> =
        smooth.get(..=end_index).unwrap_or_default().iter().map(|&(t, m)| (t, (m - settled) * 100.0)).collect();
    let last = GLIDE_POINTS - 1;
    let points: Vec<(f32, f32)> = (0..GLIDE_POINTS)
        .map(|k| {
            let t = end * k as f64 / last as f64;
            let cents = if k == last { 0.0 } else { sustain::interpolate(&curve, t) };
            (t as f32, cents.clamp(-f64::from(MAX_GLIDE_CENTS), f64::from(MAX_GLIDE_CENTS)) as f32)
        })
        .collect();
    let glide = Glide::from_points(&points);
    Ok(glide.validate().is_ok().then_some(glide))
}

/// Whether a recording's whole level holds after its attack (a release may follow), by the
/// rule of [`held_span`] on the RMS level of `HELD_LEVEL_FRAME`-sample windows over the analyzed
/// span (finer than the envelope windows, so a short held note still has enough of them).
fn held_level(mono: &[f64], frame_size: usize, frames: usize) -> NoteLevel {
    let length = (frame_size * frames).min(mono.len());
    let frame_size = HELD_LEVEL_FRAME;
    let levels: Vec<(usize, f64, f64)> = mono
        .get(..length)
        .unwrap_or_default()
        .chunks_exact(frame_size)
        .enumerate()
        .map(|(index, frame)| {
            let power = frame.iter().map(|x| x * x).sum::<f64>() / frame_size as f64;
            let t = (index as f64 + 0.5) * frame_size as f64 / f64::from(RATE);
            (index, t, 0.5 * power.max(1e-300).ln())
        })
        .collect();
    let held = held_span(0.0, &levels, 0.0).is_some();
    // The trend: the least-squares slope of the level after the attack, while within
    // `TREND_FLOOR_DB` of the loudest window (a swell has none).
    let loudest = levels.iter().map(|v| v.2).fold(f64::NEG_INFINITY, f64::max);
    let points: Vec<(f64, f64)> = levels
        .iter()
        .filter(|v| v.1 >= HELD_FROM_SECONDS && (loudest - v.2) * DB <= TREND_FLOOR_DB)
        .map(|v| (v.1, v.2))
        .collect();
    let trend =
        if points.len() >= HELD_MIN_WINDOWS { line(&points).map_or(0.0, |(_, slope)| slope.min(0.0)) } else { 0.0 };
    NoteLevel { held, trend: if held { 0.0 } else { trend }, loudest }
}

/// How a recording's whole level moves after its attack.
#[derive(Clone, Copy, Debug, PartialEq)]
struct NoteLevel {
    /// The level holds (a release may follow): partials are tried as held first.
    held: bool,
    /// Otherwise the level's slope (log amplitude per second, at most 0), which a partial at an
    /// uneven level that no fit describes follows.
    trend: f64,
    /// The loudest window's RMS level (log amplitude).
    loudest: f64,
}

/// A partial held at an uneven level (a bowed, blown or sung partial moving with the bow, the
/// breath or a beat, more than a decay's error limits allow) that never clearly falls while the
/// note is held: kept sustained at its mean amplitude over the held part. From the first window
/// centered at least `HELD_FROM_SECONDS` in (after the attack), the held part runs to the last window at most `HELD_RELEASE_DB`
/// below the median of the span's first half (what follows is the note's release, which the
/// held layer measures); in it, at most a quarter of the windows lie `HELD_DIP_DB` below their
/// median (a bow change or a beat's null), and the median level of its last third stands within
/// `SUSTAIN_DB` of its first third's (the held layer gives a mode its rise before that).
/// Medians, since a few dips pull a mean down as a decay would.
fn held_span(hz: f64, values: &[(usize, f64, f64)], trend: f64) -> Option<(f64, f64, ModeEvidence)> {
    // After the attack: a bow's or a breath's accent is not the held level. Levels are judged
    // against the note's own trend (none in a held note).
    let start = values.iter().position(|v| v.1 >= HELD_FROM_SECONDS)?;
    let trend = trend.min(0.0);
    let detrended: Vec<(usize, f64, f64)> =
        values.get(start..)?.iter().map(|&(i, t, y)| (i, t, y - trend * t)).collect();
    let whole = detrended.as_slice();
    let level_median = |part: &[(usize, f64, f64)]| {
        let mut levels: Vec<f64> = part.iter().map(|p| p.2).collect();
        median(&mut levels)
    };
    let opening = level_median(whole.get(..whole.len().div_ceil(2))?);
    let end = whole.iter().rposition(|v| (opening - v.2) * DB <= HELD_RELEASE_DB)?;
    let span = whole.get(..=end)?;
    if span.len() < HELD_MIN_WINDOWS {
        return None;
    }
    let middle = level_median(span);
    let dips = span.iter().filter(|v| (middle - v.2) * DB > HELD_DIP_DB).count();
    if dips as f64 > HELD_MAX_DIPS * span.len() as f64 {
        return None;
    }
    let third = span.len() / 3;
    let fall_db = (level_median(span.get(..third)?) - level_median(span.get(span.len() - third..)?)) * DB;
    // Held: neither falling nor swelling by more than SUSTAIN_DB (a rising envelope is no held
    // tone and no resonance).
    if fall_db.abs() > SUSTAIN_DB {
        return None;
    }
    let count = span.len() as f64;
    let level = (span.iter().map(|v| v.2.exp()).sum::<f64>() / count).ln();
    let mean_t = span.iter().map(|v| v.1).sum::<f64>() / count;
    let longest = -1000.0_f64.ln() / SUSTAINED_SECONDS;
    let slope = trend.min(longest);
    let rms_db = (span.iter().map(|v| ((v.2 - level) * DB).powi(2)).sum::<f64>() / count).sqrt();
    let decay = -1000.0_f64.ln() / slope;
    Some((
        level + (trend - slope) * mean_t,
        slope,
        ModeEvidence {
            frequency_hz: hz,
            decay_seconds: decay,
            estimation_rms_db: rms_db,
            validation_rms_db: rms_db,
            estimation_windows: span.len(),
            validation_windows: 0,
            sustained: decay >= SUSTAINED_SECONDS,
            fast_decay_seconds: 0.0,
            fast_share: 0.0,
            beat_hz: 0.0,
            beat_share: 0.0,
            phase: 0.0,
            // Held in a held note; following a fading note's trend, no validated fit.
            loose: trend < 0.0,
        },
    ))
}

/// A fast first stage the envelope windows averaged away, from short-window measurements:
/// the excess over the slow decay (true amplitude `slow`, `slope` per second, as `window` would
/// measure it), from the loudest of the first four windows while it is at least 1 dB, fitted as
/// a second exponential. Kept when it covers at least three windows, decays faster than half the
/// slow T60 (at least 1 ms) and lowers those windows' RMS error by at least 1 dB. Returns its
/// true amplitude and slope.
fn early_stage(early: &[(usize, f64, f64)], slow: f64, slope: f64, window: &[f64]) -> Option<(f64, f64)> {
    let predicted = |t: f64| slow * window_bias(window, slope) * (slope * t).exp();
    let head = early.get(..early.len().min(4))?;
    let start = head.iter().enumerate().max_by(|a, b| a.1.2.total_cmp(&b.1.2))?.0;
    let mut excess = Vec::new();
    let mut covered = Vec::new();
    for &(_, t, y) in early.get(start..)? {
        let (measured, model) = (y.exp(), predicted(t));
        if !(model > 0.0 && (measured / model).ln() * DB >= EARLY_EXCESS_DB) {
            break;
        }
        excess.push((t, (measured - model).ln()));
        covered.push((t, y));
    }
    if excess.len() < EARLY_MIN_WINDOWS {
        return None;
    }
    let (intercept, fast_slope) = line(&excess)?;
    let fast_decay = -1000.0_f64.ln() / fast_slope;
    if !(fast_slope < 0.0 && fast_slope < 2.0 * slope && fast_decay >= 0.001) {
        return None;
    }
    let rms = |model: &dyn Fn(f64) -> f64| {
        (covered.iter().map(|(t, y)| ((y - model(*t).ln()) * DB).powi(2)).sum::<f64>() / covered.len() as f64).sqrt()
    };
    let single = rms(&|t| predicted(t));
    let two = rms(&|t| predicted(t) + (intercept + fast_slope * t).exp());
    (two + TWO_STAGE_GAIN_DB <= single).then(|| (intercept.exp() / window_bias(window, fast_slope), fast_slope))
}

/// A prompt sound and an aftersound in opposite phase (`Prompt sound and aftersound` in the
/// spec): a null at least 10 dB under the head's peak that a later window stands 6 dB above, a
/// smooth aftersound after it and a faster prompt sound before it whose difference describes the
/// windows within 6 dB RMS. The estimate's line is the aftersound, its fast part the prompt sound.
fn anti_phase(hz: f64, values: &[(usize, f64, f64)]) -> Option<Estimate> {
    let head = values.get(..values.len().min(3))?;
    let start = head.iter().enumerate().max_by(|a, b| a.1.2.total_cmp(&b.1.2))?.0;
    let peak = values.get(start)?.2;
    let rest = values.get(start..)?;
    // The first null: a local minimum (over two windows each side) deep enough under the peak
    // that a later window stands well above (a beat's later nulls come after the aftersound).
    let null_index = rest.iter().enumerate().position(|(index, value)| {
        let later = rest.get(index + 1..).unwrap_or_default().iter().map(|v| v.2).fold(f64::NEG_INFINITY, f64::max);
        let around = rest.get(index.saturating_sub(2)..(index + 3).min(rest.len())).unwrap_or_default();
        (later - value.2) * DB >= RERISE_DB
            && (peak - value.2) * DB >= ANTI_NULL_DB
            && around.iter().all(|v| v.2 >= value.2)
    })?;
    let after = rest.get(null_index + 1..)?;
    // The aftersound up to the next null: from its loudest window while within 20 dB of it.
    let recovery = after.iter().enumerate().max_by(|a, b| a.1.2.total_cmp(&b.1.2))?.0;
    let top = after.get(recovery)?.2;
    let late: Vec<(f64, f64)> = after
        .get(recovery..)?
        .iter()
        .take_while(|v| (peak - v.2) * DB <= SPAN_FLOOR_DB && (top - v.2) * DB <= ANTI_LATE_DB)
        .map(|v| (v.1, v.2))
        .collect();
    if late.len() < 3 {
        return None;
    }
    let (slow_intercept, fitted_slope) = line(&late)?;
    let smooth = (late.iter().map(|(t, y)| ((y - slow_intercept - fitted_slope * t) * DB).powi(2)).sum::<f64>()
        / late.len() as f64)
        .sqrt();
    if fitted_slope > 0.0 || smooth > ANTI_SMOOTH_DB {
        return None;
    }
    // The longest T60 a mode keeps: a flatter aftersound rings that long.
    let slope = fitted_slope.min(-1000.0_f64.ln() / SUSTAINED_SECONDS);
    let slow_intercept = slow_intercept + (fitted_slope - slope) * late.first().map_or(0.0, |p| p.0);
    let early: Vec<(f64, f64)> = rest
        .get(..null_index)?
        .iter()
        .filter_map(|v| {
            let (measured, after) = (v.2.exp(), (slow_intercept + slope * v.1).exp());
            (measured >= ANTI_EARLY * after).then(|| (v.1, (measured + after).ln()))
        })
        .collect();
    if early.len() < 3 {
        return None;
    }
    let (fast_intercept, fast_slope) = line(&early)?;
    let slow_decay = -1000.0_f64.ln() / slope;
    let fast_decay = -1000.0_f64.ln() / fast_slope;
    if !(fast_slope < slope && fast_slope < 0.0 && (0.001..slow_decay * 0.5).contains(&fast_decay)) {
        return None;
    }
    let floor = peak - SPAN_FLOOR_DB / DB;
    // The null itself and its neighbours are left out: the pair's exact zero there and the
    // measured floor always differ.
    let covered: Vec<&(usize, f64, f64)> = rest
        .iter()
        .take(null_index + 1 + recovery + late.len())
        .enumerate()
        .filter(|(index, _)| index.abs_diff(null_index) > 1)
        .map(|(_, v)| v)
        .collect();
    let rms = (covered
        .iter()
        .map(|v| {
            let model = ((fast_intercept + fast_slope * v.1).exp() - (slow_intercept + slope * v.1).exp()).abs();
            let model = model.max(1e-300).ln().max(floor);
            ((v.2.max(floor) - model) * DB).powi(2)
        })
        .sum::<f64>()
        / covered.len().max(1) as f64)
        .sqrt();
    if rms > ANTI_MAX_DB {
        return None;
    }
    Some(Estimate {
        intercept: slow_intercept,
        slope,
        evidence: ModeEvidence {
            frequency_hz: hz,
            decay_seconds: slow_decay,
            estimation_rms_db: rms,
            validation_rms_db: rms,
            estimation_windows: covered.len(),
            validation_windows: 0,
            sustained: false,
            fast_decay_seconds: 0.0,
            fast_share: 0.0,
            beat_hz: 0.0,
            beat_share: 0.0,
            phase: 0.0,
            loose: true,
        },
        fast: Some((fast_intercept, fast_slope)),
        first_window: values.get(start)?.0,
        anti: true,
    })
}

/// A last resort for a partial that clearly fades but whose envelope no fit describes (a deep,
/// uneven beat, energy moving between strings): from its loudest of the first third of the
/// windows to the floor, the least-squares decay, kept loose when it falls at least 3 dB from the
/// first third to the last and needs no error limit.
fn fallback_span(hz: f64, values: &[(usize, f64, f64)]) -> Option<(f64, f64, ModeEvidence)> {
    let third = (values.len() / 3).max(1);
    let head = values.get(..third)?;
    let start = head.iter().enumerate().max_by(|a, b| a.1.2.total_cmp(&b.1.2))?.0;
    let peak = values.get(start)?.2;
    let span: Vec<(f64, f64)> =
        values.get(start..)?.iter().take_while(|v| (peak - v.2) * DB <= SPAN_FLOOR_DB).map(|v| (v.1, v.2)).collect();
    if span.len() < FALLBACK_MIN_WINDOWS {
        return None;
    }
    let mean = |part: &[(f64, f64)]| part.iter().map(|p| p.1).sum::<f64>() / part.len().max(1) as f64;
    let part = (span.len() / 3).max(1);
    let fall_db = (mean(span.get(..part)?) - mean(span.get(span.len() - part..)?)) * DB;
    let (intercept, slope) = line(&span)?;
    let decay = -1000.0_f64.ln() / slope;
    if !(slope < 0.0 && fall_db >= FALLBACK_DROP_DB && (0.001..=SUSTAINED_SECONDS).contains(&decay)) {
        return None;
    }
    let rms_db =
        (span.iter().map(|(t, y)| ((y - intercept - slope * t) * DB).powi(2)).sum::<f64>() / span.len() as f64).sqrt();
    Some((
        intercept,
        slope,
        ModeEvidence {
            frequency_hz: hz,
            decay_seconds: decay,
            estimation_rms_db: rms_db,
            validation_rms_db: rms_db,
            estimation_windows: span.len(),
            validation_windows: 0,
            sustained: false,
            fast_decay_seconds: 0.0,
            fast_share: 0.0,
            beat_hz: 0.0,
            beat_share: 0.0,
            phase: 0.0,
            loose: true,
        },
    ))
}

/// Least-squares intercept and slope through (time, value) points.
fn line(points: &[(f64, f64)]) -> Option<(f64, f64)> {
    let n = points.len() as f64;
    let mean_t = points.iter().map(|p| p.0).sum::<f64>() / n;
    let mean_y = points.iter().map(|p| p.1).sum::<f64>() / n;
    let variance = points.iter().map(|p| (p.0 - mean_t).powi(2)).sum::<f64>();
    if variance <= 0.0 {
        return None;
    }
    let slope = points.iter().map(|p| (p.0 - mean_t) * (p.1 - mean_y)).sum::<f64>() / variance;
    slope.is_finite().then_some((mean_y - slope * mean_t, slope))
}

/// RMS difference in dB between a span's log amplitudes and a log-amplitude model.
fn span_rms(span: &[(usize, f64, f64)], model: impl Fn(f64) -> f64) -> f64 {
    (span.iter().map(|p| ((p.2 - model(p.1)) * DB).powi(2)).sum::<f64>() / span.len().max(1) as f64).sqrt()
}

/// From the loudest of the first three windows (an attack is not a decay) until the level is
/// more than `SPAN_FLOOR_DB` below that peak or `RERISE_DB` above its lowest so far.
fn envelope_span(values: &[(usize, f64, f64)]) -> &[(usize, f64, f64)] {
    let head = values.get(..values.len().min(3)).unwrap_or_default();
    let mut start = None;
    for (index, value) in head.iter().enumerate() {
        if start.is_none_or(|(_, peak)| value.2 > peak) {
            start = Some((index, value.2));
        }
    }
    let Some((start, peak)) = start else { return &[] };
    let mut low = peak;
    let mut end = start;
    for value in values.iter().skip(start) {
        if (peak - value.2) * DB > SPAN_FLOOR_DB || (value.2 - low) * DB > RERISE_DB {
            break;
        }
        low = low.min(value.2);
        end += 1;
    }
    values.get(start..end).unwrap_or_default()
}

fn fit_span(hz: f64, span: &[(usize, f64, f64)]) -> Option<(f64, f64, ModeEvidence)> {
    let training: Vec<_> = span.iter().step_by(2).collect();
    let validation: Vec<_> = span.iter().skip(1).step_by(2).collect();
    if training.len() < 3 || validation.len() < 3 {
        return None;
    }
    let count = training.len() as f64;
    let mean_t = training.iter().map(|v| v.1).sum::<f64>() / count;
    let mean_y = training.iter().map(|v| v.2).sum::<f64>() / count;
    let variance = training.iter().map(|v| (v.1 - mean_t).powi(2)).sum::<f64>();
    if variance <= 0.0 {
        return None;
    }
    let slope = training.iter().map(|v| (v.1 - mean_t) * (v.2 - mean_y)).sum::<f64>() / variance;
    if !slope.is_finite() {
        return None;
    }
    let intercept = mean_y - slope * mean_t;
    let rms = |v: &[&(usize, f64, f64)]| {
        (v.iter().map(|p| ((p.2 - intercept - slope * p.1) * DB).powi(2)).sum::<f64>() / v.len() as f64).sqrt()
    };
    let estimation_rms_db = rms(&training);
    let validation_rms_db = rms(&validation);
    if estimation_rms_db > MAX_ESTIMATION_DB || validation_rms_db > MAX_VALIDATION_DB {
        return None;
    }
    let drop_db = -slope * (training.last()?.1 - training.first()?.1) * DB;
    let decay = -1000.0_f64.ln() / slope;
    let decaying = slope < 0.0
        && (0.001..=SUSTAINED_SECONDS).contains(&decay)
        && drop_db >= MIN_DROP_DB
        && drop_db >= DROP_TO_ERROR * estimation_rms_db.max(validation_rms_db);
    // A level that stays put within the error limits is a sustained partial, not a failure.
    let (intercept, slope, decay, sustained) = if decaying {
        (intercept, slope, decay, false)
    } else if drop_db.abs() <= SUSTAIN_DB {
        let slope = -1000.0_f64.ln() / SUSTAINED_SECONDS;
        (mean_y - slope * mean_t, slope, SUSTAINED_SECONDS, true)
    } else {
        return None;
    };
    Some((
        intercept,
        slope,
        ModeEvidence {
            frequency_hz: hz,
            decay_seconds: decay,
            estimation_rms_db,
            validation_rms_db,
            estimation_windows: training.len(),
            validation_windows: validation.len(),
            sustained,
            fast_decay_seconds: 0.0,
            fast_share: 0.0,
            beat_hz: 0.0,
            beat_share: 0.0,
            phase: 0.0,
            loose: false,
        },
    ))
}

#[cfg(test)]
#[path = "modal_tests.rs"]
mod tests;
