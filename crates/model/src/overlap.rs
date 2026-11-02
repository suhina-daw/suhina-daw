// Copyright (C) 2026 The Suhina contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! Audio clips that sound together on one track: two arrangement clips whose spans overlap in
//! time. Stacking them is ordinary (both play); an ARA plug-in's first version in Suhina does not
//! edit such clips ([ADR 0033](../../docs/decisions/0033-ara-hosting.md)), so the session needs
//! to find them by their actual start and end, not by how many clips a track has.

use crate::{LaneData, Project, TrackId};

/// How far apart, in beats, a clip's end and the next clip's start may be and still only touch.
/// The arrangement's stacking layout uses the same tolerance, so clips drawn in one row never
/// overlap here: `0.1 + 0.2` ends a hair after a clip that starts at `0.3`, and that is a touch.
pub const TOUCH_TOLERANCE: f64 = 1e-9;

/// At most this many pairs are reported; a track of a thousand clips stacked on each other would
/// otherwise make half a million.
pub const MAX_AUDIO_OVERLAPS: usize = 10_000;

/// A clip on the arrangement, as `clip.move` and `project.clips` count it: the track of its lane
/// and its index there, with the lane's place in [`crate::Arrangement::lanes`].
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct OverlapClip {
    pub track: TrackId,
    pub index: usize,
    pub lane: usize,
}

/// Two audio clips that sound together on `track` from `start` to `end`, in beats. `first`
/// starts no later than `second` (by start, then by place).
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct AudioOverlap {
    pub track: TrackId,
    pub first: OverlapClip,
    pub second: OverlapClip,
    pub start: f64,
    pub end: f64,
    /// One clip lies wholly within the other (or both span the same time).
    pub contained: bool,
}

/// What [`Project::audio_overlaps`] found, and whether it stopped at [`MAX_AUDIO_OVERLAPS`].
#[derive(Debug, Clone, PartialEq, Default)]
pub struct AudioOverlaps {
    pub pairs: Vec<AudioOverlap>,
    pub truncated: bool,
}

struct Span {
    place: OverlapClip,
    start: f64,
    end: f64,
}

