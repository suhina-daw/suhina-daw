// Copyright (C) 2026 The Suhina contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! MIDI controllers ([ADR 0017](../../../docs/decisions/0017-inputs-and-latency.md)): lists the
//! system's MIDI input ports and opens them with `midir`, and turns what they send into the
//! engine's live notes. This is the only code that opens a MIDI device; the session sees it as a
//! [`MidiSystem`].
//!
//! The driver calls back on a thread of its own (never the audio thread), which parses each
//! message and hands the note to the session's [`LiveInput`], and from there to the engine of
//! the project that plays, with the time the driver stamped it with turned into the engine's
//! live clock ([`StampClock`]), so the engine plays it at a fixed delay from when it was played
//! (#269). Notes, the sustain pedal, every other controller, pitch bend and channel pressure
//! (aftertouch) are read, on every channel (#812); program change, polyphonic key pressure,
//! system exclusive, clock and other system messages are ignored.

use std::sync::Arc;

use midir::{Ignore, MidiInput, MidiInputConnection};
use suhina_engine::{LiveEvent, LiveSource, StampClock, live_now, port_id};
use suhina_session::{LiveInput, MidiSystem};

/// The name Suhina's ports carry in the system's MIDI graph.
const CLIENT: &str = "Suhina";
/// Controller numbers read from control change messages.
const SUSTAIN: u8 = 64;
const ALL_SOUND_OFF: u8 = 120;
const ALL_NOTES_OFF: u8 = 123;

/// The MIDI pitch bend wheel's centre, as a 14-bit value.
const BEND_CENTRE: f32 = 8_192.0;

/// The engine's live note or performance control for one MIDI message, with its channel (0 to
/// 15); `None` for anything else, including short or malformed messages. Note on and off,
/// control change and pitch bend are three bytes; channel pressure is two.
pub fn parse(message: &[u8]) -> Option<(u8, LiveEvent)> {
    let (&status, data) = message.split_first()?;
    let event = match status & 0xF0 {
        0x90 | 0x80 => {
            let (&key, rest) = data.split_first()?;
            let &velocity = rest.first()?;
            if key >= 0x80 || velocity >= 0x80 {
                return None;
            }
            if status & 0xF0 == 0x90 && velocity > 0 {
                LiveEvent::NoteOn { key, velocity: f32::from(velocity) / 127.0 }
            } else {
                LiveEvent::NoteOff { key }
            }
        }
        0xB0 => {
            let (&number, rest) = data.split_first()?;
            let &value = rest.first()?;
            if number >= 0x80 || value >= 0x80 {
                return None;
            }
            match number {
                SUSTAIN => LiveEvent::Sustain { on: value >= 64 },
                ALL_SOUND_OFF | ALL_NOTES_OFF => LiveEvent::AllNotesOff,
                _ => LiveEvent::Controller { number, value: f32::from(value) / 127.0 },
            }
        }
        0xE0 => {
            let (&lsb, rest) = data.split_first()?;
            let &msb = rest.first()?;
            if lsb >= 0x80 || msb >= 0x80 {
                return None;
            }
            let combined = f32::from(u16::from(lsb) | (u16::from(msb) << 7));
            LiveEvent::PitchBend(((combined - BEND_CENTRE) / BEND_CENTRE).clamp(-1.0, 1.0))
        }
        0xD0 => {
            let &value = data.first()?;
            if value >= 0x80 {
                return None;
            }
            LiveEvent::Pressure(f32::from(value) / 127.0)
        }
        _ => return None,
    };
    Some((status & 0x0F, event))
}

/// The system's MIDI input ports.
#[derive(Debug, Default, Clone, Copy)]
pub struct Midi;

impl Midi {
    pub fn new() -> Self {
        Self
    }
}

fn client() -> Result<MidiInput, String> {
    let mut input = MidiInput::new(CLIENT).map_err(|e| e.to_string())?;
    // System exclusive, clock and active sensing are not read; skipping them saves the
    // callback's work.
    input.ignore(Ignore::All);
    Ok(input)
}

