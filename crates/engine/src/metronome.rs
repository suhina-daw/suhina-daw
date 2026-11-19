// Copyright (C) 2026 The Suhina contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! The metronome: a short click on every beat while the transport runs, higher on the first beat
//! of a bar. It follows the launcher's beat clock, so it keeps time through tempo changes, scene
//! tempos and graph swaps, and the meter's bars through time-signature changes. It is off until
//! [`crate::LaunchCommand::SetMetronome`] turns it on. Its sounds are a [`ClickSet`]: a click on
//! the beat and one on the bar, each synthesized, one of the recorded clicks in
//! `assets/metronome/` or a sound file of the library's, sent with [`crate::Controller::set_clicks`].
//! Where it joins the output is a [`ClickRoute`].

/// A synthesized click is cut after `LENGTH_SECONDS`.
const LENGTH_SECONDS: f64 = 0.05;

/// How much faster a recorded click plays on a bar's first beat: a fifth higher.
const BAR_RATE: f64 = 1.5;

/// The longest a click may be, in seconds: a longer sound file is cut to it.
pub const MAX_CLICK_SECONDS: f64 = 2.0;

/// One click: synthesized, or a recorded sound.
#[derive(Debug, Clone, PartialEq)]
pub enum Click {
    /// A sine at `hz` decaying by the time constant `decay` (seconds), cut after 50 ms; on a
    /// bar's first beat it plays at `bar_hz` unless the bar has a click of its own.
    Tone { hz: f64, bar_hz: f64, decay: f64 },
    /// A recorded sound's samples (one channel) at their sample rate, read `rate` times as fast.
    Samples { samples: Vec<f32>, sample_rate: f64, rate: f64 },
}

/// The clicks a beat and a bar's first beat play.
#[derive(Debug, Clone, PartialEq)]
pub struct ClickSet {
    beat: Click,
    bar: Click,
}

impl Default for ClickSet {
    /// The beep, higher on the bar.
    fn default() -> Self {
        let beep = Click::Tone { hz: 1_000.0, bar_hz: 1_600.0, decay: 0.012 };
        Self::new(beep, None)
    }
}

impl ClickSet {
    /// `beat` on every beat and `bar` on a bar's first beat, or without one, `beat` higher there:
    /// a synthesized click at its bar pitch, a recorded one a fifth higher (and so shorter).
    pub fn new(beat: Click, bar: Option<Click>) -> Self {
        let bar = bar.unwrap_or_else(|| match &beat {
            Click::Tone { bar_hz, decay, .. } => Click::Tone { hz: *bar_hz, bar_hz: *bar_hz, decay: *decay },
            Click::Samples { samples, sample_rate, rate } => {
                Click::Samples { samples: samples.clone(), sample_rate: *sample_rate, rate: rate * BAR_RATE }
            }
        });
        Self { beat, bar }
    }

    /// The already loaded beat or bar sound, for replacing one without rereading the other.
    pub fn click(&self, bar: bool) -> &Click {
        if bar { &self.bar } else { &self.beat }
    }
}

/// A family of clicks that sound alike, as the metronome's menu groups them: built in, or a
/// folder of sound files in the library (`folder`), each file one of its sounds.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ClickFamily {
    pub name: String,
    /// The library folder its sounds are in; `None` for a family built into Suhina.
    pub folder: Option<String>,
    /// Its sounds: the id commands use and the name shown.
    pub sounds: Vec<(String, String)>,
}

/// The built-in families: their names and their sounds' ids and names.
const BUILT_IN: [(&str, &[(&str, &str)]); 2] = [
    ("Tones", &[("beep", "Beep"), ("wood", "Wood")]),
    ("Soft", &[("soft", "Soft"), ("muted", "Muted"), ("deep", "Deep"), ("tick", "Tick"), ("knock", "Knock")]),
];

