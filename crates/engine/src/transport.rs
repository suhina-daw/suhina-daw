// Copyright (C) 2026 The Suhina contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! The transport (#764 › 5): where the arrangement plays and whether it rolls, as a
//! [`Transport`] the renderer holds, and the renderer's functions that start, stop, locate, jump
//! and loop it. Arrangement time is `position - origin + time_at_origin`, in frames, placed on
//! beats through the graph's tempo map.

use std::sync::atomic::Ordering;

use super::*;
use crate::launcher::*;

/// The transport's state.
pub(crate) struct Transport {
    /// Frames rendered since the renderer was built.
    pub(crate) position: u64,
    /// The frame at which the transport last started or the tempo last changed, and the
    /// arrangement frame playing then: arrangement time is `position - origin + time_at_origin`.
    pub(crate) origin: u64,
    pub(crate) time_at_origin: u64,
    /// The play start marker, in beats: where the transport starts and, while stopped, the
    /// playhead (`docs/spec/arrangement-editing.md` › Play position).
    pub(crate) start_beat: f64,
    pub(crate) running: bool,
    /// The transport started or jumped: the notes that span the playhead start on the next
    /// frame, as if their beginning were there (`docs/spec/arrangement-editing.md` › Play
    /// position).
    pub(crate) chase: bool,
    /// A start waiting for the stretched clips at the marker to be rendered, with the frames
    /// waited so far: the transport rolls once they are, so their beginnings are heard (a
    /// locate waits for the read-ahead in OSS hosts too), or after [`START_WAIT_SECONDS`].
    pub(crate) start_waiting: Option<u64>,
    /// The count-in before recording starts the transport, while it counts.
    pub(crate) count_in: Option<crate::count_in::CountIn>,
    /// Playback repeats the project's loop region; off for a render of the whole arrangement.
    pub(crate) follow_loop: bool,
}

impl Transport {
    /// At frame 0 with the play start marker at the beginning, rolling when `running`.
    pub(crate) fn new(running: bool) -> Self {
        Self {
            position: 0,
            origin: 0,
            time_at_origin: 0,
            start_beat: 0.0,
            running,
            chase: false,
            start_waiting: None,
            count_in: None,
            follow_loop: true,
        }
    }
}

impl Renderer {
    pub(crate) fn ensure_running(&mut self) {
        if !self.transport.running {
            self.start_transport();
        }
    }

    /// Starts the transport once the stretched clips at the marker are rendered ahead (at once
    /// when they are, offline, or without any), so their beginnings are not lost to a worker
    /// still filling their rings.
    pub(crate) fn start_when_ready(&mut self) {
        if !self.stretch_fresh {
            self.graph.ahead.move_to(self.start_frame());
            self.stretch_fresh = true;
        }
        if self.graph.ahead.ready_at(self.start_frame()) {
            self.start_transport();
        } else {
            self.transport.start_waiting = Some(0);
        }
    }

    /// Moves a waiting start along before a block of `frames`: rolls when the stretched clips
    /// are ready or it has waited [`crate::START_WAIT_SECONDS`].
    pub(crate) fn wait_to_start(&mut self, frames: u64) {
        let Some(waited) = self.transport.start_waiting else { return };
        self.graph.ahead.service();
        let limit = (crate::START_WAIT_SECONDS * f64::from(self.sample_rate)) as u64;
        let target = self.frame_of_beat(self.start_target.unwrap_or(self.transport.start_beat));
        if waited >= limit || self.graph.ahead.ready_at(target) {
            self.transport.start_waiting = None;
            self.start_transport();
        } else {
            self.transport.start_waiting = Some(waited + frames);
        }
    }

    pub(crate) fn start_transport(&mut self) {
        // Native clip readers already guard their starts. Hosted processors may
        // produce an immediate step (including ARA or a chased instrument note).
        if !self.plugin_info.is_empty() {
            self.start_guard.begin(self.graph.latency);
        }
        let resumed = self.start_target.take();
        let target = resumed.unwrap_or(self.transport.start_beat);
        self.transport.start_waiting = None;
        self.transport.running = true;
        self.transport.count_in = None;
        // A start plays the arrangement's tempo and bars again; a scene launched with it takes
        // the clock off the map again at its launch beat.
        if resumed.is_none() {
            self.graph.launcher.follow_map();
        }
        // The stretch voices are moved to the marker when the transport stops or the marker
        // moves, so the worker has them ready by the next start; only a start without either
        // before it moves them here.
        if !self.stretch_fresh {
            self.graph.ahead.move_to(self.start_frame());
            self.stretch_fresh = true;
        }
        self.locate(target);
        self.transport.chase = true;
        self.graph.launcher.monitor.running.store(true, Ordering::Relaxed);
    }