impl Project {
    /// Every pair of audio clips on the arrangement whose spans overlap on the track they sound
    /// on, in track order, then by start. A clip sounds on its lane's track, or on the track its
    /// content's audio lane names; a clip that plays only notes is left out, and so is one with a
    /// span that is not a finite, positive length. Clips that only touch do not overlap
    /// ([`TOUCH_TOLERANCE`]). Disabled clips count: enabling one makes no new edit to check.
    pub fn audio_overlaps(&self) -> AudioOverlaps {
        let mut by_track: Vec<(TrackId, Vec<Span>)> = Vec::new();
        for (lane_index, lane) in self.arrangement.lanes.iter().enumerate() {
            for (index, clip) in lane.clips.iter().enumerate() {
                let end = clip.start + clip.length;
                if !clip.start.is_finite() || !end.is_finite() || clip.length <= 0.0 {
                    continue;
                }
                let Some(content) = self.content(clip.content) else { continue };
                let mut sounds_on: Vec<TrackId> = Vec::new();
                for content_lane in &content.lanes {
                    if !matches!(content_lane.data, LaneData::Audio(_)) {
                        continue;
                    }
                    if let Some(track) = content_lane.target.or(lane.track)
                        && !sounds_on.contains(&track)
                    {
                        sounds_on.push(track);
                    }
                }
                let Some(place_track) = lane.track.or(sounds_on.first().copied()) else { continue };
                let place = OverlapClip { track: place_track, index, lane: lane_index };
                for track in sounds_on {
                    let span = Span { place, start: clip.start, end };
                    match by_track.iter_mut().find(|(t, _)| *t == track) {
                        Some((_, spans)) => spans.push(span),
                        None => by_track.push((track, vec![span])),
                    }
                }
            }
        }
        let order = |id: &TrackId| self.tracks.iter().position(|t| t.id == *id).unwrap_or(usize::MAX);
        by_track.sort_by_key(|(t, _)| (order(t), *t));

        let mut found = AudioOverlaps::default();
        for (track, mut spans) in by_track {
            spans.sort_by(|a, b| {
                a.start
                    .total_cmp(&b.start)
                    .then(a.place.track.cmp(&b.place.track))
                    .then(a.place.index.cmp(&b.place.index))
            });
            for (i, a) in spans.iter().enumerate() {
                // Sorted by start, so the clips after the first that starts at or past this one's
                // end cannot overlap it either.
                for b in spans.iter().skip(i + 1).take_while(|b| b.start < a.end - TOUCH_TOLERANCE) {
                    let end = a.end.min(b.end);
                    if end - b.start <= TOUCH_TOLERANCE {
                        continue;
                    }
                    if found.pairs.len() == MAX_AUDIO_OVERLAPS {
                        found.truncated = true;
                        return found;
                    }
                    let within =
                        |x: &Span, y: &Span| x.start >= y.start - TOUCH_TOLERANCE && x.end <= y.end + TOUCH_TOLERANCE;
                    found.pairs.push(AudioOverlap {
                        track,
                        first: a.place,
                        second: b.place,
                        start: b.start,
                        end,
                        contained: within(a, b) || within(b, a),
                    });
                }
            }
        }
        found
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{
        Arrangement, ArrangementLane, AudioRegion, Clip, ClipContent, ContentId, ContentLane, Extensions, Note, Track,
    };

    fn content(id: u32, audio: bool, target: Option<TrackId>) -> ClipContent {
        let data = if audio {
            LaneData::Audio(AudioRegion::new("a.wav"))
        } else {
            LaneData::Notes(vec![Note::new(0.0, 1.0, 60, 0.8)])
        };
        ClipContent {
            id: ContentId(id),
            name: format!("Clip {id}"),
            length: 4.0,
            lanes: vec![ContentLane { target, data }],
            extensions: Extensions::default(),
            scale: None,
        }
    }

    /// Tracks 1 and 2; content 1 is audio, 2 plays notes, 3 is audio that sounds on track 2.
    fn project(lanes: Vec<(u32, Vec<(f64, f64)>)>, contents: &[u32]) -> Project {
        let mut p = Project {
            tracks: vec![Track::new(TrackId(1), "One"), Track::new(TrackId(2), "Two")],
            contents: vec![content(1, true, None), content(2, false, None), content(3, true, Some(TrackId(2)))],
            arrangement: Arrangement::default(),
            ..Project::default()
        };
        let mut next = contents.iter();
        p.arrangement.lanes = lanes
            .into_iter()
            .map(|(track, spans)| ArrangementLane {
                track: Some(TrackId(track)),
                clips: spans
                    .into_iter()
                    .map(|(start, length)| Clip::new(ContentId(*next.next().unwrap_or(&1)), start, length))
                    .collect(),
                ..ArrangementLane::default()
            })
            .collect();
        p
    }

    fn audio(lanes: Vec<(u32, Vec<(f64, f64)>)>) -> Project {
        let count = lanes.iter().map(|(_, s)| s.len()).sum::<usize>();
        project(lanes, &vec![1; count])
    }

    fn pairs(p: &Project) -> Vec<(usize, usize, f64, f64, bool)> {
        p.audio_overlaps().pairs.iter().map(|o| (o.first.index, o.second.index, o.start, o.end, o.contained)).collect()
    }

    #[test]
    fn separate_clips_do_not_overlap() {
        assert!(pairs(&audio(vec![(1, vec![(0.0, 4.0), (8.0, 4.0)])])).is_empty());
    }

    #[test]
    fn a_partial_overlap_is_the_shared_span() {
        assert_eq!(pairs(&audio(vec![(1, vec![(0.0, 4.0), (3.0, 4.0)])])), vec![(0, 1, 3.0, 4.0, false)]);
    }

    #[test]
    fn a_clip_inside_another_is_contained() {
        assert_eq!(pairs(&audio(vec![(1, vec![(0.0, 8.0), (2.0, 2.0)])])), vec![(0, 1, 2.0, 4.0, true)]);
        assert_eq!(pairs(&audio(vec![(1, vec![(2.0, 2.0), (2.0, 2.0)])])), vec![(0, 1, 2.0, 4.0, true)]);
    }

    #[test]
    fn clips_that_touch_do_not_overlap() {
        assert!(pairs(&audio(vec![(1, vec![(0.0, 4.0), (4.0, 4.0)])])).is_empty());
        // 0.1 + 0.2 is 0.30000000000000004: a hair past the next start, still a touch.
        assert!(pairs(&audio(vec![(1, vec![(0.1, 0.2), (0.3, 1.0)])])).is_empty());
    }

    #[test]
    fn pairs_come_in_start_order_whatever_the_lane_order() {
        let p = audio(vec![(1, vec![(6.0, 4.0), (0.0, 4.0), (3.0, 4.0)])]);
        assert_eq!(pairs(&p), vec![(1, 2, 3.0, 4.0, false), (2, 0, 6.0, 7.0, false)]);
    }

    #[test]
    fn notes_and_other_tracks_are_left_out() {
        assert!(pairs(&project(vec![(1, vec![(0.0, 4.0), (2.0, 4.0)])], &[1, 2])).is_empty());
        assert!(pairs(&audio(vec![(1, vec![(0.0, 4.0)]), (2, vec![(2.0, 4.0)])])).is_empty());
    }

    #[test]
    fn a_clip_counts_on_the_track_its_audio_sounds_on() {
        let p = project(vec![(1, vec![(0.0, 4.0)]), (2, vec![(2.0, 4.0)])], &[3, 1]);
        let found = p.audio_overlaps().pairs;
        assert_eq!(found.len(), 1);
        assert_eq!(found[0].track, TrackId(2));
        assert_eq!(found[0].first, OverlapClip { track: TrackId(1), index: 0, lane: 0 });
        assert_eq!(found[0].second, OverlapClip { track: TrackId(2), index: 0, lane: 1 });
    }

    #[test]
    fn moving_a_clip_makes_and_removes_an_overlap() {
        let mut p = audio(vec![(1, vec![(0.0, 4.0), (8.0, 4.0)])]);
        p.arrangement.lanes[0].clips[1].start = 2.0;
        assert_eq!(pairs(&p), vec![(0, 1, 2.0, 4.0, false)]);
        p.arrangement.lanes[0].clips[1].start = 4.0;
        assert!(pairs(&p).is_empty());
    }

    #[test]
    fn bad_spans_are_skipped() {
        for (start, length) in [(f64::NAN, 4.0), (0.0, f64::INFINITY), (0.0, 0.0), (0.0, -1.0)] {
            assert!(pairs(&audio(vec![(1, vec![(0.0, 4.0), (start, length)])])).is_empty());
        }
    }

    #[test]
    fn a_pile_of_stacked_clips_stops_at_the_cap() {
        let found = audio(vec![(1, vec![(0.0, 4.0); 200])]).audio_overlaps();
        assert_eq!(found.pairs.len(), MAX_AUDIO_OVERLAPS);
        assert!(found.truncated);
        let few = audio(vec![(1, vec![(0.0, 4.0); 3])]).audio_overlaps();
        assert_eq!((few.pairs.len(), few.truncated), (3, false));
    }
}