/// The families built into Suhina, the synthesized tones first.
pub fn built_in_click_families() -> Vec<ClickFamily> {
    BUILT_IN
        .iter()
        .map(|(name, sounds)| ClickFamily {
            name: (*name).to_string(),
            folder: None,
            sounds: sounds.iter().map(|(id, name)| ((*id).to_string(), (*name).to_string())).collect(),
        })
        .collect()
}

impl Click {
    /// The built-in click `id` (`beep`, `wood`, `soft`, `muted`, `deep`, `tick`, `knock`), or
    /// `None` for another name. Decodes the recorded ones the first time: not on the audio thread.
    pub fn built_in(id: &str) -> Option<Self> {
        let index = match id {
            "beep" => return Some(Self::Tone { hz: 1_000.0, bar_hz: 1_600.0, decay: 0.012 }),
            "wood" => return Some(Self::Tone { hz: 1_800.0, bar_hz: 2_500.0, decay: 0.004 }),
            "soft" => 0,
            "muted" => 1,
            "deep" => 2,
            "tick" => 3,
            "knock" => 4,
            _ => return None,
        };
        let clip = recorded().get(index)?;
        Some(Self::Samples { samples: clip.samples.clone(), sample_rate: clip.sample_rate, rate: 1.0 })
    }

    /// A click from a decoded sound file: its channels mixed to one, cut to
    /// [`MAX_CLICK_SECONDS`]. `None` for a file with no frames or no sample rate.
    pub fn from_audio(data: &crate::AudioData) -> Option<Self> {
        let channels = usize::from(data.channels.max(1));
        let rate = data.sample_rate;
        if rate == 0 {
            return None;
        }
        let most = (MAX_CLICK_SECONDS * f64::from(rate)) as usize;
        let samples: Vec<f32> = data
            .samples
            .chunks_exact(channels)
            .take(most)
            .map(|frame| {
                let sum: f32 = frame.iter().map(|s| if s.is_finite() { *s } else { 0.0 }).sum();
                (sum / channels as f32).clamp(-1.0, 1.0)
            })
            .collect();
        (!samples.is_empty()).then(|| Self::Samples { samples, sample_rate: f64::from(rate), rate: 1.0 })
    }
}

/// The recorded clicks, in the order of [`Click::built_in`]'s indexes.
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
    /// Recorded: the bar's click or the beat's, the position in it and how far that moves a frame.
    Recorded { bar: bool, at: f64, step: f64 },
}

#[derive(Debug, Clone, Copy)]
pub(crate) struct Metronome {
    on: bool,
    gain: f32,
    /// The beat whose click last started; `None` before the first one.
    last_beat: Option<i64>,
    /// The click sounding; `None` when silent.
    sounding: Option<Sounding>,
    sample_rate: f64,
}

impl Metronome {
    pub(crate) fn new(sample_rate: u32) -> Self {
        Self { on: false, gain: 0.5, last_beat: None, sounding: None, sample_rate: f64::from(sample_rate) }
    }

