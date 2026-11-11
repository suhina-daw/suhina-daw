// Copyright (C) 2026 The Suhina contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! The style engine's budget (#1231): what the demo project and an expanded FM synth cost, held at
//! the counts recorded here. A change may only lower them; one that does lowers the recorded
//! number with it, so the budget keeps what was won. Headless and deterministic: no GPU, a fixed
//! step, and the window's RAM figure held.

use suhina_theme_render::budget::{Counts, count};
use suhina_theme_render::{Subject, scenes};

/// The counts recorded for each scene. `fresh` is that of a settled frame; the `*_fresh` are
/// summed over the frames a change takes to settle. The style engine works out an element's style
/// in the frame that draws it (#1273), so it asks for no frame of its own after a change
/// (`*_style_frames`); the frame left after a theme change is egui's own after a new palette, and
/// those after a hover are its tooltip's. Every background blends (#1233), so a settled frame has
/// no paint callbacks: only a menu's or dialog's backdrop clears the pixels under it.
#[cfg(not(windows))]
const RECORDED: [(&str, Counts); 2] = [
    (
        "launcher",
        Counts {
            nodes: 319,
            identities: 293,
            unstyled: 0,
            fresh: 0,
            paint_callbacks: 0,
            theme_change_frames: 2,
            theme_change_style_frames: 0,
            theme_change_fresh: 293,
            hover_frames: 3,
            hover_style_frames: 0,
            hover_fresh: 0,
        },
    ),
    (
        "expanded-fm-synth",
        Counts {
            nodes: 543,
            identities: 516,
            unstyled: 0,
            fresh: 0,
            paint_callbacks: 0,
            theme_change_frames: 2,
            theme_change_style_frames: 0,
            theme_change_fresh: 516,
            hover_frames: 4,
            hover_style_frames: 0,
            hover_fresh: 0,
        },
    ),
];

/// The same on Windows, where Suhina draws the window's title bar itself (the application bar), which
/// the other systems leave to the system.
#[cfg(windows)]
const RECORDED: [(&str, Counts); 2] = [
    (
        "launcher",
        Counts {
            nodes: 327,
            identities: 301,
            unstyled: 0,
            fresh: 0,
            paint_callbacks: 0,
            theme_change_frames: 2,
            theme_change_style_frames: 0,
            theme_change_fresh: 301,
            hover_frames: 4,
            hover_style_frames: 0,
            hover_fresh: 0,
        },
    ),
    (
        "expanded-fm-synth",
        Counts {
            nodes: 551,
            identities: 524,
            unstyled: 0,
            fresh: 0,
            paint_callbacks: 0,
            theme_change_frames: 2,
            theme_change_style_frames: 0,
            theme_change_fresh: 524,
            hover_frames: 4,
            hover_style_frames: 0,
            hover_fresh: 0,
        },
    ),
];

fn within(name: &str, theme: &str, counts: Counts, budget: Counts) -> Vec<String> {
    let fields = [
        ("nodes", counts.nodes as u64, budget.nodes as u64),
        ("identities", counts.identities as u64, budget.identities as u64),
        ("unstyled", counts.unstyled as u64, budget.unstyled as u64),
        ("fresh styles", counts.fresh as u64, budget.fresh as u64),
        ("paint callbacks", counts.paint_callbacks as u64, budget.paint_callbacks as u64),
        ("frames after a theme change", counts.theme_change_frames, budget.theme_change_frames),
        ("style frames after a theme change", counts.theme_change_style_frames, budget.theme_change_style_frames),
        ("fresh styles after a theme change", counts.theme_change_fresh as u64, budget.theme_change_fresh as u64),
        ("frames after a hover", counts.hover_frames, budget.hover_frames),
        ("style frames after a hover", counts.hover_style_frames, budget.hover_style_frames),
        ("fresh styles after a hover", counts.hover_fresh as u64, budget.hover_fresh as u64),
    ];
    fields
        .into_iter()
        .filter(|(_, now, recorded)| now > recorded)
        .map(|(field, now, recorded)| {
            format!("{theme} in {name}: {field} went up from {recorded} to {now}: {counts:?}")
        })
        .collect()
}

#[test]
fn the_demo_and_an_expanded_fm_synth_stay_within_the_budget() {
    // Glass, a bundled see-through theme with a style sheet, and the see-through test theme.
    let themes = [Subject::built_in("suhina-glass").expect("a built-in theme"), super::see_through().unwrap()];
    let mut over = Vec::new();
    for theme in &themes {
        for (name, budget) in RECORDED {
            let scene = scenes::find(name).unwrap_or_else(|| panic!("no scene {name}"));
            let counts = count(theme, &scene).unwrap_or_else(|e| panic!("{name}: {e}"));
            over.extend(within(name, &theme.id, counts, budget));
        }
    }
    assert!(over.is_empty(), "over the budget:\n{}", over.join("\n"));
}

#[test]
fn counting_twice_gives_the_same_counts() {
    let theme = super::see_through().unwrap();
    let scene = scenes::find("launcher").expect("the launcher scene");
    assert_eq!(count(&theme, &scene), count(&theme, &scene));
}
