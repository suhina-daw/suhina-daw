// Copyright (C) 2026 The Suhina contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! Track names with the track's number in them (#946). A `#` in a track's name that no digit
//! follows stands for the track's number, its place among the tracks other than returns and Main,
//! counted from 1: "# Bass" reads "2 Bass" on the second track and follows the track when tracks
//! move. New tracks are named "# MIDI" and "# Audio". A name without such a `#` reads as written.

use crate::{ChannelRole, Project, Track, TrackId};

/// The name a new track of the kind `kind` ("MIDI", "Audio") is given: its number and the kind.
pub fn new_track_name(kind: &str) -> String {
    format!("# {kind}")
}

/// Whether `name` holds a `#` that stands for the track's number (one no digit follows).
pub fn has_number_mark(name: &str) -> bool {
    number_marks(name).next().is_some()
}

/// `name` as the views show it: each `#` that no digit follows replaced by `number`.
pub fn shown_with_number(name: &str, number: usize) -> String {
    let mut out = String::with_capacity(name.len() + 2);
    let mut last = 0;
    for at in number_marks(name) {
        out.push_str(name.get(last..at).unwrap_or_default());
        out.push_str(&number.to_string());
        last = at + 1;
    }
    out.push_str(name.get(last..).unwrap_or_default());
    out
}

/// The byte positions of the `#`s in `name` that no digit follows.
fn number_marks(name: &str) -> impl Iterator<Item = usize> + '_ {
    let bytes = name.as_bytes();
    bytes
        .iter()
        .enumerate()
        .filter(move |&(i, &b)| b == b'#' && !bytes.get(i + 1).is_some_and(u8::is_ascii_digit))
        .map(|(i, _)| i)
}

impl Project {
    /// `track`'s number: its place among the tracks other than returns and Main, from 1, as the
    /// headers show it; `None` for a return, Main or a track the project lacks.
    pub fn track_number(&self, track: TrackId) -> Option<usize> {
        let unnumbered = |t: &Track| {
            t.channel
                .and_then(|c| self.channel(c))
                .is_some_and(|c| matches!(c.role, ChannelRole::Effect | ChannelRole::Master))
        };
        let mut number = 0;
        for t in &self.tracks {
            if unnumbered(t) {
                continue;
            }
            number += 1;
            if t.id == track {
                return Some(number);
            }
        }
        None
    }

    /// `track`'s name as the views show it, its `#` replaced by its number
    /// ([`Project::track_number`]); the name as written for a return or a missing number.
    pub fn shown_track_name(&self, track: TrackId) -> String {
        let Some(t) = self.track(track) else { return String::new() };
        match self.track_number(track) {
            Some(number) => shown_with_number(&t.name, number),
            None => t.name.clone(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_hash_no_digit_follows_reads_as_the_number() {
        assert_eq!(shown_with_number("# Bass", 2), "2 Bass");
        assert_eq!(shown_with_number("Bass #", 12), "Bass 12");
        assert_eq!(shown_with_number("Bass 1 #2", 5), "Bass 1 #2");
        assert_eq!(shown_with_number("3 Bass", 5), "3 Bass");
        assert_eq!(shown_with_number("#", 1), "1");
        assert!(has_number_mark("# MIDI") && !has_number_mark("Bass #2") && !has_number_mark("Bass"));
        assert_eq!(new_track_name("Audio"), "# Audio");
    }
}
