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
    /// Distinct scope stacks kept with an approximate style.
    pub contexts: usize,
    /// Styles served in a frame to scopes with no style resolved for them.
    pub approximations: usize,
    /// Of those, the ones worked out afresh in that frame.
    pub context_misses: usize,
    /// Paint callbacks in a frame: background fills that set the window's pixels.
    pub paint_callbacks: usize,
    /// Frames from choosing the theme to a still window.
    pub theme_change_frames: u64,
    /// Styles served from an approximation over those frames.
    pub theme_change_approximations: usize,
    /// Frames from moving the pointer onto a clip (or a clip slot) to a still window.
    pub hover_frames: u64,
    /// Styles served from an approximation over those frames.
    pub hover_approximations: usize,
}

/// A stand-in for the GPU's background fills: each fill becomes a callback as on the GPU, so the
/// callbacks can be counted without one.
struct CountedFills;

impl suhina_ui::kit::surface::Provider for CountedFills {
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

/// Frames a change takes to settle, as kittest's `run` counts them (the still frame included),
/// and the approximate styles served over them. Past the most a scene may take, that most.
fn frames(h: &mut Harness<'static, App>) -> (u64, usize) {
    let mut approximations = 0;
    for frame in 1..=crate::MAX_STEPS {
        h.step();
        approximations += native_css::budget().approximations;
        let still = h.output().viewport_output.get(&egui::ViewportId::ROOT).is_none_or(|v| !v.repaint_delay.is_zero());
        if still {
            return (frame, approximations);
        }
    }
    (crate::MAX_STEPS, approximations)
}

/// The counts for `subject` in `scene`: the scene driven with the built-in theme, `subject`
/// chosen, then the pointer moved onto a clip.
pub fn count(subject: &Subject, scene: &crate::scenes::Scene) -> Result<Counts, String> {
    let built_in = Subject { id: suhina_ui::kit::theme::BUILT_IN.to_owned(), theme: None };
    let mut h = crate::headless((scene.app)()?, &built_in, scene.width);
    suhina_ui::kit::surface::install(&h.ctx, Arc::new(CountedFills));
    (scene.drive)(&mut h)?;
    frames(&mut h);
    crate::choose(&mut h, subject);
    let (theme_change_frames, theme_change_approximations) = frames(&mut h);
    // One more frame, so the counts are a settled frame's.
    h.step();
    let style = native_css::budget();
    let paint_callbacks = callbacks(&h.output().shapes);
    let mut counts = Counts {
        nodes: style.nodes,
        contexts: style.contexts,
        approximations: style.approximations,
        context_misses: style.context_misses,
        paint_callbacks,
        theme_change_frames,
        theme_change_approximations,
        hover_frames: 0,
        hover_approximations: 0,
    };
    // A clip in the Arrangement view, or a clip slot in the launcher.
    let clip = ["Clip ", "Launch "]
        .iter()
        .find_map(|label| h.query_all_by_label_contains(label).next().map(|n| n.rect().center()));
    if let Some(at) = clip {
        h.hover_at(at);
        (counts.hover_frames, counts.hover_approximations) = frames(&mut h);
    }
    Ok(counts)
}
