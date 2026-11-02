// Copyright (C) 2026 The Suhina contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! An envelope in seconds beside the DX7 model (ADR 0034): initial, attack, peak, decay, sustain,
//! release, and a loop mode. An operator's runs in decibels, with a linear attack and exponential
//! decay and release; the filter's, the pitch's and the LFO's run in their own units with slopes
//! and an end level.

/// What happens while a key stays down.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum LoopMode {
    /// Holds the sustain level.
    #[default]
    Off,
    /// From the sustain level, starts over after the loop time.
    Loop,
    /// Starts over at the repeat length, counted from the note.
    Beat,
    /// Starts over on the song's grid of the repeat length while the song plays; otherwise as
    /// [`LoopMode::Beat`].
    Sync,
    /// Ignores the key going up: the release follows the decay.
    Trigger,
}

impl LoopMode {
    pub fn from_index(index: usize) -> Self {
        match index {
            1 => Self::Loop,
            2 => Self::Beat,
            3 => Self::Sync,
            4 => Self::Trigger,
            _ => Self::Off,
        }
    }
}

/// An envelope's settings, as its parameters hold them.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct EnvSettings {
    pub attack: f32,
    pub decay: f32,
    pub release: f32,
    pub initial: f32,
    pub peak: f32,
    pub sustain: f32,
    pub end: f32,
    pub mode: LoopMode,
    pub loop_time: f32,
    pub repeat_beats: f32,
    pub time_by_velocity: f32,
    pub time_by_key: f32,
    pub attack_slope: f32,
    pub decay_slope: f32,
    pub release_slope: f32,
}

impl Default for EnvSettings {
    fn default() -> Self {
        Self {
            attack: 0.001,
            decay: 0.6,
            release: 0.05,
            initial: 0.0,
            peak: 0.0,
            sustain: 0.0,
            end: 0.0,
            mode: LoopMode::Off,
            loop_time: 0.1,
            repeat_beats: 1.0,
            time_by_velocity: 0.0,
            time_by_key: 0.0,
            attack_slope: 0.0,
            decay_slope: 0.0,
            release_slope: 0.0,
        }
    }
}

/// The transport as a block starts.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Clock {
    pub tempo: f64,
    pub beat: f64,
    pub playing: bool,
}

impl Default for Clock {
    fn default() -> Self {
        Self { tempo: 120.0, beat: 0.0, playing: false }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
enum Stage {
    #[default]
    Attack,
    Decay,
    Sustain,
    Release,
    Done,
}

/// A sixteenth note, in beats.
const SIXTEENTH: f64 = 0.25;
/// Shortest Sync repeat, in beats, so a zero cannot stall it.
const SHORTEST_REPEAT: f64 = 1e-3;

/// Decibels at or below which an operator's envelope is silence.
pub const SILENCE_DB: f32 = -69.9;

fn gain(db: f32) -> f32 {
    if db <= SILENCE_DB { 0.0 } else { 10f32.powf(db / 20.0) }
}

fn decibels(gain: f32) -> f32 {
    if gain <= 1e-4 { -80.0 } else { 20.0 * gain.log10() }
}

/// `p` from 0 to 1 bent by `slope`: above 0 fast then slow, below 0 slow then fast.
fn bend(p: f32, slope: f32) -> f32 {
    let p = p.clamp(0.0, 1.0);
    if slope > 0.0 {
        1.0 - (1.0 - p).powf(1.0 + 4.0 * slope)
    } else if slope < 0.0 {
        p.powf(1.0 - 4.0 * slope)
    } else {
        p
    }
}

/// See the module.
#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub struct TimeEnv {
    stage: Stage,
    value: f32,
    from: f32,
    /// Seconds into the stage.
    position: f32,
    /// How much faster than set this note's envelope runs.
    speed: f32,
    /// Seconds since a loop last started, unscaled for Beat.
    timer: f32,
    /// The song position in beats of the next repeat, for Sync.
    next_beat: f64,
    amp: bool,
}

impl TimeEnv {
    /// Starts for a key going down. `velocity` 0 to 1, `key` the MIDI key; `amp` runs in decibels.
    pub fn start(&mut self, s: &EnvSettings, velocity: f32, key: f32, amp: bool, clock: &Clock) {
        let by_velocity = 2f32.powf(s.time_by_velocity * (velocity - 0.5) * 4.0);
        let by_key = 2f32.powf(s.time_by_key * (key - 60.0) / 12.0);
        *self = Self {
            stage: Stage::Attack,
            value: s.initial,
            from: s.initial,
            position: 0.0,
            speed: by_velocity * by_key,
            timer: 0.0,
            next_beat: first_repeat(s, clock),
            amp,
        };
    }

