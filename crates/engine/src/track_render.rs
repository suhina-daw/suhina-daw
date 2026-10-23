// Copyright (C) 2026 The Suhina contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! One track's arrangement clips rendered on their own, as they sound before the track's mixer
//! strip, for `clip.consolidate` (`docs/spec/arrangement-editing.md` › Consolidating). The same
//! renderer as playback and offline renders.

use crate::{RenderInputs, RenderOptions};
use suhina_model::{
    Arrangement, Channel, ChannelId, ChannelRole, ClipContent, Extensions, Project, Track, TrackId, Transport,
};

use crate::{AUDIO_HEADROOM, AudioPool, MAX_RENDER_SECONDS, RenderError, Renderer, track_gains};

/// Frames rendered per call to the renderer.
const BLOCK_FRAMES: usize = 512;

/// Renders the first `frames` of `track`'s arrangement lane into interleaved stereo: each audio
/// clip with its own gain, fades and warping, at unity gain and centred, with no devices, no
/// other track and no headroom, so a full-scale file plays back at full scale. Notes on the lane
/// play the engine's test voice; `clip.consolidate` renders only audio lanes.
pub fn render_track(
    project: &Project,
    audio: &AudioPool,
    track: TrackId,
    sample_rate: u32,
    frames: usize,
) -> Result<Vec<f32>, RenderError> {
    let seconds = frames as f64 / f64::from(sample_rate.max(1));
    if seconds > MAX_RENDER_SECONDS {
        return Err(RenderError::TooLong { seconds });
    }
    let alone = alone(project, track);
    let (left, right) = track_gains(&alone, track).unwrap_or((0.0, 0.0));
    let mut renderer = Renderer::build(&alone, RenderInputs::from_audio(audio), sample_rate, RenderOptions::PLAYING)?;
    renderer.set_offline(true);
    let mut out = vec![0.0f32; frames.saturating_mul(2)];
    for block in out.chunks_mut(BLOCK_FRAMES * 2) {
        renderer.process(block);
    }
    // Undo the centred pan and the headroom the mix gives every audio clip.
    let undo = |gain: f32| if gain > 0.0 { 1.0 / (gain * AUDIO_HEADROOM) } else { 0.0 };
    let (left, right) = (undo(left), undo(right));
    for [l, r] in out.as_chunks_mut::<2>().0 {
        *l *= left;
        *r *= right;
    }
    Ok(out)
}

