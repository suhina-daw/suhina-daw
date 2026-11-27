// Copyright (C) 2026 The Suhina contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! Channel-MPE output (`docs/spec/mpe.md` › Plug-in boundaries): the MIDI values of curves, the
//! prepared curves, and what a plug-in instrument is sent when notes with curves play from the
//! arrangement and from a launched clip, through loops, seeks, stops and voice stealing.

use std::sync::{Arc, Mutex};
use std::time::Duration;

use suhina_device_api::{ChannelMessage, NoteEvent};
use suhina_model::*;

use super::*;
use crate::chain_tests::{BEAT, SR, blocks, device, note, project};
use crate::{
    AudioPool, LaunchCommand, PluginProcessor, PluginSlotInfo, Plugins, RenderInputs, RenderOptions, Renderer,
};

type Seen = Arc<Mutex<Vec<(usize, NoteEvent)>>>;

/// A plug-in instrument that keeps every event it gets, at its frame in the renderer's output,
/// and plays silence.
struct Recorder {
    seen: Seen,
    frames: usize,
}

impl PluginProcessor for Recorder {
    fn process(&mut self, buffer: &mut [f32], events: &[(usize, NoteEvent)], _timeout: Duration) -> bool {
        if let Ok(mut seen) = self.seen.lock() {
            seen.extend(events.iter().map(|&(at, e)| (self.frames + at, e)));
        }
        buffer.fill(0.0);
        self.frames += buffer.len() / 2;
        true
    }
}

fn instrument() -> Device {
    let mut d = device("plug-in", &[]);
    d.kind = DeviceKind::Vst3 { id: "565354416263646566".into() };
    d
}

/// `p`'s renderer with its plug-in recording into the returned list.
fn renderer(p: &Project, options: RenderOptions) -> (Renderer, Seen) {
    let seen = Seen::default();
    let mut plugins = Plugins::new();
    for found in crate::plugin_devices(p) {
        let recorder = Recorder { seen: Arc::clone(&seen), frames: 0 };
        plugins.insert(found.key, PluginSlotInfo { instrument: true, latency: 0 }, Box::new(recorder));
    }
    let inputs = RenderInputs::from_audio(&AudioPool::new(), suhina_device_registry::factory()).with_plugins(plugins);
    (Renderer::build(p, inputs, SR, options).unwrap(), seen)
}

fn bent(time: f64, duration: f64, key: u8, points: &[(f64, f64)]) -> Note {
    let pitch = points.iter().map(|&(offset, value)| ExpressionPoint::new(offset, value)).collect();
    Note {
        expression: Some(Shared::new(NoteExpression { pitch, ..NoteExpression::default() })),
        ..note(time, duration, key)
    }
}

fn events(seen: &Seen) -> Vec<(usize, NoteEvent)> {
    seen.lock().unwrap().clone()
}

fn on(channel: u8, key: u8) -> impl Fn(&NoteEvent) -> bool {
    move |e| matches!(e, NoteEvent::Channel { channel: c, message: ChannelMessage::On { key: k, .. } } if *c == channel && *k == key)
}

fn off(channel: u8, key: u8) -> impl Fn(&NoteEvent) -> bool {
    move |e| matches!(e, NoteEvent::Channel { channel: c, message: ChannelMessage::Off { key: k, .. } } if *c == channel && *k == key)
}

fn bend(channel: u8, value: u16) -> NoteEvent {
    NoteEvent::Channel { channel, message: ChannelMessage::PitchBend(bend_event(value)) }
}

fn cc(channel: u8, number: u8, value: u8) -> NoteEvent {
    controller(channel, number, value)
}

/// The channels of the note-ons, in order.
fn note_channels(seen: &[(usize, NoteEvent)]) -> Vec<(u8, u8)> {
    seen.iter()
        .filter_map(|(_, e)| match e {
            NoteEvent::Channel { channel, message: ChannelMessage::On { key, .. } } => Some((*channel, *key)),
            _ => None,
        })
        .collect()
}

/// The MPE Configuration Message: RPN 6 on channel 1 with 15 members, then the null RPN.
const MCM: [(u8, u8); 5] = [(101, 0), (100, 6), (6, 15), (101, 127), (100, 127)];

