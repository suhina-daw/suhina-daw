// Copyright (C) 2026 The Suhina contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! Launch modes, legato and per-clip launch quantisation: what a press and a release of a
//! slot's launch button do (`docs/spec/clip-launching.md` › Launch settings). The launcher's
//! queue does the waiting; this module decides what goes into it. Never allocates.

use suhina_model::{LaunchMode, LaunchQuantization, LaunchSettings};

use super::{ClipLauncher, ClipPlan, Pending, Playing, Quantization, Queued};

/// A slot clip's launch settings, ready for the audio thread.
#[derive(Debug, Clone, Copy, Default)]
pub(super) struct SlotLaunch {
    pub(super) mode: LaunchMode,
    pub(super) legato: bool,
    /// The clip's own grid; `None` follows the launcher's global one.
    pub(super) quantization: Option<Quantization>,
}

impl SlotLaunch {
    /// `settings`, with a clip's bars kept as bars: the launcher counts them as it counts the
    /// global quantisation's, through the meter or from where a scene changed the time
    /// signature.
    pub(super) fn new(settings: &LaunchSettings) -> Self {
        let quantization = match settings.quantization {
            LaunchQuantization::Bars(n) => {
                Some(Quantization::Bars(u32::from(if matches!(n, 8 | 4 | 2) { n } else { 1 })))
            }
            // A note value's step does not depend on the bar.
            other => other.beats(4.0).map(|step| {
                if step > 0.0 && step.is_finite() { Quantization::Beats(step) } else { Quantization::None }
            }),
        };
        Self { mode: settings.mode, legato: settings.legato, quantization }
    }
}

impl ClipLauncher {
    /// The grid a launch of `slot` in `column` waits for: the clip's own, or the global one.
    pub(super) fn slot_quantization(&self, column: usize, slot: usize) -> Quantization {
        self.columns
            .get(column)
            .and_then(|c| c.slots.get(slot))
            .and_then(|s| s.launch.quantization)
            .unwrap_or(self.bars.quantization)
    }

    /// A press of the launch button of `slot`, which holds a clip that can play.
    pub(super) fn press(&mut self, column: usize, slot: usize) {
        let quantization = self.slot_quantization(column, slot);
        let Some(c) = self.columns.get_mut(column) else { return };
        let Some(mode) = c.slots.get(slot).map(|s| s.launch.mode) else { return };
        c.held = mode.uses_release().then_some(slot);
        if mode == LaunchMode::Toggle {
            let queued = c.pending.map(|p| p.action);
            let playing = c.playing.is_some_and(|p| p.slot == slot);
            if queued == Some(Queued::Launch(slot)) || (playing && queued == Some(Queued::Stop)) {
                // A second press before the boundary takes the first one back.
                c.pending = None;
                return;
            }
            if playing {
                self.queue_on(column, Queued::Stop, quantization);
                return;
            }
        }
        self.queue_on(column, Queued::Launch(slot), quantization);
    }

    /// The release of the launch button of `slot`.
    pub(super) fn release(&mut self, column: usize, slot: usize) {
        let quantization = self.slot_quantization(column, slot);
        let Some(c) = self.columns.get_mut(column) else { return };
        if c.held != Some(slot) {
            return;
        }
        c.held = None;
        let queued = c.pending.map(|p| (p.action, p.repeat));
        match c.slots.get(slot).map(|s| s.launch.mode) {
            Some(LaunchMode::Gate) => {
                if c.playing.is_some_and(|p| p.slot == slot) {
                    self.queue_on(column, Queued::Stop, quantization);
                } else if queued.is_some_and(|(action, _)| action == Queued::Launch(slot)) {
                    // Released before the launch: the clip does not play.
                    c.pending = None;
                }
            }
            // The clip plays on; only the next repeat is taken back.
            Some(LaunchMode::Repeat) if queued == Some((Queued::Launch(slot), true)) => c.pending = None,
            _ => {}
        }
    }

    /// Called when `slot` in `column` was launched at `at_beat`: while the button of a clip in
    /// repeat mode is held, its next launch is queued one grid step later.
    pub(super) fn launched(&mut self, column: usize, slot: usize, at_beat: f64) {
        let step = self.slot_quantization(column, slot).step(self.bars.bar_beats);
        let Some(c) = self.columns.get_mut(column) else { return };
        if c.held.is_some_and(|held| held != slot) {
            // Another clip took the column over; its release no longer does anything.
            c.held = None;
            return;
        }
        let repeat = c.slots.get(slot).is_some_and(|s| s.launch.mode == LaunchMode::Repeat);
        if repeat && c.held == Some(slot) && step > 0.0 {
            c.pending = Some(Pending { at_beat: at_beat + step, action: Queued::Launch(slot), repeat: true });
        }
    }
}

/// Starts `playing` where the clip it replaces was, `previous` (beats and frames since that
/// clip's launch), instead of at its own start: a clip that loops wraps into its loop, one that
/// does not into its length. Stretch voices are moved there.
pub(super) fn take_over(plan: &mut ClipPlan, playing: &mut Playing, previous: (f64, f64), frames_per_beat: f64) {
    let (beats, frames) = previous;
    if !(beats.is_finite() && frames.is_finite() && beats > 0.0) {
        return;
    }
    let elapsed = match plan.loop_range {
        Some(_) => beats,
        None if plan.length > 0.0 => beats.rem_euclid(plan.length),
        None => 0.0,
    };
    playing.elapsed_beats = elapsed;
    playing.elapsed_frames = frames * elapsed / beats;
    playing.last_clip_beat = plan.clip_beat(elapsed);
    playing.next_note = plan.first_note_from(playing.last_clip_beat);
    let to = (elapsed * frames_per_beat).round().max(0.0) as u64;
    for voice in plan.audio.iter_mut().filter_map(|a| a.stretch.as_mut()) {
        voice.seek(to);
    }
}

#[cfg(test)]
mod tests;