/// An open port: closing it lets go of the keys it holds, on every channel.
struct Port {
    connection: Option<MidiInputConnection<()>>,
    input: Arc<LiveInput>,
    port: u64,
}

impl Drop for Port {
    fn drop(&mut self) {
        if let Some(connection) = self.connection.take() {
            connection.close();
        }
        for channel in 0..16 {
            self.input.send_from(LiveSource { port: self.port, channel }, LiveEvent::AllNotesOff);
        }
    }
}

impl MidiSystem for Midi {
    fn ports(&self) -> Result<Vec<String>, String> {
        let input = client()?;
        Ok(input.ports().iter().filter_map(|port| input.port_name(port).ok()).collect())
    }

    fn open(&self, name: &str, input: Arc<LiveInput>) -> Result<Box<dyn std::any::Any + Send>, String> {
        let client = client()?;
        let port = client
            .ports()
            .into_iter()
            .find(|port| client.port_name(port).is_ok_and(|n| n == name))
            .ok_or_else(|| format!("{name} is not there"))?;
        let sink = Arc::clone(&input);
        let id = port_id(name);
        let port_name = name.to_string();
        // The live clock starts before the first message, so its first stamp is not at 0.
        let _started = live_now();
        let mut clock = StampClock::default();
        let connection = client
            .connect(
                &port,
                &format!("{CLIENT} input"),
                move |stamp, message, _| {
                    let parsed = parse(message);
                    // Everything but notes, which a player sends many of: whether a wheel or a
                    // knob arrives at all, and what it was read as (docs/debugging.md).
                    if !matches!(parsed, Some((_, LiveEvent::NoteOn { .. } | LiveEvent::NoteOff { .. }))) {
                        suhina_plugins::trace!("midi in {port_name}: {message:02x?} read as {parsed:?}");
                    }
                    if let Some((channel, event)) = parsed {
                        let time = clock.time(stamp, live_now());
                        sink.send_at(LiveSource { port: id, channel }, event, time);
                    }
                },
                (),
            )
            .map_err(|e| e.to_string())?;
        Ok(Box::new(Port { connection: Some(connection), input, port: id }))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn notes_on_every_channel_are_read() {
        assert_eq!(parse(&[0x90, 60, 127]), Some((0, LiveEvent::NoteOn { key: 60, velocity: 1.0 })));
        assert_eq!(parse(&[0x9F, 0, 1]), Some((15, LiveEvent::NoteOn { key: 0, velocity: 1.0 / 127.0 })));
        assert_eq!(parse(&[0x80, 60, 64]), Some((0, LiveEvent::NoteOff { key: 60 })));
        assert_eq!(parse(&[0x8A, 127, 0]), Some((10, LiveEvent::NoteOff { key: 127 })));
        // A note on of velocity 0 is a note off.
        assert_eq!(parse(&[0x93, 72, 0]), Some((3, LiveEvent::NoteOff { key: 72 })));
    }

    #[test]
    fn the_pedal_and_all_notes_off_are_read() {
        assert_eq!(parse(&[0xB0, 64, 127]), Some((0, LiveEvent::Sustain { on: true })));
        assert_eq!(parse(&[0xB5, 64, 64]), Some((5, LiveEvent::Sustain { on: true })));
        assert_eq!(parse(&[0xB0, 64, 63]), Some((0, LiveEvent::Sustain { on: false })));
        assert_eq!(parse(&[0xB0, 123, 0]), Some((0, LiveEvent::AllNotesOff)));
        assert_eq!(parse(&[0xB0, 120, 0]), Some((0, LiveEvent::AllNotesOff)));
    }

    #[test]
    fn other_controllers_the_mod_wheel_among_them_are_read() {
        assert_eq!(parse(&[0xB0, 7, 100]), Some((0, LiveEvent::Controller { number: 7, value: 100.0 / 127.0 })));
        assert_eq!(parse(&[0xB0, 1, 127]), Some((0, LiveEvent::Controller { number: 1, value: 1.0 })));
        assert_eq!(parse(&[0xBA, 74, 0]), Some((10, LiveEvent::Controller { number: 74, value: 0.0 })));
    }

    #[test]
    fn pitch_bend_is_read_as_fourteen_bits_centred_on_zero() {
        assert_eq!(parse(&[0xE0, 0, 64]), Some((0, LiveEvent::PitchBend(0.0))), "centre");
        assert_eq!(parse(&[0xE0, 0, 0]), Some((0, LiveEvent::PitchBend(-1.0))), "full down");
        let full_up = (16_383.0 - 8_192.0) / 8_192.0;
        assert_eq!(parse(&[0xEF, 127, 127]), Some((15, LiveEvent::PitchBend(full_up))), "full up, channel 16");
    }

    #[test]
    fn channel_pressure_is_read_as_a_two_byte_message() {
        assert_eq!(parse(&[0xD0, 64]), Some((0, LiveEvent::Pressure(64.0 / 127.0))));
        assert_eq!(parse(&[0xD5, 127]), Some((5, LiveEvent::Pressure(1.0))));
        assert_eq!(parse(&[0xD0, 0]), Some((0, LiveEvent::Pressure(0.0))));
        // Bytes after a whole message are left alone, as for every other message.
        assert_eq!(parse(&[0xD0, 64, 99, 99]), Some((0, LiveEvent::Pressure(64.0 / 127.0))));
    }

    #[test]
    fn short_malformed_and_other_messages_are_ignored() {
        for message in [
            &[][..],
            &[0x90],
            &[0x90, 60],
            &[0x90, 0x80, 10],
            &[0x90, 60, 0xFF],
            &[60, 100, 0x90],
            &[0xE0, 0],
            &[0xE0, 0x80, 0],
            &[0xE0, 0, 0x80],
            &[0xD0],
            &[0xD0, 0x80],
            &[0xC0, 5, 0],
            &[0xF0, 1, 2, 3, 0xF7],
            &[0xF8, 0, 0],
            &[0xFE],
        ] {
            assert_eq!(parse(message), None, "{message:?}");
        }
        // Bytes after a whole message are left alone.
        assert_eq!(parse(&[0x90, 60, 100, 1, 2]), Some((0, LiveEvent::NoteOn { key: 60, velocity: 100.0 / 127.0 })));
    }

    #[test]
    fn every_byte_sequence_is_safe() {
        for status in 0..=255u8 {
            for first in [0u8, 1, 64, 120, 123, 127, 128, 255] {
                for second in [0u8, 1, 63, 64, 127, 128, 255] {
                    match parse(&[status, first, second]) {
                        Some((channel, LiveEvent::NoteOn { velocity, .. })) => {
                            assert!(velocity > 0.0 && velocity <= 1.0);
                            assert_eq!(channel, status & 0x0F);
                        }
                        Some((channel, LiveEvent::Controller { value, .. })) => {
                            assert!((0.0..=1.0).contains(&value));
                            assert_eq!(channel, status & 0x0F);
                        }
                        Some((channel, LiveEvent::PitchBend(value))) => {
                            assert!((-1.0..=1.0).contains(&value));
                            assert_eq!(channel, status & 0x0F);
                        }
                        Some((channel, LiveEvent::Pressure(value))) => {
                            assert!((0.0..=1.0).contains(&value));
                            assert_eq!(channel, status & 0x0F);
                        }
                        Some((channel, _)) => assert!(channel < 16),
                        None => {}
                    }
                }
            }
        }
    }

    #[test]
    fn listing_the_ports_never_fails_hard() {
        // Without a MIDI system (a container without the ALSA sequencer) this is an error, not a
        // crash; with one it lists what is there.
        match Midi::new().ports() {
            Ok(ports) => assert!(ports.iter().all(|p| !p.is_empty())),
            Err(e) => assert!(!e.is_empty()),
        }
        let input = Arc::new(LiveInput::default());
        assert!(Midi::new().open("no such port, surely", input).is_err());
    }
}