fn configurations(seen: &[(usize, NoteEvent)]) -> usize {
    seen.windows(MCM.len())
        .filter(|w| w.iter().zip(MCM).all(|((_, e), (number, value))| *e == cc(0, number, value)))
        .count()
}

#[test]
fn bend_values_follow_the_14_bit_mapping_at_48_semitones() {
    assert_eq!(bend_value(0.0), 8192);
    assert_eq!(bend_value(48.0), 16_383);
    assert_eq!(bend_value(-48.0), 1);
    // 8192 + 12 / 48 × 8191 = 10239.75.
    assert_eq!(bend_value(12.0), 10_240);
    assert_eq!(bend_value(2.0), 8_533);
    assert_eq!(bend_value(100.0), 16_383, "past the range it stays at its end");
    assert_eq!(bend_value(-100.0), 0);
    assert_eq!(bend_value(f64::NAN), 8192);
}

#[test]
fn the_plug_in_host_turns_each_bend_back_into_its_14_bit_value() {
    // The host sends round((bend + 1) / 2 × 16383), in f32.
    for value in 0..=16_383u16 {
        let sent = ((bend_event(value) + 1.0) / 2.0 * 16_383.0).round().clamp(0.0, 16_383.0) as u16;
        assert_eq!(sent, value);
    }
    for value in 0..=127u16 {
        assert_eq!((seven_bit(value) * 127.0).round() as u16, value);
    }
}

#[test]
fn pressure_and_timbre_take_7_bit_values() {
    assert_eq!(pressure_value(0.0), 0);
    assert_eq!(pressure_value(1.0), 127);
    assert_eq!(pressure_value(0.5), 64);
    assert_eq!(timbre_value(0.5), 64, "the neutral timbre is CC74's centre");
    assert_eq!(timbre_value(0.0), 0);
    assert_eq!(timbre_value(1.0), 127);
    assert_eq!(timbre_value(0.25), 32);
    assert_eq!(timbre_value(0.75), 96);
}

#[test]
fn prepared_curves_hold_the_exact_values_at_their_points() {
    let note = bent(0.0, 2.0, 60, &[(0.5, 2.0), (1.0, 12.0), (1.5, -48.0)]);
    let mut curves = Curves::default();
    let range = curves.add(note.expression.as_ref(), 2.0);
    let changes = curves.changes.get(range.first as usize..range.end as usize).unwrap();
    // The start: the curve holds its first value before its first point, the others neutral.
    let at_start: Vec<_> = changes.iter().filter(|c| c.at == 0.0).map(|c| (c.dim, c.value)).collect();
    assert_eq!(at_start, [(Dim::Bend, 8_533), (Dim::Pressure, 0), (Dim::Timbre, 64)]);
    let bend_at = |at: f32| changes.iter().find(|c| c.dim == Dim::Bend && c.at == at).map(|c| c.value);
    assert_eq!(bend_at(1.0), Some(10_240));
    assert_eq!(bend_at(1.5), Some(1));
    // Sorted, every change a new value, nothing after the last point (the curve holds).
    assert!(changes.windows(2).all(|w| w[0].at <= w[1].at));
    let bends: Vec<_> = changes.iter().filter(|c| c.dim == Dim::Bend).collect();
    assert!(bends.windows(2).all(|w| w[0].value != w[1].value));
    assert!(bends.iter().all(|c| c.at <= 1.5));
    // Between points it moves at most every step.
    assert!(bends.windows(2).all(|w| f64::from(w[1].at - w[0].at) >= CURVE_STEP - 1e-6));
    // The same note again (a loop's next pass) is prepared once.
    let again = curves.add(note.expression.as_ref(), 2.0);
    assert_eq!(again, range);
    assert!(curves.add(None, 2.0).is_empty());
}

#[test]
fn a_curve_past_the_notes_end_is_cut_there() {
    let note = bent(0.0, 4.0, 60, &[(0.0, 0.0), (4.0, 48.0)]);
    let mut curves = Curves::default();
    let range = curves.add(note.expression.as_ref(), 1.0);
    let changes = curves.changes.get(range.first as usize..range.end as usize).unwrap();
    assert!(changes.iter().all(|c| c.at < 1.0), "nothing at or after the end");
    let last = changes.iter().rev().find(|c| c.dim == Dim::Bend).unwrap();
    // Just before beat 1, where the curve is near 12 semitones.
    assert!((10_200..10_240).contains(&last.value), "{last:?}");
}