    /// Whether it has ended.
    pub fn is_done(&self) -> bool {
        self.stage == Stage::Done
    }

    fn enter(&mut self, stage: Stage) {
        self.stage = stage;
        self.from = self.value;
        self.position = 0.0;
    }

    fn restart(&mut self) {
        // From the level it has now, so a repeat does not click.
        self.enter(Stage::Attack);
        self.timer = 0.0;
    }

    /// Advances by `seconds` of real time, `time_scale` faster than set, and gives the value.
    /// `down` is whether the key is held.
    pub fn next(&mut self, s: &EnvSettings, seconds: f32, time_scale: f32, down: bool, clock: &Clock) -> f32 {
        let down = down || s.mode == LoopMode::Trigger;
        if !down && !matches!(self.stage, Stage::Release | Stage::Done) {
            self.enter(Stage::Release);
        }
        let scaled = seconds * time_scale * self.speed;
        if down {
            match s.mode {
                LoopMode::Loop if self.stage == Stage::Sustain => {
                    self.timer += scaled;
                    if self.timer >= s.loop_time.max(0.0005) {
                        self.restart();
                    }
                }
                LoopMode::Beat => self.repeat_by_beat(s, seconds, clock),
                LoopMode::Sync if clock.playing => {
                    if clock.beat >= self.next_beat {
                        self.next_beat += f64::from(s.repeat_beats).max(SHORTEST_REPEAT);
                        self.restart();
                    }
                }
                LoopMode::Sync => self.repeat_by_beat(s, seconds, clock),
                _ => {}
            }
        }
        self.position += scaled;
        match self.stage {
            Stage::Attack => {
                let p = self.position / s.attack.max(0.0005);
                self.value = if self.amp {
                    let (a, b) = (gain(self.from), gain(s.peak));
                    decibels(a + (b - a) * p.min(1.0))
                } else {
                    self.from + (s.peak - self.from) * bend(p, s.attack_slope)
                };
                if p >= 1.0 {
                    self.value = s.peak;
                    self.enter(Stage::Decay);
                }
            }
            Stage::Decay => {
                let p = self.position / s.decay.max(0.0005);
                let slope = if self.amp { 0.0 } else { s.decay_slope };
                self.value = self.from + (s.sustain - self.from) * bend(p, slope);
                if p >= 1.0 {
                    self.value = s.sustain;
                    if s.mode == LoopMode::Trigger {
                        self.enter(Stage::Release);
                    } else {
                        // Loop counts from the sustain; Beat and Sync from the note.
                        if s.mode == LoopMode::Loop {
                            self.timer = 0.0;
                        }
                        self.enter(Stage::Sustain);
                    }
                }
            }
            Stage::Sustain => self.value = s.sustain,
            Stage::Release => {
                let p = self.position / s.release.max(0.0005);
                let (end, slope) = if self.amp { (-80.0, 0.0) } else { (s.end, s.release_slope) };
                self.value = self.from + (end - self.from) * bend(p, slope);
                if p >= 1.0 {
                    self.value = end;
                    self.stage = Stage::Done;
                }
            }
            Stage::Done => {}
        }
        self.value
    }

    fn repeat_by_beat(&mut self, s: &EnvSettings, seconds: f32, clock: &Clock) {
        // Beats are song time: the device's Time does not move them.
        self.timer += seconds;
        let length = f64::from(s.repeat_beats) * 60.0 / clock.tempo.max(1.0);
        if f64::from(self.timer) >= length {
            self.restart();
        }
    }
}

/// Where a Sync loop first repeats: one repeat on, at the nearest sixteenth of the song.
fn first_repeat(s: &EnvSettings, clock: &Clock) -> f64 {
    let at = clock.beat + f64::from(s.repeat_beats).max(SHORTEST_REPEAT);
    (at / SIXTEENTH).round() * SIXTEENTH
}

#[cfg(test)]
mod tests {
    use super::*;

    const BLOCK: f32 = 64.0 / 48_000.0;

    fn run(env: &mut TimeEnv, s: &EnvSettings, seconds: f32, down: bool) -> Vec<f32> {
        let blocks = (seconds / BLOCK) as usize;
        (0..blocks).map(|_| env.next(s, BLOCK, 1.0, down, &Clock::default())).collect()
    }