/// The project with only `track` and its arrangement lane, playing straight to the output
/// through a unity channel.
fn alone(project: &Project, track: TrackId) -> Project {
    let channel = ChannelId(0);
    let source = project.track(track);
    Project {
        // The clips keep their ids, so the counter stays past them.
        next_id: project.next_id,
        // The range is rendered straight through: the arrangement's loop region does not wrap it.
        transport: Transport { loop_enabled: false, ..project.transport.clone() },
        tracks: vec![Track {
            id: track,
            name: source.map(|t| t.name.clone()).unwrap_or_default(),
            color: None,
            content: source.map(|t| t.content).unwrap_or_default(),
            channel: Some(channel),
            parent: None,
            extensions: Extensions::default(),
            audio_input: Default::default(),
            midi_input: Default::default(),
        }],
        channels: vec![Channel {
            id: channel,
            name: String::new(),
            role: ChannelRole::Regular,
            volume: 1.0,
            pan: 0.0,
            mute: false,
            solo: false,
            cue: false,
            output: None,
            sends: Vec::new(),
            devices: Vec::new(),
            extensions: Extensions::default(),
            automation: Vec::new(),
        }],
        // Lanes that play on another track (a shared pattern) are left out with that track.
        contents: project
            .contents
            .iter()
            .map(|c| ClipContent {
                lanes: c.lanes.iter().filter(|l| l.target.is_none_or(|t| t == track)).cloned().collect(),
                ..c.clone()
            })
            .collect(),
        arrangement: Arrangement {
            lanes: project.arrangement.lanes.iter().filter(|l| l.track == Some(track)).cloned().collect(),
            markers: Vec::new(),
        },
        launcher: Default::default(),
        extensions: Extensions::default(),
        default_scale: None,
    }
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;

    use suhina_model::{AudioRegion, ContentId, ContentLane, LaneData};

    use super::*;
    use crate::AudioData;
    use crate::render_offline_from;

    fn region(file: &str) -> AudioRegion {
        AudioRegion {
            sample_rate: Some(48_000),
            channels: Some(1),
            duration_seconds: Some(1.0),
            gain: 0.5,
            ..AudioRegion::new(file)
        }
    }

    fn project() -> (Project, AudioPool) {
        let mut audio = AudioPool::new();
        let samples: Vec<f32> = (0..48_000).map(|i| ((i as f32) * 0.01).sin() * 0.8).collect();
        audio.files.insert("a.wav".into(), Arc::new(AudioData { sample_rate: 48_000, channels: 1, samples }));
        let track = TrackId(3);
        let project = Project {
            tracks: vec![Track {
                content: suhina_model::ContentKinds { notes: false, audio: true },
                channel: Some(ChannelId(7)),
                ..Track::new(track, "Audio")
            }],
            channels: vec![Channel { volume: 0.3, pan: -0.5, ..Channel::new(ChannelId(7), "Audio") }],
            contents: vec![ClipContent {
                id: ContentId(1),
                name: "a".into(),
                length: 2.0,
                lanes: vec![ContentLane { target: None, data: LaneData::Audio(region("a.wav")) }],
                extensions: Extensions::default(),
                scale: None,
            }],
            arrangement: Arrangement {
                lanes: vec![suhina_model::ArrangementLane {
                    id: Default::default(),
                    name: "Audio".into(),
                    track: Some(track),
                    clips: vec![suhina_model::Clip {
                        offset: 0.25,
                        fade_in: 0.25,
                        ..suhina_model::Clip::new(ContentId(1), 0.5, 1.0)
                    }],
                }],
                markers: Vec::new(),
            },
            ..Default::default()
        };
        (project, audio)
    }

    #[test]
    fn a_track_renders_without_its_channel_and_the_mix_headroom() {
        let (project, audio) = project();
        let frames = 48_000;
        let alone = render_track(&project, &audio, TrackId(3), 48_000, frames).unwrap_or_default();
        assert_eq!(alone.len(), frames * 2);
        // The mix renders the same clip through its channel: scaled by its gains and headroom.
        let mixed =
            render_offline_from(&project, RenderInputs::from_audio(&audio), 48_000, 512, None).unwrap_or_default();
        let (l, r) = track_gains(&project, TrackId(3)).unwrap_or((0.0, 0.0));
        let peak = alone.iter().fold(0.0f32, |m, s| m.max(s.abs()));
        assert!(peak > 0.3, "the clip is heard at its own level, peak {peak}");
        for (frame, ([al, ar], [ml, mr])) in alone.as_chunks::<2>().0.iter().zip(mixed.as_chunks::<2>().0).enumerate() {
            let expected = (al * l * AUDIO_HEADROOM, ar * r * AUDIO_HEADROOM);
            assert!((expected.0 - ml).abs() < 1e-5 && (expected.1 - mr).abs() < 1e-5, "frame {frame}");
        }
        // Left and right are the same: centred.
        assert!(alone.as_chunks::<2>().0.iter().all(|[l, r]| (l - r).abs() < 1e-6));
    }

    #[test]
    fn the_arrangement_loop_does_not_wrap_a_track_render() {
        let (mut project, audio) = project();
        let frames = 48_000;
        let straight = render_track(&project, &audio, TrackId(3), 48_000, frames).unwrap_or_default();
        // A loop over the first half beat, before the clip starts at beat 0.5.
        project.transport.loop_range = Some(suhina_model::LoopRange { start: 0.0, end: 0.5 });
        project.transport.loop_enabled = true;
        let looped = render_track(&project, &audio, TrackId(3), 48_000, frames).unwrap_or_default();
        assert!(straight.iter().any(|s| s.abs() > 0.3), "the clip is heard");
        assert_eq!(straight, looped);
    }

    #[test]
    fn a_render_longer_than_the_limit_is_refused() {
        let (project, audio) = project();
        let too_long = (MAX_RENDER_SECONDS as usize + 1) * 8_000;
        assert!(matches!(
            render_track(&project, &audio, TrackId(3), 8_000, too_long),
            Err(RenderError::TooLong { .. })
        ));
    }
}