#[test]
fn only_the_curved_notes_channel_is_bent_and_the_setup_comes_first() {
    let notes = vec![bent(0.0, 2.0, 60, &[(0.0, 2.0), (1.0, 12.0)]), note(0.0, 2.0, 64)];
    let p = project(notes, 4.0, vec![instrument()]);
    let (mut renderer, seen) = renderer(&p, RenderOptions::PLAYING);
    assert!(renderer.graph.chains.first().is_some_and(crate::chain::Chain::plays_mpe));
    blocks(&mut renderer, 3 * BEAT / 480, 480);
    let seen = events(&seen);
    // The configuration first, then the bend range and neutral controls on every member.
    assert_eq!(configurations(&seen), 1);
    // A chain's scale goes to its devices before anything else; the zone's setup is the first
    // MIDI it sends.
    let midi = seen.iter().filter(|(_, e)| matches!(e, NoteEvent::Channel { .. }));
    assert!(midi.take(MCM.len()).zip(MCM).all(|((_, e), (n, v))| *e == cc(0, n, v)), "{seen:?}");
    for channel in 1..=15 {
        let rpn = [(101, 0), (100, 0), (6, 48), (38, 0)].map(|(n, v)| cc(channel, n, v));
        let at = seen.iter().position(|(_, e)| *e == rpn[0]).unwrap();
        let got: Vec<NoteEvent> = seen.iter().skip(at).take(4).map(|(_, e)| *e).collect();
        assert_eq!(got, rpn, "channel {channel}");
    }
    // Lowest key first: two notes, two channels.
    assert_eq!(note_channels(&seen), [(1, 60), (2, 64)]);
    let first_on = seen.iter().position(|(_, e)| e.is_note_on()).unwrap();
    // The curved note's start value goes before its note-on; its point at beat 1 on its frame.
    let start = seen.iter().position(|(_, e)| *e == bend(1, 8_533)).unwrap();
    assert!(start < first_on, "{seen:?}");
    assert!(seen.contains(&(BEAT, bend(1, 10_240))), "{seen:?}");
    // The plain note's channel gets no bend after the setup.
    let after: Vec<_> = seen.iter().skip(first_on).map(|(_, e)| *e).collect();
    assert!(
        !after.iter().any(|e| matches!(e, NoteEvent::Channel { channel: 2, message: ChannelMessage::PitchBend(_) }))
    );
    assert!(
        after.iter().any(|e| matches!(e, NoteEvent::Channel { channel: 1, message: ChannelMessage::PitchBend(_) }))
    );
    // Both end on their own channels, at beat 2.
    assert!(seen.iter().any(|(at, e)| *at == 2 * BEAT && off(1, 60)(e)));
    assert!(seen.iter().any(|(at, e)| *at == 2 * BEAT && off(2, 64)(e)));
    // Nothing goes as an ordinary note.
    assert!(!seen.iter().any(|(_, e)| matches!(e, NoteEvent::On { .. } | NoteEvent::Off { .. })));
}

#[test]
fn a_track_without_curves_plays_as_before() {
    let p = project(vec![note(0.0, 1.0, 60), note(0.0, 1.0, 64)], 2.0, vec![instrument()]);
    let (mut renderer, seen) = renderer(&p, RenderOptions::PLAYING);
    assert!(!renderer.graph.chains.first().is_some_and(crate::chain::Chain::plays_mpe));
    blocks(&mut renderer, 2 * BEAT / 480, 480);
    let seen = events(&seen);
    assert!(!seen.iter().any(|(_, e)| matches!(e, NoteEvent::Channel { .. })), "{seen:?}");
    let ons: Vec<_> = seen.iter().filter(|(_, e)| matches!(e, NoteEvent::On { .. })).collect();
    assert_eq!(ons.len(), 2);
}

#[test]
fn a_built_in_instrument_ignores_the_curves() {
    let p = project(vec![bent(0.0, 1.0, 60, &[(0.0, 5.0)])], 2.0, vec![device("fm-synth", &[])]);
    let (renderer, _) = renderer(&p, RenderOptions::PLAYING);
    assert!(!renderer.graph.chains.first().is_some_and(crate::chain::Chain::plays_mpe));
}

