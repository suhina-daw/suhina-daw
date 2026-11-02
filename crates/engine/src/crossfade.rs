// Copyright (C) 2026 The Suhina contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! The crossfade at a cut between two parts of a comp that come from different takes (#989):
//! the part before the cut plays on past it, the part after it starts before it, and each fades
//! over the span around the cut, so the cut never clicks. Worked out from the project when a
//! graph is built, so it follows every trim, swap, split and undo; its length is the parts' own
//! fades at the cut, or the default ([`Project::comp_crossfade_beats`], #1078).

use std::borrow::Cow;

use suhina_model::{ArrangementLane, Clip, FadeCurve, FadeShape, Project};

/// How close two clips' edges must be, in beats, to meet at a cut.
const MEETS: f64 = 1e-6;

/// `clip`, one of `lane`'s, as it plays at `bpm`: with its crossfades at the cuts it shares with
/// parts of other takes, or as it is.
pub(crate) fn crossfaded<'a>(project: &Project, lane: &ArrangementLane, clip: &'a Clip, bpm: f64) -> Cow<'a, Clip> {
    let Some(track) = lane.track else { return Cow::Borrowed(clip) };
    // ARA clips are played by their plug-in, and their overlaps are refused (ADR 0033).
    if clip.ara || bpm <= 0.0 || !project.arrangement.takes.iter().any(|t| t.track == track) {
        return Cow::Borrowed(clip);
    }
    let Some(source) = project.comp_source(track, clip) else { return Cow::Borrowed(clip) };
    let end = clip.start + clip.length;
    let other_take = |c: &Clip| project.comp_source(track, c).is_some_and(|s| s != source);
    let before = lane.clips.iter().find(|c| (c.start + c.length - clip.start).abs() < MEETS && other_take(c));
    let after = lane.clips.iter().find(|c| (c.start - end).abs() < MEETS && other_take(c));
    if before.is_none() && after.is_none() {
        return Cow::Borrowed(clip);
    }
    let content = project.content(clip.content).map_or(f64::INFINITY, |c| c.length);
    let mut out = clip.clone();
    let equal = FadeCurve { shape: FadeShape::EqualPower, curve: 0.0 };
    if let Some(left) = before {
        // Half of it before the cut: no earlier than the content's start, or the song's.
        let half = Project::comp_crossfade_beats(left, clip, bpm) / 2.0;
        let lead = half.min(clip.offset).min(clip.start).max(0.0);
        out.start -= lead;
        out.offset -= lead;
        out.length += lead;
        out.fade_in = 2.0 * lead;
        out.fade_in_curve = equal;
    }
    if let Some(right) = after {
        let half = Project::comp_crossfade_beats(clip, right, bpm) / 2.0;
        let tail = if clip.loop_range.is_some() { half } else { half.min(content - (clip.offset + clip.length)) };
        let tail = tail.max(0.0);
        out.length += tail;
        out.fade_out = 2.0 * tail;
        out.fade_out_curve = equal;
    }
    out.fit_fades();
    Cow::Owned(out)
}

#[cfg(test)]
mod tests {
    use super::*;
    use suhina_model::{ArrangementLane, ContentId, LaneId, TakeLane, TrackId};

    /// Parts of takes 1 and 2 of one 24-beat file, meeting at beat 4, and a third part of take
    /// 2 right after the second.
    fn project() -> Project {
        let track = TrackId(1);
        let mut p = Project::default();
        let pass = |offset: f64| Clip { offset, ..Clip::new(ContentId(1), 0.0, 8.0) };
        p.arrangement.takes = vec![
            TakeLane { track, name: "Take 1".into(), clips: vec![pass(0.0)], color: None },
            TakeLane { track, name: "Take 2".into(), clips: vec![pass(8.0)], color: None },
        ];
        p.contents.push(suhina_model::ClipContent {
            id: ContentId(1),
            name: "take".into(),
            length: 24.0,
            lanes: Vec::new(),
            extensions: Default::default(),
            scale: None,
        });
        let part = |start: f64, length: f64, offset: f64| Clip { offset, ..Clip::new(ContentId(1), start, length) };
        p.arrangement.lanes = vec![ArrangementLane {
            id: LaneId(1),
            name: String::new(),
            track: Some(track),
            clips: vec![part(0.0, 4.0, 0.0), part(4.0, 2.0, 12.0), part(6.0, 2.0, 14.0)],
        }];
        p
    }

