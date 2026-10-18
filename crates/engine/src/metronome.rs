// Copyright (C) 2026 The Suhina contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! The metronome: a short click on every beat while the transport runs, higher on the first beat
//! of a bar. It follows the launcher's beat clock, so it keeps time through tempo changes, scene
//! tempos and graph swaps, and the meter's bars through time-signature changes. It is off until
//! [`crate::LaunchCommand::SetMetronome`] turns it on.

/// The click is cut after `LENGTH_SECONDS`.
const LENGTH_SECONDS: f64 = 0.05;

/// The click's sound, so a long session need not hear the same one throughout.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum ClickSound {
    /// A plain sine beep, 1 kHz on a beat and 1.6 kHz on a bar's first beat.
    #[default]
    Beep,
    /// Higher and much shorter, like a wood block.
    Wood,
    /// Lower and rounder, for long takes.
    Soft,
}

impl ClickSound {
    pub const ALL: [Self; 3] = [Self::Beep, Self::Wood, Self::Soft];

    /// The name commands use.
    pub fn name(self) -> &'static str {
        match self {
            Self::Beep => "beep",
            Self::Wood => "wood",
            Self::Soft => "soft",
        }
    }

    pub fn from_name(name: &str) -> Option<Self> {
        Self::ALL.into_iter().find(|s| s.name() == name)
    }

    /// The pitch on a beat and on a bar's first beat, in Hz, and the time constant it decays by.
    fn voice(self) -> (f64, f64, f64) {
        match self {
            Self::Beep => (1_000.0, 1_600.0, 0.012),
            Self::Wood => (1_800.0, 2_500.0, 0.004),
            Self::Soft => (600.0, 900.0, 0.02),
        }
    }
}
/// Fade-in, so the click starts without a step.
const ATTACK_SECONDS: f64 = 0.000_5;

#[derive(Debug, Clone, Copy)]
pub(crate) struct Metronome {
    on: bool,
    gain: f32,
    sound: ClickSound,
    /// The beat whose click last started; `None` before the first one.
    last_beat: Option<i64>,
    /// Frames into the sounding click, its phase step and its decay; `None` when silent.
    sounding: Option<(f64, f64, f64)>,
    sample_rate: f64,
}

impl Metronome {
    pub(crate) fn new(sample_rate: u32) -> Self {
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
    /// number starts a bar (asked once a beat).
    pub(crate) fn frame(&mut self, running: bool, beat: f64, starts_bar: impl Fn(i64) -> bool) -> f32 {
        if running && beat.is_finite() {
            let index = beat.floor() as i64;
            if self.last_beat != Some(index) {
                self.last_beat = Some(index);
                if self.on {
                    let (beat_hz, bar_hz, decay) = self.sound.voice();
                    let hz = if starts_bar(index) { bar_hz } else { beat_hz };
                    self.sounding = Some((0.0, hz / self.sample_rate, decay));
                }
            }
        }
        let Some((age, step, decay)) = self.sounding.as_mut() else { return 0.0 };
        let seconds = *age / self.sample_rate;
        if seconds >= LENGTH_SECONDS || !self.on {
            self.sounding = None;
            return 0.0;
        }
        let envelope = (seconds / ATTACK_SECONDS).min(1.0) * (-seconds / *decay).exp();
        let sample = (std::f64::consts::TAU * *step * *age).sin() * envelope;
        *age += 1.0;
        sample as f32 * self.gain
    }
}