#[test]
fn renders_send_the_same_events_whatever_the_block_size() {
    let notes = vec![bent(0.0, 2.0, 60, &[(0.0, 0.0), (2.0, 24.0)]), bent(0.5, 1.0, 67, &[(0.0, -3.0), (1.0, 3.0)])];
    let p = project(notes, 4.0, vec![instrument()]);
    let run = |block: usize| {
        let (mut renderer, seen) = renderer(&p, RenderOptions::PLAYING);
        blocks(&mut renderer, 3 * BEAT / block, block);
        events(&seen).into_iter().filter(|(_, e)| matches!(e, NoteEvent::Channel { .. })).collect::<Vec<_>>()
    };
    let reference = run(480);
    for block in [64, 160, 1_000] {
        assert_eq!(run(block), reference, "block {block}");
    }
}

#[test]
fn the_sixteenth_note_steals_the_oldest_notes_channel() {
    let mut notes: Vec<Note> = (60..76).map(|key| note(0.0, 1.0, key)).collect();
    notes[0] = bent(0.0, 1.0, 60, &[(0.0, 1.0)]);
    let p = project(notes, 2.0, vec![instrument()]);
    let (mut renderer, seen) = renderer(&p, RenderOptions::PLAYING);
    blocks(&mut renderer, 2 * BEAT / 480, 480);
    let seen = events(&seen);
    let channels = note_channels(&seen);
    let expected: Vec<(u8, u8)> = (1..=15).zip(60..75).chain([(1, 75)]).collect();
    assert_eq!(channels, expected);
    // The oldest note is released on its channel before the new one takes it, and only then.
    let released = seen.iter().position(|(_, e)| off(1, 60)(e)).unwrap();
    let taken = seen.iter().position(|(_, e)| on(1, 75)(e)).unwrap();
    assert!(released < taken);
    assert_eq!(seen.iter().filter(|(_, e)| off(1, 60)(e)).count(), 1, "not released again at its end");
    // The stolen channel goes back to neutral for the plain note.
    let reset = seen.iter().skip(released).position(|(_, e)| *e == bend(1, 8192)).map(|i| i + released);
    assert!(reset.is_some_and(|r| r < taken), "{seen:?}");
}

#[test]
fn a_free_channel_released_longest_ago_is_taken_first() {
    // Sixteen short notes one after another: each takes a channel not used yet, then the
    // sixteenth the first one again, released longest ago.
    let mut notes: Vec<Note> = (0..16).map(|i| note(f64::from(i) * 0.25, 0.125, 60 + i as u8)).collect();
    notes[0] = bent(0.0, 0.125, 60, &[(0.0, 1.0)]);
    let p = project(notes, 4.0, vec![instrument()]);
    let (mut renderer, seen) = renderer(&p, RenderOptions::PLAYING);
    blocks(&mut renderer, 4 * BEAT / 480, 480);
    let channels: Vec<u8> = note_channels(&events(&seen)).into_iter().map(|(c, _)| c).collect();
    let expected: Vec<u8> = (1..=15).chain([1]).collect();
    assert_eq!(channels, expected);
}

#[test]
fn every_loop_pass_starts_its_notes_afresh() {
    let mut p = project(vec![bent(0.0, 2.0, 60, &[(0.0, 2.0), (1.0, 12.0)])], 2.0, vec![instrument()]);
    p.transport.loop_range = Some(LoopRange { start: 0.0, end: 2.0 });
    p.transport.loop_enabled = true;
    let (mut renderer, seen) = renderer(&p, RenderOptions::PLAYING);
    // Three passes, in blocks the wraps fall inside.
    assert!((3 * 2 * BEAT).is_multiple_of(576) && !(2 * BEAT).is_multiple_of(576));
    blocks(&mut renderer, 3 * 2 * BEAT / 576, 576);
    let seen = events(&seen);
    assert_eq!(configurations(&seen), 1, "the zone is set up once");
    let starts: Vec<(usize, u8)> = seen
        .iter()
        .filter_map(|(at, e)| match e {
            NoteEvent::Channel { channel, message: ChannelMessage::On { key: 60, .. } } => Some((*at, *channel)),
            _ => None,
        })
        .collect();
    assert_eq!(starts.len(), 3, "{starts:?}");
    for pair in starts.windows(2) {
        let ((_, old), (at, channel)) = (pair[0], pair[1]);
        // The old note ends at the wrap, on its channel, before the new one starts.
        let ended = seen.iter().position(|(_, e)| off(old, 60)(e));
        let started = seen.iter().position(|(t, e)| *t == at && on(channel, 60)(e));
        assert!(ended.is_some() && ended < started, "{seen:?}");
        // A channel not used yet: its start value goes out before the note-on.
        let start_bend = seen.iter().position(|(t, e)| *t == at && *e == bend(channel, 8_533));
        assert!(start_bend.is_some() && start_bend < started, "{seen:?}");
        assert!(seen.contains(&(at + BEAT, bend(channel, 10_240))), "the point on its frame: {seen:?}");
    }
}

