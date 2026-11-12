// Copyright (C) 2026 The Suhina contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! Running audio through a plug-in offline, as fast as it goes: the same host process and
//! link as playback, but each block waits for the plug-in instead of a real-time deadline.

use std::time::Duration;

use suhina_ipc::Outcome;

use crate::{Event, Health, Launch, Plugin, PluginError, SavedState};

/// Frames per block offline.
pub const BLOCK_FRAMES: u32 = 512;
/// How long one block may take offline before the plug-in counts as stuck.
pub const BLOCK_TIMEOUT: Duration = Duration::from_secs(10);

/// What an offline run gave.
#[derive(Debug, Clone, PartialEq)]
pub struct Rendered {
    /// Interleaved stereo: the input's length plus the tail, lined up with the input (the
    /// plug-in's latency taken out).
    pub samples: Vec<f32>,
    /// The latency the plug-in reported, in frames.
    pub latency: u32,
    /// Why the plug-in did not take the state it was given, if it did not; it ran with its own
    /// default settings then.
    pub state_refused: Option<String>,
}

/// Runs interleaved stereo `input` through the plug-in `launch` names, then `tail_frames` of
/// silence so effects ring out. `launch.channels` and `launch.max_frames` are set here.
pub fn render(launch: Launch, input: &[f32], tail_frames: usize) -> Result<Rendered, PluginError> {
    render_with_state(launch, None, input, tail_frames)
}

/// [`render`] with the plug-in set to `state` first, as a project saved it. A plug-in that
/// refuses the state still runs, with its defaults, and [`Rendered::state_refused`] says why.
pub fn render_with_state(
    launch: Launch,
    state: Option<SavedState>,
    input: &[f32],
    tail_frames: usize,
) -> Result<Rendered, PluginError> {
    render_with_events(launch, state, input, &[], tail_frames)
}

/// [`render_with_state`] with notes and controllers for an instrument: `events`, sorted by
/// frame, each frame counted from the start of `input`.
pub fn render_with_events(
    mut launch: Launch,
    state: Option<SavedState>,
    input: &[f32],
    events: &[Event],
    tail_frames: usize,
) -> Result<Rendered, PluginError> {
    launch.channels = 2;
    launch.max_frames = BLOCK_FRAMES;
    let (mut plugin, mut audio) = Plugin::start(launch)?;
    let mut state_refused = None;
    if let Some(state) = state {
        match plugin.load_state(state) {
            Ok(()) => {}
            Err(PluginError::Host(why)) => state_refused = Some(why),
            Err(other) => return Err(other),
        }
    }
    let rendered = render_started(&mut plugin, &mut audio, input, events, tail_frames)?;
    drop(plugin);
    Ok(Rendered { state_refused, ..rendered })
}

/// [`render_with_events`] through a plug-in already started (as a project's offline render
/// starts it) and given its state, in blocks of [`BLOCK_FRAMES`] (its `max_frames` must be at
/// least that). [`Rendered::state_refused`] is `None`: the caller gave the state.
pub fn render_started(
    plugin: &mut Plugin,
    audio: &mut crate::PluginAudio,
    input: &[f32],
    events: &[Event],
    tail_frames: usize,
) -> Result<Rendered, PluginError> {
    let latency = plugin.latency();
    let input_frames = input.len() / 2;
    let wanted = input_frames + tail_frames;
    // Run long enough that the delayed output covers what is wanted.
    let total = wanted + latency as usize;
    let block = BLOCK_FRAMES as usize;
    let mut output = Vec::with_capacity(total * 2 + block * 2);
    let mut chunk_in = vec![0.0f32; block * 2];
    let mut chunk_out = vec![0.0f32; block * 2];
    let mut frame = 0;
    let mut next_event = 0;
    let mut block_events: Vec<Event> = Vec::with_capacity(crate::MAX_EVENTS);
    while frame < total {
        let frames = block.min(total - frame);
        let samples = frames * 2;
        chunk_in.fill(0.0);
        let from = (frame * 2).min(input.len());
        let to = ((frame + frames) * 2).min(input.len());
        if let (Some(source), Some(target)) = (input.get(from..to), chunk_in.get_mut(..to - from)) {
            target.copy_from_slice(source);
        }
        let (Some(block_in), Some(block_out)) = (chunk_in.get(..samples), chunk_out.get_mut(..samples)) else {
            break;
        };
        // The events in this block, their frames counted from its start.
        block_events.clear();
        let end = (frame + frames) as u64;
        while let Some(event) = events.get(next_event).filter(|e| u64::from(e.frame) < end) {
            block_events.push(Event { frame: (u64::from(event.frame).saturating_sub(frame as u64)) as u32, ..*event });
            next_event += 1;
        }
        match audio.process_with_events(block_in, &block_events, block_out, BLOCK_TIMEOUT) {
            Outcome::Done => output.extend_from_slice(block_out),
            // A host that died with the block (a plug-in that crashed) is told apart from one
            // that is stuck in it.
            Outcome::Missed | Outcome::Busy if plugin.health() == Health::Crashed => {
                return Err(PluginError::Protocol("the plug-in crashed while running a block offline".into()));
            }
            Outcome::Missed | Outcome::Busy => {
                return Err(PluginError::Timeout("a block offline (the plug-in stopped answering)"));
            }
        }
        frame += frames;
    }
    let skip = (latency as usize * 2).min(output.len());
    let mut samples = output.split_off(skip);
    samples.truncate(wanted * 2);
    samples.resize(wanted * 2, 0.0);
    Ok(Rendered { samples, latency, state_refused: None })
}
