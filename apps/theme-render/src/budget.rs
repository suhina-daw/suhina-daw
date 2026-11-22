// Copyright (C) 2026 The Suhina contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! Budget counts: how much work the style engine and the painter do in a scene. They need no GPU,
//! and the tests in `tests/budget.rs` hold them, so later work may only lower them.

use std::sync::Arc;

use egui_kittest::Harness;
use egui_kittest::kittest::Queryable;
use suhina_ui::App;
use suhina_ui::kit::native_css;

use crate::Subject;

/// What one scene costs once it has settled, and how many frames changes take to settle.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Counts {
    /// Style nodes in a frame.
    pub nodes: usize,
    /// Element identities whose styles are kept.
    pub identities: usize,
    /// Styles asked for by elements past the style tree's limits, which got none.
    pub unstyled: usize,
    /// Styles worked out afresh in a settled frame.
    pub fresh: usize,
    /// Paint callbacks in a frame: the clears under a menu's or dialog's backdrop, which set the
    /// window's pixels. Every background blends, so nothing else is one (#1233).
    pub paint_callbacks: usize,
    /// Frames from choosing the theme to a still window.
    pub theme_change_frames: u64,
    /// Of those, the frames after which the style engine asked for another (#1273: none).
    pub theme_change_style_frames: u64,
    /// Styles worked out afresh over those frames.
    pub theme_change_fresh: usize,
    /// Frames from moving the pointer onto a clip (or a clip slot) to a still window.
    pub hover_frames: u64,
    /// Of those, the frames after which the style engine asked for another (#1273: none).
    pub hover_style_frames: u64,
    /// Styles worked out afresh over those frames.
    pub hover_fresh: usize,
    /// Frames from one color of the theme's color map edited, as the theme editor does, to a
    /// still window (#1448). None for a built-in theme, which is not edited in place.
    pub color_change_frames: u64,
    /// Styles worked out afresh over those frames: one reset's at most.
    pub color_change_fresh: usize,
}

/// A stand-in for the GPU's clears: each becomes a callback as on the GPU, so the callbacks can be
/// counted without one.
struct CountedClears;

impl suhina_ui::kit::surface::Provider for CountedClears {
    fn shape(&self, rect: egui::Rect, _: egui::CornerRadius, _: egui::Color32) -> egui::Shape {
        egui::Shape::Callback(egui::PaintCallback { rect, callback: Arc::new(()) })
    }
}

fn callbacks(shapes: &[egui::epaint::ClippedShape]) -> usize {
    fn count(shape: &egui::Shape) -> usize {
        match shape {
            egui::Shape::Callback(_) => 1,
            egui::Shape::Vec(shapes) => shapes.iter().map(count).sum(),
            _ => 0,
        }
    }
    shapes.iter().map(|clipped| count(&clipped.shape)).sum()
}

/// Frames a change takes to settle, as kittest's `run` counts them (the still frame included), the
/// frames after which the style engine itself asked for another, and the styles worked out
/// afresh over them. Past the most a scene may take, that most.
fn frames(h: &mut Harness<'static, App>) -> (u64, u64, usize) {
    let (mut fresh, mut asked) = (0, 0);
    for frame in 1..=crate::MAX_STEPS {
        h.step();
        let budget = native_css::budget();
        fresh += budget.fresh;
        asked += u64::from(budget.asked);
        let still = h.output().viewport_output.get(&egui::ViewportId::ROOT).is_none_or(|v| !v.repaint_delay.is_zero());
        if still {
            return (frame, asked, fresh);
        }
    }
    (crate::MAX_STEPS, asked, fresh)
}

/// The counts for `subject` in `scene`: the scene driven with the built-in theme (Glass for
/// Suhina Studio itself, so choosing it resets the styles kept while the scene was driven),
/// `subject` chosen, then the pointer moved onto a clip, then one color of a theme's own map
/// edited.
pub fn count(subject: &Subject, scene: &crate::scenes::Scene) -> Result<Counts, String> {
    let start =
        if subject.id == suhina_ui::kit::theme::BUILT_IN { "suhina-glass" } else { suhina_ui::kit::theme::BUILT_IN };
    let start = Subject { id: start.to_owned(), theme: None };
    let mut h = crate::headless((scene.app)()?, &start, scene.width);
    suhina_ui::kit::surface::install(&h.ctx, Arc::new(CountedClears));
    (scene.drive)(&mut h)?;
    frames(&mut h);
    crate::choose(&mut h, subject);
    let (theme_change_frames, theme_change_style_frames, theme_change_fresh) = frames(&mut h);
    // One more frame, so the counts are a settled frame's.
    h.step();
    let style = native_css::budget();
    let paint_callbacks = callbacks(&h.output().shapes);
    let mut counts = Counts {
        nodes: style.nodes,
        identities: style.identities,
        unstyled: style.unstyled,
        fresh: style.fresh,
        paint_callbacks,
        theme_change_frames,
        theme_change_style_frames,
        theme_change_fresh,
        hover_frames: 0,
        hover_style_frames: 0,
        hover_fresh: 0,
        color_change_frames: 0,
        color_change_fresh: 0,
    };
    // A clip in the Arrangement view, or a clip slot in the launcher.
    let clip = ["Clip ", "Launch "]
        .iter()
        .find_map(|label| h.query_all_by_label_contains(label).next().map(|n| n.rect().center()));
    if let Some(at) = clip {
        h.hover_at(at);
        (counts.hover_frames, counts.hover_style_frames, counts.hover_fresh) = frames(&mut h);
    }
    // One color of the theme's map edited: the base tier's `:root` rule changes.
    if let Some(theme) = subject.theme.as_ref().filter(|_| !suhina_ui::kit::theme::is_built_in(&subject.id)) {
        let mut edited = theme.clone();
        edited.set_color("accent", [0x12, 0x34, 0x56, 0xFF]);
        let ctx = h.ctx.clone();
        let entry = suhina_ui::settings::themes::ThemeEntry { id: subject.id.clone(), theme: edited };
        h.state_mut().put_theme(&ctx, entry);
        let (frames, _, fresh) = frames(&mut h);
        (counts.color_change_frames, counts.color_change_fresh) = (frames, fresh);
    }
    Ok(counts)
}
