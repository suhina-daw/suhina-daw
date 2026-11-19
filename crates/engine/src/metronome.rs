// Copyright (C) 2026 The Suhina contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! The metronome: a short click on every beat while the transport runs, higher on the first beat
//! of a bar. It follows the launcher's beat clock, so it keeps time through tempo changes, scene
//! tempos and graph swaps, and the meter's bars through time-signature changes. It is off until
//! [`crate::LaunchCommand::SetMetronome`] turns it on. Its sound is synthesized or one of the
//! recorded clicks in `assets/metronome/`, and where it joins the output is a [`ClickRoute`].

/// A synthesized click is cut after `LENGTH_SECONDS`.
const LENGTH_SECONDS: f64 = 0.05;

/// How much faster a recorded click plays on a bar's first beat: a fifth higher.
const BAR_RATE: f64 = 1.5;

/// The click's sound, so a long session need not hear the same one throughout.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum ClickSound {
    /// A plain sine beep, 1 kHz on a beat and 1.6 kHz on a bar's first beat.
    #[default]
    Beep,
    /// Higher and much shorter, like a wood block.
    Wood,
    /// A recorded soft hit (`assets/metronome/soft.wav`). The recorded clicks play a fifth
    /// higher on a bar's first beat.
    Soft,
    /// The soft hit darker, low passed.
    Muted,
    /// The soft hit lower and longer.
    Deep,
    /// Only the soft hit's top, very short.
    Tick,
    /// The soft hit with itself an octave lower under it.
    Knock,
}

/// The recorded clicks, in the order of [`ClickSound::sample`]'s indexes.
const RECORDED: [&[u8]; 5] = [
    include_bytes!("../../../assets/metronome/soft.wav"),
    include_bytes!("../../../assets/metronome/muted.wav"),
    include_bytes!("../../../assets/metronome/deep.wav"),
    include_bytes!("../../../assets/metronome/tick.wav"),
    include_bytes!("../../../assets/metronome/knock.wav"),
];

/// A recorded click, decoded: its samples (the first channel) and their sample rate.
#[derive(Debug, Default)]
pub(crate) struct Recorded {
    pub(crate) samples: Vec<f32>,
    pub(crate) sample_rate: f64,
}

static DECODED: std::sync::OnceLock<Vec<Recorded>> = std::sync::OnceLock::new();

/// The recorded clicks, decoded the first time they are asked for; a file that cannot be read is
/// silence. Allocates the first time: [`Metronome::new`] asks, off the audio thread, so the
/// audio thread only reads them.
fn recorded() -> &'static [Recorded] {
    DECODED.get_or_init(|| RECORDED.iter().map(|bytes| decode_wav(bytes).unwrap_or_default()).collect())
}

/// The first channel of a WAV file of 32-bit float or 16-bit samples, or `None` for anything
/// else. Never panics on a malformed file.
pub(crate) fn decode_wav(bytes: &[u8]) -> Option<Recorded> {
    let u16_at = |at: usize| bytes.get(at..at.checked_add(2)?).map(|b| u16::from_le_bytes([b[0], b[1]]));
    let u32_at = |at: usize| bytes.get(at..at.checked_add(4)?).map(|b| u32::from_le_bytes([b[0], b[1], b[2], b[3]]));
    if bytes.get(0..4)? != b"RIFF" || bytes.get(8..12)? != b"WAVE" {
        return None;
    }
    let mut at = 12usize;
    let mut format = None;
    while let (Some(id), Some(size)) = (bytes.get(at..at.checked_add(4)?), u32_at(at.checked_add(4)?)) {
        let body = at.checked_add(8)?;
        let size = usize::try_from(size).ok()?;
        match id {
            b"fmt " => {
                format = Some((u16_at(body)?, u16_at(body + 2)?, u32_at(body + 4)?, u16_at(body + 14)?));
            }
            b"data" => {
                let (tag, channels, rate, bits) = format?;
                let data = bytes.get(body..body.saturating_add(size).min(bytes.len()))?;
                let channels = usize::from(channels.max(1));
                let samples = match (tag, bits) {
                    (3, 32) => data
                        .chunks_exact(4 * channels)
                        .map(|b| f32::from_le_bytes([b[0], b[1], b[2], b[3]]))
                        .map(|v| if v.is_finite() { v.clamp(-1.0, 1.0) } else { 0.0 })
                        .collect(),
                    (1, 16) => data
                        .chunks_exact(2 * channels)
                        .map(|b| f32::from(i16::from_le_bytes([b[0], b[1]])) / 32_768.0)
                        .collect(),
                    _ => return None,
                };
                return (rate > 0).then(|| Recorded { samples, sample_rate: f64::from(rate) });
            }
            _ => {}
        }
        // Chunks are padded to an even length.
        at = body.checked_add(size)?.checked_add(size % 2)?;
    }
    None
}