    #[test]
    fn parts_of_different_takes_crossfade_at_their_cut_and_one_takes_parts_do_not() {
        let p = project();
        let lane = &p.arrangement.lanes[0];
        // 120 bpm: 5 ms is a hundredth of a beat.
        let beats = 0.01;
        let first = crossfaded(&p, lane, &lane.clips[0], 120.0);
        assert!((first.length - (4.0 + beats)).abs() < 1e-9, "plays past the cut: {}", first.length);
        assert!((first.fade_out - 2.0 * beats).abs() < 1e-9);
        assert_eq!(first.fade_out_curve.shape, FadeShape::EqualPower);
        assert_eq!(first.start, 0.0, "nothing before it");
        let second = crossfaded(&p, lane, &lane.clips[1], 120.0);
        assert!((second.start - (4.0 - beats)).abs() < 1e-9, "starts before the cut: {}", second.start);
        assert!((second.offset - (12.0 - beats)).abs() < 1e-9, "where its take was then");
        assert!((second.fade_in - 2.0 * beats).abs() < 1e-9);
        assert!((second.length - (2.0 + beats)).abs() < 1e-9, "nothing past its end: the next is its own take");
        // The third part follows a part of its own take: no crossfade there.
        assert!(matches!(crossfaded(&p, lane, &lane.clips[2], 120.0), Cow::Borrowed(_)));
    }

    /// Rendered: around the cut both takes sound, each through its fade; away from it each plays
    /// alone.
    #[test]
    fn the_parts_fades_at_a_cut_set_the_crossfades_length() {
        let mut p = project();
        p.arrangement.lanes[0].clips[0].fade_out = 1.0;
        let lane = p.arrangement.lanes[0].clone();
        let first = crossfaded(&p, &lane, &lane.clips[0], 120.0);
        assert_eq!((first.length, first.fade_out), (4.5, 1.0));
        let second = crossfaded(&p, &lane, &lane.clips[1], 120.0);
        assert_eq!((second.start, second.fade_in), (3.5, 1.0), "the other part uses the same length");
    }

    #[test]
    fn the_crossfade_is_heard_at_the_cut() {
        use crate::{AudioData, AudioPool, RenderInputs, render_offline_from};
        const SR: u32 = 48_000;
        let mut p = project();
        p.transport.tempo_bpm = 120.0;
        let track = TrackId(1);
        p.tracks.push(suhina_model::Track {
            content: suhina_model::ContentKinds { notes: false, audio: true },
            channel: Some(suhina_model::ChannelId(1)),
            ..suhina_model::Track::new(track, "Vox")
        });
        p.channels.push(suhina_model::Channel::new(suhina_model::ChannelId(1), "Vox"));
        p.contents[0].lanes = vec![suhina_model::ContentLane {
            target: None,
            data: suhina_model::LaneData::Audio(suhina_model::AudioRegion::new("t.wav")),
        }];
        // Only the first two parts: 0.2 from take 1, then 0.6 from take 2.
        p.arrangement.lanes[0].clips.truncate(2);
        p.assign_ids().unwrap();
        let beat = SR as usize / 2;
        let samples: Vec<f32> = (0..beat * 24).map(|i| if i < beat * 8 { 0.2 } else { 0.6 }).collect();
        let mut pool = AudioPool::new();
        pool.files.insert("t.wav".into(), std::sync::Arc::new(AudioData { sample_rate: SR, channels: 1, samples }));
        let out = render_offline_from(&p, RenderInputs::from_audio(&pool), SR, 512, None).unwrap();
        let left = |frame: usize| out.get(frame * 2).copied().unwrap_or(0.0);
        let take_1 = left(beat * 2);
        let take_2 = left(beat * 5);
        assert!((take_2 / take_1 - 3.0).abs() < 1e-3, "{take_1} {take_2}");
        // At the cut both play through equal-power fades, each at about 0.707.
        let cut = left(beat * 4);
        let expected = (take_1 + take_2) * std::f32::consts::FRAC_1_SQRT_2;
        assert!((cut - expected).abs() < 0.02 * expected, "{cut} vs {expected}");
        // No step between neighbouring frames anywhere around it.
        let step = (beat * 4 - 400..beat * 4 + 400).map(|f| (left(f + 1) - left(f)).abs()).fold(0.0f32, f32::max);
        assert!(step < (take_2 - take_1) / 20.0, "{step}");
    }
}
