// Copyright (C) 2026 The Suhina contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! Identified native voices preserve the FM core's fixed 64-frame rendering quantum.
use super::*;

impl FmSynth {
    pub(super) fn finish_instances(&mut self) {
        // A completed compute quantum may still have audible cached samples. Retire only
        // after the host consumed those samples, including both detuned layers.
        if self.position < N {
            return;
        }
        for slot in &mut self.slots {
            if let Some(id) = slot.retiring_instance.take() {
                self.lifetimes.finish(id);
            }
            if (!slot.live || !slot.note.is_playing() && (!slot.spread || !slot.twin.is_playing()))
                && let Some(id) = slot.instance.take()
            {
                self.lifetimes.finish(id);
            }
        }
    }
}

impl NoteInstanceDevice for FmSynth {
    fn event(&mut self, id: NoteInstanceId, event: NoteInstanceEvent) -> Result<(), NoteInstanceDispatchError> {
        let event = event.validated().ok_or(NoteInstanceDispatchError::InvalidEvent)?;
        self.finish_instances();
        if let NoteInstanceEvent::On { key, velocity, expression } = event {
            if self.lifetimes.contains(id) {
                return Err(NoteInstanceDispatchError::AlreadyActive);
            }
            let index = self
                .slots
                .iter()
                .take(self.voices)
                .position(|s| s.instance.is_none() && (!s.live || !s.note.is_playing()))
                .ok_or(NoteInstanceDispatchError::Capacity)?;
            self.lifetimes.admit(id)?;
            self.key_down_at(key, seven_bit(velocity), Some((index, id, expression)));
            return Ok(());
        }
        let slot =
            self.slots.iter_mut().find(|s| s.instance == Some(id)).ok_or(NoteInstanceDispatchError::UnknownInstance)?;
        match event {
            NoteInstanceEvent::Expression { kind, value } => {
                slot.expression.set(kind, value);
            }
            NoteInstanceEvent::Off { .. } => {
                slot.down = false;
                slot.sustained = false;
                slot.release(&self.tables);
            }
            NoteInstanceEvent::Choke { .. } => {
                slot.live = false;
                slot.down = false;
                slot.sustained = false;
                // Cached samples end at the current intrinsic quantum, before retirement.
            }
            NoteInstanceEvent::On { .. } => {}
        }
        Ok(())
    }

    fn drain_finished(&mut self, output: &mut [Option<NoteInstanceId>]) -> usize {
        self.finish_instances();
        self.lifetimes.drain(output)
    }
}