impl ClickSound {
    pub const ALL: [Self; 7] = [Self::Beep, Self::Wood, Self::Soft, Self::Muted, Self::Deep, Self::Tick, Self::Knock];

    /// The name commands use.
    pub fn name(self) -> &'static str {
        match self {
            Self::Beep => "beep",
            Self::Wood => "wood",
            Self::Soft => "soft",
            Self::Muted => "muted",
            Self::Deep => "deep",
            Self::Tick => "tick",
            Self::Knock => "knock",
        }
    }

    pub fn from_name(name: &str) -> Option<Self> {
        Self::ALL.into_iter().find(|s| s.name() == name)
    }

    /// For a synthesized click: the pitch on a beat and on a bar's first beat, in Hz, and the
    /// time constant it decays by.
    fn voice(self) -> Option<(f64, f64, f64)> {
        match self {
            Self::Beep => Some((1_000.0, 1_600.0, 0.012)),
            Self::Wood => Some((1_800.0, 2_500.0, 0.004)),
            _ => None,
        }
    }

    /// For a recorded click: its index in [`RECORDED`].
    fn sample(self) -> Option<usize> {
        match self {
            Self::Beep | Self::Wood => None,
            Self::Soft => Some(0),
            Self::Muted => Some(1),
            Self::Deep => Some(2),
            Self::Tick => Some(3),
            Self::Knock => Some(4),
        }
    }
}

/// Where the click joins the output, against the main channel (`transport.setMetronome`'s
/// `route`). The click's own level applies in every one.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum ClickRoute {
    /// Into the main output after the main channel: its devices, fader and mute leave it alone.
    #[default]
    PreFader,
    /// Past the main channel's devices, but at its fader, mute and pan.
    PostFader,
    /// Only into the cue output; into the main output, as `PreFader`, while the cue has no
    /// outputs of its own and plays with the main mix.
    Cue,
    /// Into the main channel's input: its devices, fader and mute all apply.
    ThroughMain,
}

impl ClickRoute {
    pub const ALL: [Self; 4] = [Self::PreFader, Self::PostFader, Self::Cue, Self::ThroughMain];

    /// The name commands use.
    pub fn name(self) -> &'static str {
        match self {
            Self::PreFader => "pre-fader",
            Self::PostFader => "post-fader",
            Self::Cue => "cue",
            Self::ThroughMain => "through-main",
        }
    }

    pub fn from_name(name: &str) -> Option<Self> {
        Self::ALL.into_iter().find(|s| s.name() == name)
    }
}

/// Fade-in, so a synthesized click starts without a step.
const ATTACK_SECONDS: f64 = 0.000_5;

/// A click sounding.
#[derive(Debug, Clone, Copy)]
enum Sounding {
    /// Synthesized: frames into it, its phase step and its decay.
    Tone { age: f64, step: f64, decay: f64 },
    /// Recorded: which one, the position in it and how far that moves a frame.
    Recorded { index: usize, at: f64, step: f64 },
}

#[derive(Debug, Clone, Copy)]
pub(crate) struct Metronome {
    on: bool,
    gain: f32,
    sound: ClickSound,
    /// The beat whose click last started; `None` before the first one.
    last_beat: Option<i64>,
    /// The click sounding; `None` when silent.
    sounding: Option<Sounding>,
    sample_rate: f64,
}

