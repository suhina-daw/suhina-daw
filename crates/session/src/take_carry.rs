// Copyright (C) 2026 The Suhina contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! What a part of a comp takes along when it moves or is copied (#1078, `docs/spec/recording.md` ›
//! Take lanes): what its track's takes hold under it goes where it goes. A move takes those pieces
//! out of their lanes and a copy leaves them there as well. What is already at the spot where a piece
//! lands (the same content, lined up with the beats the same way) is the same audio, so it is not
//! added twice. The rest lands on the same lanes when those are free there, and replaces nothing:
//! where a lane already holds something else, it gets new lanes, as many as it fills.

use suhina_model::{Clip, Project, TakeLane, TrackId};

use crate::commands::{content_at, make_room};

/// How far apart, in beats, two clips' alignments with their content may be and still be one.
const ALIGNED: f64 = 1e-6;

/// A part of a comp that moves or is copied: the track it leaves and the one it lands on, the beats
/// it covered, and how far it goes.
#[derive(Debug, Clone, Copy)]
pub(crate) struct Span {
    from: TrackId,
    to: TrackId,
    start: f64,
    end: f64,
    by: f64,
    content: suhina_model::ContentId,
}

impl Span {
    /// Whether this is the span of `clip`, not moved yet.
    pub(crate) fn is_of(&self, clip: &Clip) -> bool {
        self.content == clip.content && self.start == clip.start && self.end == clip.start + clip.length
    }
}

/// Each take's index in `arrangement.takes` and its pieces going along.
type Carried = Vec<(usize, Vec<Clip>)>;

/// `clip`'s span, going `by` beats from `from` to `to`, when it is a part of a take on `from`; a
/// clip of no take takes nothing along.
pub(crate) fn span_of_part(p: &Project, from: TrackId, to: TrackId, clip: &Clip, by: f64) -> Option<Span> {
    p.comp_source(from, clip)?;
    Some(Span { from, to, start: clip.start, end: clip.start + clip.length, by, content: clip.content })
}

/// Takes along what the takes hold under `spans`. Answers whether a take lane left with nothing
/// went, as the indices of the takes then change.
pub(crate) fn carry(p: &mut Project, spans: &[Span], copy: bool) -> bool {
    let mut groups: Vec<(TrackId, TrackId)> = spans.iter().map(|s| (s.from, s.to)).collect();
    groups.sort_by_key(|(from, to)| (from.0, to.0));
    groups.dedup();
    // Worked out before anything changes: each group's takes and their pieces.
    let cut: Vec<((TrackId, TrackId), Carried)> = groups
        .iter()
        .map(|&group| {
            let mine: Vec<&Span> = spans.iter().filter(|s| (s.from, s.to) == group).collect();
            let pieces = p
                .arrangement
                .takes
                .iter()
                .enumerate()
                .filter(|(_, take)| take.track == group.0)
                .map(|(index, take)| (index, pieces_under(take, &mine)))
                .filter(|(_, pieces)| !pieces.is_empty())
                .collect();
            (group, pieces)
        })
        .collect();
    if !copy {
        for span in spans {
            for take in p.arrangement.takes.iter_mut().filter(|t| t.track == span.from) {
                make_room(&mut take.clips, span.start, span.end);
            }
        }
    }
    for ((from, to), pieces) in cut {
        place(p, from, to, pieces);
    }
    let mut emptied = false;
    if !copy {
        // Only those a part took everything from: a take has clips until then.
        let tracks: Vec<TrackId> = spans.iter().map(|s| s.from).collect();
        let before = p.arrangement.takes.len();
        p.arrangement.takes.retain(|t| !t.clips.is_empty() || !tracks.contains(&t.track));
        emptied = p.arrangement.takes.len() != before;
    }
    emptied
}

/// What `take` holds under `spans`, cut to them and moved as far as each goes.
fn pieces_under(take: &TakeLane, spans: &[&Span]) -> Vec<Clip> {
    let mut out = Vec::new();
    for span in spans {
        for clip in &take.clips {
            let (start, end) = (clip.start.max(span.start), (clip.start + clip.length).min(span.end));
            if end - start < 1e-9 {
                continue;
            }
            out.push(Clip {
                start: (start + span.by).max(0.0),
                length: end - start,
                offset: content_at(clip, start - clip.start, 1.0),
                fade_in: 0.0,
                fade_out: 0.0,
                ..clip.clone()
            });
        }
    }
    out
}

/// What of `piece` is not on `track`'s takes yet: a clip of the same content lined up the same way
/// plays the same audio at the same place, so that much of the piece is there already.
fn not_there_yet(p: &Project, track: TrackId, piece: Clip) -> Vec<Clip> {
    let lined_up = piece.offset - piece.start;
    let mut rest = vec![piece];
    let on_track = p.arrangement.takes.iter().filter(|t| t.track == track).flat_map(|t| &t.clips);
    for clip in on_track {
        let same = rest.first().is_some_and(|r| r.content == clip.content);
        if same && (clip.offset - clip.start - lined_up).abs() < ALIGNED {
            make_room(&mut rest, clip.start, clip.start + clip.length);
        }
    }
    rest
}

/// Puts `pieces` on `to`, leaving out what is there already: on the lanes they belong on when all
/// of those are free where they land, or else on new lanes, one for each take they came from.
fn place(p: &mut Project, from: TrackId, to: TrackId, pieces: Carried) {
    let pieces: Carried = pieces
        .into_iter()
        .map(|(index, clips)| {
            let rest: Vec<Clip> = clips.into_iter().flat_map(|piece| not_there_yet(p, to, piece)).collect();
            (index, rest)
        })
        .filter(|(_, clips)| !clips.is_empty())
        .collect();
    let on_target: Vec<usize> =
        (0..p.arrangement.takes.len()).filter(|&i| p.arrangement.takes.get(i).is_some_and(|t| t.track == to)).collect();
    let target = |at: usize, index: usize| if from == to { Some(index) } else { on_target.get(at).copied() };
    let free = pieces.iter().enumerate().all(|(at, (index, clips))| {
        target(at, *index).and_then(|t| p.arrangement.takes.get(t)).is_some_and(|take| {
            clips.iter().all(|piece| {
                take.clips
                    .iter()
                    .all(|c| c.start + c.length <= piece.start + 1e-9 || piece.start + piece.length <= c.start + 1e-9)
            })
        })
    });
    let targets: Vec<Option<usize>> =
        pieces.iter().enumerate().map(|(at, (index, _))| target(at, *index).filter(|_| free)).collect();
    for ((index, clips), target) in pieces.into_iter().zip(targets) {
        match target.and_then(|t| p.arrangement.takes.get_mut(t)) {
            Some(take) => {
                take.clips.extend(clips);
                take.clips.sort_by(|a, b| a.start.total_cmp(&b.start));
            }
            None => {
                let color = p.arrangement.takes.get(index).and_then(|t| t.color);
                let on_track = p.arrangement.takes.iter().filter(|t| t.track == to);
                let number = on_track.filter_map(TakeLane::name_number).max().unwrap_or(0) + 1;
                let name = format!("Take {number}");
                p.arrangement.takes.push(TakeLane { track: to, name, clips, color });
            }
        }
    }
}