    /// Turns the click on or off, at `gain` (linear; non-finite or negative is silence), or at
    /// the level it had.
    pub(crate) fn set(&mut self, on: bool, gain: Option<f32>) {
        self.on = on;
        if let Some(gain) = gain {
            self.gain = if gain.is_finite() { gain.max(0.0) } else { 0.0 };
        }
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

    /// The click's sample from `clicks` for a frame at launcher beat `beat`; `starts_bar` says
    /// whether a beat number starts a bar (asked once a beat). Never allocates.
    pub(crate) fn frame(
        &mut self,
        clicks: &ClickSet,
        running: bool,
        beat: f64,
        starts_bar: impl Fn(i64) -> bool,
    ) -> f32 {
        if running && beat.is_finite() {
            let index = beat.floor() as i64;
            if self.last_beat != Some(index) {
                self.last_beat = Some(index);
                if self.on {
                    self.sounding = Some(self.start(clicks, starts_bar(index)));
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
            Some(Sounding::Recorded { bar, at, step }) => {
                // A set that arrived while the click sounds is read on from where it was.
                let samples = match clicks.click(*bar) {
                    Click::Samples { samples, .. } => &samples[..],
                    Click::Tone { .. } => &[][..],
                };
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

    /// A click of `clicks` starting now, the bar's when `bar`.
    fn start(&self, clicks: &ClickSet, bar: bool) -> Sounding {
        match clicks.click(bar) {
            Click::Tone { hz, decay, .. } => Sounding::Tone { age: 0.0, step: hz / self.sample_rate, decay: *decay },
            Click::Samples { sample_rate, rate, .. } => {
                let step = sample_rate / self.sample_rate.max(1.0) * rate;
                Sounding::Recorded { bar, at: 0.0, step: if step.is_finite() && step > 0.0 { step } else { 1.0 } }
            }
        }
    }
}

/// How many click sets can wait to reach the audio thread, and to come back from it.
const CLICK_QUEUE: usize = 8;

/// The renderer's click set and the queues a new one comes by and the old one goes back by, so
/// the audio thread never frees one.
pub(crate) struct Clicks {
    pub(crate) set: std::sync::Arc<ClickSet>,
    incoming: Option<rtrb::Consumer<std::sync::Arc<ClickSet>>>,
    retired: Option<rtrb::Producer<std::sync::Arc<ClickSet>>>,
}

impl Clicks {
    pub(crate) fn new() -> Self {
        Self { set: std::sync::Arc::new(ClickSet::default()), incoming: None, retired: None }
    }

    /// Connects a new pair of queues; the controller gets the other ends.
    pub(crate) fn sender(&mut self) -> ClickSender {
        let (sets, incoming) = rtrb::RingBuffer::new(CLICK_QUEUE);
        let (outgoing, retired) = rtrb::RingBuffer::new(CLICK_QUEUE);
        self.incoming = Some(incoming);
        self.retired = Some(outgoing);
        ClickSender { sets, retired, outstanding: 0 }
    }

    /// Takes the sets that arrived, the last one staying; each one replaced goes back to be
    /// freed. A set waits while the way back is full. Never allocates or frees.
    pub(crate) fn receive(&mut self) {
        let (Some(incoming), Some(retired)) = (self.incoming.as_mut(), self.retired.as_mut()) else { return };
        while retired.slots() > 0 {
            let Ok(set) = incoming.pop() else { return };
            let old = std::mem::replace(&mut self.set, set);
            // There was room, and only this thread pushes: it goes back.
            if let Err(rtrb::PushError::Full(old)) = retired.push(old) {
                std::mem::forget(old);
            }
        }
    }
}

/// The controller's end of [`Clicks`].
pub(crate) struct ClickSender {
    sets: rtrb::Producer<std::sync::Arc<ClickSet>>,
    retired: rtrb::Consumer<std::sync::Arc<ClickSet>>,
    // Each accepted set reserves a return slot until its predecessor is collected. Counting
    // both queues keeps that slot reserved even if the renderer runs during a send.
    outstanding: usize,
}

impl ClickSender {
    pub(crate) fn collect(&mut self) {
        while self.retired.pop().is_ok() {
            self.outstanding = self.outstanding.saturating_sub(1);
        }
    }

    /// Frees the sets that came back, then sends `set`. Fails while the queue is full.
    pub(crate) fn send(&mut self, set: std::sync::Arc<ClickSet>) -> Result<(), crate::ControlError> {
        self.collect();
        self.enqueue(set)
    }

    /// Reserves the return slot before accepting a set, without collecting again.
    fn enqueue(&mut self, set: std::sync::Arc<ClickSet>) -> Result<(), crate::ControlError> {
        if self.outstanding >= CLICK_QUEUE {
            return Err(crate::ControlError::Full("metronome sound"));
        }
        self.sets.push(set).map_err(|_| crate::ControlError::Full("metronome sound"))?;
        self.outstanding += 1;
        Ok(())
    }
}

#[cfg(test)]
#[path = "metronome_tests.rs"]
mod tests;