impl Metronome {
    pub(crate) fn new(sample_rate: u32) -> Self {
        // The recorded clicks are decoded here, off the audio thread.
        recorded();
        Self {
            on: false,
            gain: 0.5,
            sound: ClickSound::Beep,
            last_beat: None,
            sounding: None,
            sample_rate: f64::from(sample_rate),
        }
    }

    /// Turns the click on or off, at `gain` (linear; non-finite or negative is silence), or at
    /// the level it had.
    pub(crate) fn set(&mut self, on: bool, gain: Option<f32>) {
        self.on = on;
        if let Some(gain) = gain {
            self.gain = if gain.is_finite() { gain.max(0.0) } else { 0.0 };
        }
    }

    /// The sound the next click plays.
    pub(crate) fn set_sound(&mut self, sound: ClickSound) {
        self.sound = sound;
    }

    pub(crate) fn on(&self) -> bool {
        self.on
    }

    /// The click's level, linear.
    pub(crate) fn gain(&self) -> f32 {
        self.gain
    }

    /// Forgets the beat counted so far, so the next beat clicks (the transport started or stopped).
    pub(crate) fn reset(&mut self) {
        self.last_beat = None;
        self.sounding = None;
    }

    /// Like [`Metronome::reset`] for a transport starting at `beat`: a start on a beat clicks,
    /// a start between beats waits for the next one.
    pub(crate) fn reset_at(&mut self, beat: f64) {
        self.reset();
        if beat.is_finite() && beat.fract().abs() > 1e-9 {
            self.last_beat = Some(beat.floor() as i64);
        }
    }

    /// The click's sample for a frame at launcher beat `beat`; `starts_bar` says whether a beat
    /// number starts a bar (asked once a beat). Never allocates.
    pub(crate) fn frame(&mut self, running: bool, beat: f64, starts_bar: impl Fn(i64) -> bool) -> f32 {
        if running && beat.is_finite() {
            let index = beat.floor() as i64;
            if self.last_beat != Some(index) {
                self.last_beat = Some(index);
                if self.on {
                    self.sounding = self.start(starts_bar(index));
                }
            }
        }
        if !self.on {
            self.sounding = None;
        }
        let sample = match self.sounding.as_mut() {
            None => return 0.0,
            Some(Sounding::Tone { age, step, decay }) => {
                let seconds = *age / self.sample_rate;
                (seconds < LENGTH_SECONDS).then(|| {
                    let envelope = (seconds / ATTACK_SECONDS).min(1.0) * (-seconds / *decay).exp();
                    let sample = (std::f64::consts::TAU * *step * *age).sin() * envelope;
                    *age += 1.0;
                    sample as f32
                })
            }
            Some(Sounding::Recorded { index, at, step }) => {
                let samples = DECODED.get().and_then(|all| all.get(*index)).map_or(&[][..], |c| &c.samples[..]);
                let whole = *at as usize;
                let fraction = (*at - whole as f64) as f32;
                samples.get(whole).map(|&a| {
                    *at += *step;
                    a + (samples.get(whole + 1).copied().unwrap_or(0.0) - a) * fraction
                })
            }
        };
        match sample {
            Some(sample) => sample * self.gain,
            None => {
                self.sounding = None;
                0.0
            }
        }
    }

    /// A click starting now, on a bar's first beat when `bar`.
    fn start(&self, bar: bool) -> Option<Sounding> {
        if let Some((beat_hz, bar_hz, decay)) = self.sound.voice() {
            let hz = if bar { bar_hz } else { beat_hz };
            return Some(Sounding::Tone { age: 0.0, step: hz / self.sample_rate, decay });
        }
        let index = self.sound.sample()?;
        let clip = DECODED.get()?.get(index)?;
        let rate = if bar { BAR_RATE } else { 1.0 };
        Some(Sounding::Recorded { index, at: 0.0, step: clip.sample_rate / self.sample_rate.max(1.0) * rate })
    }
}

#[cfg(test)]
#[path = "metronome_tests.rs"]
mod tests;
