// Copyright (C) 2026 The Suhina contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! The texts a frame cuts off (#1548): shortened with "…" to fit, wrapped onto a line of their own
//! where they have no line break, or painted partly outside their clip. Read from the frame's
//! shapes, so it finds a cut whatever code painted the text.
//!
//! A text painted twice at the same place, each copy clipped to one side (a value over a fill, in
//! two colours), is one text whose clips are joined when they touch: it is cut only where neither
//! copy shows it. A text wholly outside its clip shows nothing and is not listed, so a label
//! pushed out of sight entirely is not found here.

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
                    // Clips that touch join into their bounding box; apart, the gap between them
                    // hides the text, so the first copy's clip alone stands.
                    Some(copy) if copy.clip.expand(0.5).intersects(clip) => copy.clip = copy.clip.union(clip),
                    Some(_) => {}
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

#[cfg(test)]
mod tests {
    use super::*;
    use egui::text::{LayoutJob, TextWrapping};
    use egui::{Color32, FontId, Pos2, pos2};

    /// `text` laid out in the default font, wrapped or shortened at `width` when given.
    fn shape(ctx: &egui::Context, text: &str, at: Pos2, width: Option<(f32, bool)>) -> Shape {
        let mut job = LayoutJob::simple(text.into(), FontId::proportional(14.0), Color32::WHITE, f32::INFINITY);
        if let Some((width, shorten)) = width {
            job.wrap =
                if shorten { TextWrapping::truncate_at_width(width) } else { TextWrapping::wrap_at_width(width) };
        }
        let galley = ctx.fonts_mut(|fonts| fonts.layout_job(job));
        Shape::galley(at, galley, Color32::WHITE)
    }

    fn context() -> egui::Context {
        let ctx = egui::Context::default();
        let mut output = ctx.run_ui(egui::RawInput::default(), |_| {});
        // Nothing draws the font atlas here: its upload is dropped on purpose.
        output.textures_delta.clear();
        ctx
    }

    fn clipped(clip: Rect, shape: Shape) -> ClippedShape {
        ClippedShape::new(clip, shape)
    }

    const WIDE: Rect = Rect::from_min_max(pos2(0.0, 0.0), pos2(1000.0, 100.0));

    fn hows(shapes: &[ClippedShape]) -> Vec<(How, String)> {
        texts(shapes).into_iter().map(|cut| (cut.how, cut.text)).collect()
    }

    #[test]
    fn a_text_that_fits_is_not_cut() {
        let ctx = context();
        assert!(hows(&[clipped(WIDE, shape(&ctx, "Quantize", pos2(10.0, 10.0), None))]).is_empty());
    }

    #[test]
    fn a_shortened_text_is_cut_and_says_what_shows() {
        let ctx = context();
        let cuts = texts(&[clipped(WIDE, shape(&ctx, "Quantize", pos2(10.0, 10.0), Some((20.0, true))))]);
        assert_eq!(cuts.len(), 1, "{cuts:?}");
        assert_eq!(cuts[0].how, How::Shortened);
        assert!(cuts[0].shown.ends_with('…') && cuts[0].shown.len() < "Quantize…".len(), "{:?}", cuts[0].shown);
    }

    #[test]
    fn a_text_wrapped_without_a_line_break_is_cut_and_one_with_breaks_is_not() {
        let ctx = context();
        let wrapped = shape(&ctx, "8 bars", pos2(10.0, 10.0), Some((15.0, false)));
        assert_eq!(hows(&[clipped(WIDE, wrapped)]), [(How::Wrapped, "8 bars".to_owned())]);
        let broken = shape(&ctx, "8\nbars", pos2(10.0, 10.0), None);
        assert!(hows(&[clipped(WIDE, broken)]).is_empty());
    }

    #[test]
    fn a_text_partly_outside_its_clip_is_cut_and_one_wholly_outside_is_not_listed() {
        let ctx = context();
        let half = Rect::from_min_max(pos2(0.0, 0.0), pos2(20.0, 100.0));
        assert_eq!(
            hows(&[clipped(half, shape(&ctx, "Frequency", pos2(10.0, 10.0), None))]),
            [(How::Clipped, "Frequency".to_owned())]
        );
        assert!(hows(&[clipped(half, shape(&ctx, "Frequency", pos2(500.0, 10.0), None))]).is_empty());
        // A glyph's edge on the clip's edge, within half a point, is not cut.
        let text = shape(&ctx, "Frequency", pos2(10.0, 10.0), None);
        let edge = text.visual_bounding_rect().expand(0.25);
        assert!(hows(&[clipped(edge, text)]).is_empty());
    }

    #[test]
    fn two_copies_clipped_to_touching_sides_are_one_whole_text() {
        let ctx = context();
        let text = || shape(&ctx, "-6.0", pos2(10.0, 10.0), None);
        let whole = text().visual_bounding_rect();
        let split = whole.center().x;
        let left = Rect::from_min_max(whole.min - egui::vec2(1.0, 1.0), pos2(split, whole.max.y + 1.0));
        let right = Rect::from_min_max(pos2(split, whole.min.y - 1.0), whole.max + egui::vec2(1.0, 1.0));
        assert!(hows(&[clipped(left, text()), clipped(right, text())]).is_empty(), "joined, the copies show it all");
        // Apart, the gap between the clips hides part of the text.
        let gap = right.translate(egui::vec2(3.0, 0.0));
        assert_eq!(hows(&[clipped(left, text()), clipped(gap, text())]), [(How::Clipped, "-6.0".to_owned())]);
    }

    #[test]
    fn a_text_with_no_glyphs_is_not_listed() {
        let ctx = context();
        assert!(hows(&[clipped(WIDE, shape(&ctx, "", pos2(10.0, 10.0), Some((1.0, true))))]).is_empty());
    }
}