    fn settings() -> EnvSettings {
        EnvSettings {
            attack: 0.01,
            decay: 0.1,
            release: 0.1,
            initial: 0.0,
            peak: 1.0,
            sustain: 0.5,
            end: 0.25,
            ..EnvSettings::default()
        }
    }

    #[test]
    fn it_goes_through_its_stages_and_ends_at_the_end_level() {
        let s = settings();
        let mut env = TimeEnv::default();
        env.start(&s, 0.5, 60.0, false, &Clock::default());
        let up = run(&mut env, &s, 0.5, true);
        assert!(up.iter().any(|v| (*v - 1.0).abs() < 0.2));
        assert_eq!(up.last().copied(), Some(0.5));
        let released = run(&mut env, &s, 0.2, false);
        assert_eq!(released.last().copied(), Some(0.25));
        assert!(env.is_done());
    }

    #[test]
    fn release_starts_from_where_it_is() {
        let s = settings();
        let mut env = TimeEnv::default();
        env.start(&s, 0.5, 60.0, false, &Clock::default());
        let before = run(&mut env, &s, 0.03, true).last().copied().unwrap_or(0.0);
        let after = env.next(&s, BLOCK, 1.0, false, &Clock::default());
        assert!((before - after).abs() < 0.1, "{before} {after}");
    }

    #[test]
    fn loop_starts_over_after_its_time_and_trigger_ignores_the_key() {
        let mut s = settings();
        s.mode = LoopMode::Loop;
        s.loop_time = 0.05;
        let mut env = TimeEnv::default();
        env.start(&s, 0.5, 60.0, false, &Clock::default());
        let out = run(&mut env, &s, 0.6, true);
        assert!(out[200..].iter().any(|v| *v > 0.9));

        s.mode = LoopMode::Trigger;
        env.start(&s, 0.5, 60.0, false, &Clock::default());
        let early = run(&mut env, &s, 0.05, false);
        assert!(early.iter().any(|v| *v > 0.5));
        run(&mut env, &s, 0.3, false);
        assert!(env.is_done());
    }

    #[test]
    fn beat_repeats_by_the_tempo_and_time_does_not_move_it() {
        let mut s = settings();
        s.mode = LoopMode::Beat;
        s.repeat_beats = 1.0;
        let count = |scale: f32| {
            let mut env = TimeEnv::default();
            env.start(&s, 0.5, 60.0, false, &Clock::default());
            let mut last = 0.0;
            let mut rises = 0;
            for _ in 0..(2.0 / BLOCK) as usize {
                let v = env.next(&s, BLOCK, scale, true, &Clock::default());
                if v > 0.9 && last <= 0.9 {
                    rises += 1;
                }
                last = v;
            }
            rises
        };
        assert_eq!(count(1.0), count(0.5));
        assert!((3..=5).contains(&count(1.0)));
    }

    #[test]
    fn sync_first_repeats_at_the_nearest_sixteenth_and_then_by_the_repeat() {
        let mut s = settings();
        s.mode = LoopMode::Sync;
        s.repeat_beats = 1.0;
        let at = |beat: f64| Clock { tempo: 120.0, beat, playing: true };
        let mut env = TimeEnv::default();
        env.start(&s, 0.5, 60.0, false, &at(0.1));
        assert_eq!(env.next_beat, 1.0);
        for beat in [0.2, 0.5, 0.9] {
            env.next(&s, BLOCK, 1.0, true, &at(beat));
        }
        assert_eq!(env.next_beat, 1.0);
        env.next(&s, BLOCK, 1.0, true, &at(1.01));
        assert_eq!(env.next_beat, 2.0);
    }

    #[test]
    fn an_operator_envelope_runs_in_decibels_and_falls_to_silence() {
        let s = EnvSettings { initial: -70.0, peak: 0.0, sustain: -6.0, release: 0.05, ..settings() };
        let mut env = TimeEnv::default();
        env.start(&s, 0.5, 60.0, true, &Clock::default());
        assert_eq!(run(&mut env, &s, 0.5, true).last().copied(), Some(-6.0));
        assert!(run(&mut env, &s, 0.1, false).last().copied().unwrap_or(0.0) <= SILENCE_DB);
        assert!(env.is_done());
    }

    #[test]
    fn slopes_bend_a_stage() {
        assert!(bend(0.5, 1.0) > 0.5);
        assert!(bend(0.5, -1.0) < 0.5);
        assert_eq!(bend(0.5, 0.0), 0.5);
    }
}
