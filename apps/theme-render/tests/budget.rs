// Copyright (C) 2026 The Suhina contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! The style engine's budget (#1231): what the demo project and an expanded FM synth cost, held at
//! the counts recorded here. A change may only lower them; one that does lowers the recorded
//! number with it, so the budget keeps what was won. Headless and deterministic: no GPU, a fixed
//! step, and the window's RAM figure held.

use suhina_theme_render::budget::{Counts, count};
use suhina_theme_render::{Subject, scenes};

/// The counts recorded for each scene. `approximations` and `context_misses` are those of a
/// settled frame; the `*_approximations` are summed over the frames a change takes to settle.
#[cfg(not(windows))]
const RECORDED: [(&str, Counts); 2] = [
    (
        "launcher",
        Counts {
            nodes: 318,
            contexts: 64,
            approximations: 0,
            context_misses: 0,
            paint_callbacks: 138,
            theme_change_frames: 3,
            theme_change_approximations: 1040,
            hover_frames: 3,
            hover_approximations: 0,
        },
    ),
    (
        "expanded-fm-synth",
        Counts {
            nodes: 541,
            contexts: 82,
            approximations: 0,
            context_misses: 0,
            paint_callbacks: 163,
            theme_change_frames: 3,
            theme_change_approximations: 2157,
            hover_frames: 4,
            hover_approximations: 0,
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
            nodes: 326,
            contexts: 67,
            approximations: 0,
            context_misses: 0,
            paint_callbacks: 139,
            theme_change_frames: 3,
            theme_change_approximations: 1097,
            hover_frames: 4,
            hover_approximations: 0,
        },
    ),
    (
        "expanded-fm-synth",
        Counts {
            nodes: 549,
            contexts: 85,
            approximations: 0,
            context_misses: 0,
            paint_callbacks: 164,
            theme_change_frames: 3,
            theme_change_approximations: 2214,
            hover_frames: 4,
            hover_approximations: 0,
        },
    ),
];

fn within(name: &str, theme: &str, counts: Counts, budget: Counts) -> Vec<String> {
    let fields = [
        ("nodes", counts.nodes as u64, budget.nodes as u64),
        ("contexts", counts.contexts as u64, budget.contexts as u64),
        ("approximations", counts.approximations as u64, budget.approximations as u64),
        ("context misses", counts.context_misses as u64, budget.context_misses as u64),
        ("paint callbacks", counts.paint_callbacks as u64, budget.paint_callbacks as u64),
        ("frames after a theme change", counts.theme_change_frames, budget.theme_change_frames),
        (
            "approximations after a theme change",
            counts.theme_change_approximations as u64,
            budget.theme_change_approximations as u64,
        ),
        ("frames after a hover", counts.hover_frames, budget.hover_frames),
        ("approximations after a hover", counts.hover_approximations as u64, budget.hover_approximations as u64),
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