#[test]
fn a_seek_into_a_note_starts_it_with_the_curve_where_it_is() {
    let p = project(vec![bent(0.0, 4.0, 60, &[(0.0, 0.0), (2.0, 24.0)])], 4.0, vec![instrument()]);
    let (mut renderer, seen) = renderer(&p, RenderOptions::PLAYING);
    blocks(&mut renderer, 1, 480);
    renderer.handle(LaunchCommand::SetPosition { beat: 1.0 });
    blocks(&mut renderer, 2, 480);
    let seen = events(&seen);
    let channels = note_channels(&seen);
    assert_eq!(channels, [(1, 60), (2, 60)], "the note again, on a fresh channel");
    let restarted = seen.iter().position(|(_, e)| on(2, 60)(e)).unwrap();
    assert!(seen.iter().take(restarted).any(|(_, e)| off(1, 60)(e)), "the old note ends first");
    // At beat 1 the curve is at 12 semitones; that goes before the note-on.
    let last_bend = seen.iter().take(restarted).rev().find_map(|(_, e)| match e {
        NoteEvent::Channel { channel: 2, message: ChannelMessage::PitchBend(v) } => Some(*v),
        _ => None,
    });
    assert_eq!(last_bend, Some(bend_event(10_240)));
}

#[test]
fn a_stop_ends_the_member_notes_and_the_next_start_sets_the_zone_up_again() {
    let p = project(vec![bent(0.0, 4.0, 60, &[(0.0, 3.0)])], 4.0, vec![instrument()]);
    let (mut renderer, seen) = renderer(&p, RenderOptions::PLAYING);
    blocks(&mut renderer, 2, 480);
    renderer.handle(LaunchCommand::Stop);
    blocks(&mut renderer, 2, 480);
    let stopped = events(&seen).len();
    renderer.handle(LaunchCommand::Play);
    blocks(&mut renderer, 2, 480);
    let seen = events(&seen);
    assert!(seen.iter().take(stopped).any(|(_, e)| off(1, 60)(e)));
    for channel in 1..=15 {
        assert!(seen.iter().take(stopped).any(|(_, e)| *e == cc(channel, 123, 0)), "channel {channel}");
    }
    assert_eq!(configurations(&seen), 2, "{seen:?}");
}

#[test]
fn a_launched_clips_notes_carry_their_curves() {
    let p = project(vec![bent(0.0, 1.0, 62, &[(0.0, -12.0), (0.5, 0.0)])], 1.0, vec![instrument()]);
    let (mut renderer, seen) = renderer(&p, RenderOptions::STOPPED);
    renderer.handle(LaunchCommand::SetQuantization(crate::Quantization::None));
    renderer.handle(LaunchCommand::LaunchClip { column: 0, slot: 0 });
    // Two passes of the looping clip, a beat each.
    blocks(&mut renderer, 2 * BEAT / 480, 480);
    let seen = events(&seen);
    assert_eq!(configurations(&seen), 1);
    let starts: Vec<usize> = seen.iter().filter(|(_, e)| e.is_note_on()).map(|(at, _)| *at).collect();
    assert_eq!(starts.len(), 2, "{seen:?}");
    // -12 semitones at the start, centred half a beat later, on each pass.
    let low = bend_value(-12.0);
    for &at in &starts {
        assert!(seen.iter().any(|(t, e)| *t == at && matches!(e, NoteEvent::Channel { message: ChannelMessage::PitchBend(v), .. } if *v == bend_event(low))), "{seen:?}");
        assert!(seen.iter().any(|(t, e)| t.abs_diff(at + BEAT / 2) <= 1 && matches!(e, NoteEvent::Channel { message: ChannelMessage::PitchBend(v), .. } if *v == bend_event(8192))), "{seen:?}");
    }
}