    /// Plays the arrangement from `beat` from the next frame on: notes that start there or
    /// later, audio clips from wherever they are at that beat. A start or a jump also chases the
    /// notes that started before it and still sound (`Renderer::chase_notes`); a loop's wrap does
    /// not (`docs/spec/arrangement-editing.md` › Play position).
    pub(crate) fn locate(&mut self, beat: f64) {
        self.transport.origin = self.transport.position;
        self.transport.time_at_origin = self.frame_of_beat(beat);
        let time = self.transport.time_at_origin;
        self.next_event = self.graph.events.partition_point(|e| e.start < time);
        // Audio clips that started before `time` and are still playing there start mid-clip.
        self.next_audio = 0;
        self.active_audio_count = 0;
        self.metronome.reset_at(beat);
        self.graph.launcher.beat = beat;
        self.graph.launcher.tempo_from_map();
    }

    /// The arrangement frame of `beat`, through the playing graph's tempo map.
    pub(crate) fn frame_of_beat(&self, beat: f64) -> u64 {
        crate::tempo::frame_of(&self.graph.tempo, beat)
    }

    /// The play start marker's arrangement frame.
    pub(crate) fn start_frame(&self) -> u64 {
        self.frame_of_beat(self.transport.start_beat)
    }

    /// The arrangement beat playing now.
    pub(crate) fn playhead_beat(&self) -> f64 {
        crate::tempo::beat_at(&self.graph.tempo, self.time())
    }

    /// Ends every sounding note and device tail that the transport stopping or jumping cuts.
    pub(crate) fn release_sounding(&mut self) {
        for voice in self.voices.iter_mut().filter(|v| v.active) {
            voice.end = voice.end.min(self.transport.position);
        }
        let frame = self.block_frame;
        for chain in &mut self.graph.chains {
            chain.release_all(frame);
        }
        self.active_audio_count = 0;
    }

    /// Whether an arrangement audio clip plays now; one a launched clip replaces does not.
    pub(crate) fn arrangement_audio_sounding(&self) -> bool {
        let launcher = &self.graph.launcher;
        let time = self.time();
        let direct = self
            .active_audio
            .iter()
            .take(self.active_audio_count)
            .any(|index| self.graph.audio_events.get(*index).is_some_and(|a| !launcher.overrides(a.column)));
        direct
            || self
                .graph
                .stretch_events
                .iter()
                .any(|e| e.start <= time && time < e.end && !launcher.overrides(e.column))
    }

    /// Stops the transport; the playhead goes back to the marker, or, with `here`, the marker
    /// moves to the playhead. Stopping while stopped moves the marker to beat 0.
    pub(crate) fn stop_transport(&mut self, here: bool) {
        // Hosted instruments fade their continuing waveform before reporting stopped,
        // ahead of downstream effects and delay compensation. A second output ramp would
        // replace the start of that waveform with a held sample. Keep the legacy fallback
        // for processors that do not provide that contract.
        let mut has_instrument = false;
        let mut needs_guard = false;
        for slot in self.graph.chains.iter_mut().flat_map(|chain| chain.plugin_slots()).filter(|s| s.instrument) {
            has_instrument = true;
            needs_guard |= !slot.processor.as_ref().is_some_and(|p| p.smooth_transport_stop());
        }
        // An arrangement audio clip has no stop fade of its own: without the ramp it ends on
        // whatever sample it had reached, a click, with a smooth hosted instrument beside it too.
        let hosted_step = !self.plugin_info.is_empty() && (!has_instrument || needs_guard);
        if self.transport.running && (hosted_step || self.arrangement_audio_sounding()) {
            self.start_guard.begin_stop(self.graph.latency);
        }
        self.transport.start_beat = if !self.transport.running {
            0.0
        } else if here {
            self.playhead_beat().clamp(0.0, MAX_START_BEAT)
        } else {
            self.transport.start_beat
        };
        self.transport.running = false;
        self.metronome.reset();
        self.release_sounding();
        // Ready for the next start, at the marker.
        self.graph.ahead.move_to(self.start_frame());
        self.stretch_fresh = true;
        let launcher = &mut self.graph.launcher;
        for column in &mut launcher.columns {
            column.pending = None;
            if let Some(mut playing) = column.playing.take() {
                playing.fade_left = launcher.stop_fade_frames;
                column.fading = Some(playing);
            }
        }
        launcher.pending_tempo = None;
        launcher.pending_signature = None;
        // A scene's time signature stays while stopped, its bars counted from the start again;
        // the next start puts the clock back on the tempo map.
        launcher.bar_origin = 0.0;
        launcher.bars_before = 0.0;
        launcher.beat = self.transport.start_beat;
        launcher.tempo_from_map();
        launcher.monitor.running.store(false, Ordering::Relaxed);
    }

    /// Moves the play start marker without moving playback (#657).
    pub(crate) fn set_start(&mut self, beat: f64) {
        if !beat.is_finite() {
            return;
        }
        if self.transport.running {
            self.transport.start_beat = beat.clamp(0.0, MAX_START_BEAT);
        } else {
            self.set_position(beat);
        }
    }

    /// A seek at the next global launch boundary while playing, at once with no quantisation
    /// or while stopped (#657).
    pub(crate) fn jump_on_grid(&mut self, beat: f64) {
        if !beat.is_finite() {
            return;
        }
        let at = self.graph.launcher.launch_beat();
        if !self.transport.running || at <= self.graph.launcher.beat() + 1e-9 {
            self.pending_jump = None;
            self.set_position(beat);
            return;
        }
        self.pending_jump = Some((beat.clamp(0.0, MAX_START_BEAT), at));
    }

