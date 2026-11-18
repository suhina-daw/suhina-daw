// Copyright (C) 2026 The Suhina contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! The texts a frame cuts off (#1548): shortened with "…" to fit, wrapped onto a line of their own
//! where they have no line break, or painted partly outside their clip. Read from the frame's
//! shapes, so it finds a cut whatever code painted the text.
//!
//! A text painted twice at the same place, each copy clipped to one side (a value over a fill, in
//! two colours), is one text whose clips are joined: it is cut only where neither copy shows it.

use std::collections::HashMap;
use std::fmt;

use egui::Rect;
use egui::epaint::{ClippedShape, Shape, TextShape};

/// How a text is cut.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub enum How {
    /// Shortened with "…" to fit its width.
    Shortened,
    /// Broken onto another line where it has no line break of its own.
    Wrapped,
    /// Partly outside the clip it is painted in.
    Clipped,
}

/// A text a frame cuts off.
#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub struct Cut {
    pub how: How,
    /// The whole text.
    pub text: String,
    /// What shows of it: the glyphs laid out, with the "…" of a shortened one.
    pub shown: String,
}

impl fmt::Display for Cut {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self.how {
            How::Shortened => write!(f, "shortened {:?} as {:?}", self.text, self.shown),
            How::Wrapped => write!(f, "wrapped {:?}", self.text),
            How::Clipped => write!(f, "clipped {:?}", self.text),
        }
    }
}

/// A text painted in a frame: its shape, and every clip a copy of it is painted in.
struct Painted<'a> {
    shape: &'a TextShape,
    clip: Rect,
}

/// Where a text is painted and what it says: two shapes with the same key are copies of one text,
/// in whatever colours.
type Key<'a> = ([u32; 2], [u32; 4], &'a str);

fn key(text: &TextShape) -> Key<'_> {
    let r = text.galley.rect;
    (
        [text.pos.x.to_bits(), text.pos.y.to_bits()],
        [r.min.x.to_bits(), r.min.y.to_bits(), r.max.x.to_bits(), r.max.y.to_bits()],
        text.galley.job.text.as_str(),
    )
}

/// The texts painted in a frame, in paint order, each once.
#[derive(Default)]
struct Frame<'a> {
    painted: Vec<Painted<'a>>,
    at: HashMap<Key<'a>, usize>,
}

impl<'a> Frame<'a> {
    /// Every text shape in `shape`, with the clip it is painted in.
    fn collect(&mut self, shape: &'a Shape, clip: Rect) {
        match shape {
            Shape::Vec(shapes) => shapes.iter().for_each(|s| self.collect(s, clip)),
            Shape::Text(text) => {
                let rect = text.visual_bounding_rect();
                // A text with no glyphs, or one wholly outside its clip, shows nothing to cut.
                if !rect.is_positive() || !clip.intersects(rect) {
                    return;
                }
                let next = self.painted.len();
                let at = *self.at.entry(key(text)).or_insert(next);
                match self.painted.get_mut(at) {
                    Some(copy) => copy.clip = copy.clip.union(clip),
                    None => self.painted.push(Painted { shape: text, clip }),
                }
            }
            _ => {}
        }
    }
}

/// The texts `shapes` cut off, in paint order, each once.
pub fn texts(shapes: &[ClippedShape]) -> Vec<Cut> {
    let mut frame = Frame::default();
    for clipped in shapes {
        frame.collect(&clipped.shape, clipped.clip_rect);
    }
    let mut cuts = Vec::new();
    for Painted { shape, clip } in frame.painted {
        let galley = &shape.galley;
        let text = galley.job.text.clone();
        let shown: String = galley.rows.iter().flat_map(|row| row.glyphs.iter().map(|g| g.chr)).collect();
        let mut cut = |how| cuts.push(Cut { how, text: text.clone(), shown: shown.clone() });
        if galley.elided {
            cut(How::Shortened);
        }
        // The last row ends the text; any other that does not end at a line break was wrapped.
        if galley.rows.iter().rev().skip(1).any(|row| !row.ends_with_newline) {
            cut(How::Wrapped);
        }
        // Half a point of slack: a glyph's edge that rounds onto the clip's is not cut.
        if !clip.expand(0.5).contains_rect(shape.visual_bounding_rect()) {
            cut(How::Clipped);
        }
    }
    cuts
}