    /// Frames until a pending jump's boundary, while one waits and the transport runs.
    pub(crate) fn frames_to_jump(&self) -> Option<usize> {
        let (_, at) = self.pending_jump.filter(|_| self.transport.running)?;
        let per_frame =
            crate::tempo::beats_per_frame(self.graph.launcher.tempo_bpm(), f64::from(self.sample_rate.max(1)));
        if !(per_frame.is_finite() && per_frame > 0.0) {
            return Some(0);
        }
        let frames = ((at - self.graph.launcher.beat()) / per_frame).ceil();
        Some(if frames.is_finite() && frames > 0.0 { frames.min(usize::MAX as f64) as usize } else { 0 })
    }

    /// Makes a pending jump whose boundary has come.
    pub(crate) fn jump_if_due(&mut self) {
        if let Some((beat, at)) = self.pending_jump
            && self.transport.running
            && self.graph.launcher.beat() >= at - 1e-9
        {
            self.pending_jump = None;
            self.set_position(beat);
        }
    }

    /// Moves the play start marker to `beat`; while running, playback jumps there. Launches and
    /// a scene tempo still waiting are queued again for the next boundary from the new beat.
    pub(crate) fn set_position(&mut self, beat: f64) {
        if !beat.is_finite() {
            return;
        }
        self.start_target = None;
        self.transport.start_beat = beat.clamp(0.0, MAX_START_BEAT);
        if !self.transport.running {
            self.graph.ahead.move_to(self.start_frame());
            self.stretch_fresh = true;
            self.graph.launcher.beat = self.transport.start_beat;
            self.graph.launcher.tempo_from_map();
            return;
        }
        self.jump_to(self.transport.start_beat);
        // A click on the ruler is a jump like `Locate`: the notes sounding there play.
        self.transport.chase = true;
    }

    /// The loop region as arrangement frames, while playback repeats it: the region is on and
    /// at least one frame long.
    pub(crate) fn loop_frames(&self) -> Option<(u64, u64)> {
        if !self.transport.follow_loop {
            return None;
        }
        let (start, end) = self.graph.loop_beats?;
        let (start, end) = (self.frame_of_beat(start), self.frame_of_beat(end));
        (end > start).then_some((start, end))
    }

    /// Frames until the playhead reaches the loop end, while it is running before it. A
    /// playhead past the loop end (moved there) plays on without wrapping.
    pub(crate) fn frames_to_loop_end(&self) -> Option<usize> {
        if !self.transport.running {
            return None;
        }
        let (_, end) = self.loop_frames()?;
        let to_end = end.checked_sub(self.time()).filter(|frames| *frames > 0)?;
        Some(usize::try_from(to_end).unwrap_or(usize::MAX))
    }

    /// At the loop end, playback jumps back to the loop start, as a jump while playing does:
    /// notes sounding are released, notes and audio clips play on from the loop start
    /// (`docs/spec/arrangement-editing.md` › Loop region). The play start marker stays.
    pub(crate) fn wrap_loop(&mut self) {
        if !self.transport.running {
            return;
        }
        let Some((_, end)) = self.loop_frames() else { return };
        if self.time() != end {
            return;
        }
        if let Some((start, _)) = self.graph.loop_beats {
            self.jump_to(start);
        }
        self.graph.launcher.publish();
    }

    /// Brings the launcher clock, back on the tempo map, to the arrangement: while running its
    /// beat becomes the playhead's (a scene tempo had let it drift), launched clips play on from
    /// where they are, and launches still waiting are queued again for the next boundary from
    /// there, as after a jump. Never allocates.
    pub(crate) fn clock_to_map(&mut self) {
        if !self.graph.launcher.follows_map {
            return;
        }
        if self.transport.running {
            let beat = self.playhead_beat();
            self.metronome.reset_at(beat);
            let launcher = &mut self.graph.launcher;
            launcher.beat = beat;
            let at_beat = launcher.launch_beat();
            for column in &mut launcher.columns {
                if let Some(pending) = column.pending.as_mut() {
                    pending.at_beat = at_beat;
                }
            }
        }
        self.graph.launcher.tempo_from_map();
        self.graph.launcher.publish();
    }

    /// While running, playback continues from `beat` from the next frame on.
    pub(crate) fn jump_to(&mut self, beat: f64) {
        self.release_sounding();
        self.locate(beat);
        // Read from the new place; until the worker gets there, stretched clips are silent.
        self.graph.ahead.move_to(self.transport.time_at_origin);
        self.stretch_fresh = false;
        let launcher = &mut self.graph.launcher;
        let at_beat = launcher.launch_beat();
        for column in &mut launcher.columns {
            if let Some(pending) = column.pending.as_mut() {
                pending.at_beat = at_beat;
            }
        }
        if let Some((at, _)) = launcher.pending_tempo.as_mut() {
            *at = at_beat;
        }
        if let Some((at, _)) = launcher.pending_signature.as_mut() {
            *at = at_beat;
        }
    }
}
