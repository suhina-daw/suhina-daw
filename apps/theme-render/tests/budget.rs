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
/// (`*_style_frames`); the frame left after a theme change is egui's own after new `Visuals`, and
/// those after a hover are its tooltip's. Every background blends (#1233), so a settled frame has
/// no paint callbacks: only a menu's or dialog's backdrop clears the pixels under it.
///
/// #1370 (the window grid) added one styled element to each scene: one more
/// identity in each.
///
/// #1336 (the content box) raised the counts of nodes and identities, by 2 and 4 in the launcher
/// and by 3 and 4 in the FM synth: its elements now hold what they lay out, inside their border.
///
/// #1372 (the bars' rows) raised them again: the bars' items are elements a theme styles
/// (`bar-item`, `project-tab`, `group`, `separator`), 5 more nodes and identities in each scene
/// (10 and 9 on Windows, where the application bar has its items too).
///
/// #1417 (the views draw from their elements) raised them by 61 nodes and 54 identities in each
/// scene: the Launcher's lines, stop buttons, scenes' numbers, its mixer's device lists, meters'
/// levels and the other parts that are elements now, each kind read once a frame, not once an item.
///
/// #1023 (the browser and the detail panels draw from their elements) raised them by 1 node and 1
/// identity in the Launcher (the empty detail area's note, `label[kind="hint"]`) and by 8 nodes and
/// 10 identities in the FM synth (its header's words and on switch, its expanded panel's header, ×
/// and lines, each read once a frame).
///
/// #1023 (the control bar, tabs, menus, Settings and the dialogs draw from their elements) raised
/// them by 11 nodes and 17 identities in each scene: the control bar's words, meters and lit
/// switches, each read once a frame (`label[kind]`, `meter`, `button`, `drop-target`).
///
/// #1442 (egui's own widgets take their look from the style sheet) raised the identities, and the
/// styles worked out after a theme change, by 11 in each scene: the lookups the root's `Visuals`
/// are built from (`button` in its four states and `[checked]`, `text-input` and its `:focus`,
/// `menu`, `scroll-bar` in three states), once when the styles reset, with no node in the tree.
///
/// #1448 (a base style sheet under every theme) runs Suhina Studio on the style engine, and its
/// scenes are counted here too: started from Glass, so choosing Studio resets the styles, it
/// costs what Glass and the see-through theme cost, and no count went up. It added the counts
/// after one color of a theme's own map is edited (`color_change_*`, the see-through theme's;
/// a built-in theme is not edited in place): the color map is the base tier's `:root` rule,
/// and the base sheet reads every color variable, so one change resets the styles once, as a
/// theme change does (2 frames, and every identity worked out afresh: 386 and 618, 398 and 630 on
/// Windows). Before, a color the theme's own sheet did not read reset nothing. Resetting only the
/// styles that read the changed variable is #1456.
///
/// #1449 (the shared controls take their colors from the base sheet) added no node and no
/// identity, and one style worked out the first time the pointer reaches a control whose colors
/// fade (`hover_fresh` 0 to 1 in each scene, and one more after a color change: 387 and 619, 399
/// and 631 on Windows): the look it fades to is a style of its own, looked up while it fades.
///
/// #1450 (the Launcher, the mixer and the Arrangement take their colors from the base sheet) added
/// 39 nodes and 39 identities to each scene (the Windows numbers move by the same): the looks the
/// views read their colors from, each a style of its own, once for the whole view or per control
/// (the switches' lit looks, the scenes' and the device rows' fades, a clip's states, the surface
/// colors of the Launcher and of the Arrangement). The theme change and the color change work
/// those identities out afresh, so their counts move by the same 39: 425 and 657, 426 and 658
/// (437 and 669, 438 and 670 on Windows).
///
/// #1451 (the detail panels, the piano roll and the browser take their colors from the base sheet)
/// raised the FM synth's counts by 18 nodes and 18 identities over #1450's, and the styles worked
/// out after a theme or a color change by as many (675 and 676, 687 and 688 on Windows); the
/// Launcher's did not move. The colors a device's canvas draws with are read from `device-canvas`
/// and its six kinds, and the parts that fell back to different colors are told apart by kinds of
/// their own (the expanded panel's fills, `detail-fill`, its parameters' `label`, `value` and
/// `slider`), each read once a frame.
/// #1452 (the app's bars, Settings and dialogs take their colors from the base sheet; egui's
/// own widgets take theirs from `widget`) raised every scene's counts by 22 nodes and 30
/// identities, and the styles worked out after a theme or a color change by 30 (455 and 456, 705
/// and 706; 467 and 468, 717 and 718 on Windows): the control bar's fills, switches and words
/// (`app-fill`, `button[kind]`, `label[kind]`, `meter[kind="cpu"]`), the two inks of text on a
/// color (`label[kind="on-light"]` and `[kind="on-dark"]`, read once a frame for each set of
/// variables in scope, not once a clip), and the lookups the root's `Visuals` are built from (`widget` in its
/// states and kinds, inside a `window`), once when the styles reset, with no node left in the tree.
///
/// #1269 (the time signature in the toolbar) raised them by 2 nodes and 1 identity in each scene:
/// its two numbers are value controls a theme styles, as the tempo is, where it was plain text.
/// With #1453's strips laid out in CSS and the stack's colors together (the combined #1023 pull
/// request), every scene's counts are 5 nodes and 4 identities over #1452's, and the styles worked
/// out after a theme or a color change 4 more (459 and 460, 709 and 710). On Windows each count is
/// one more again (478 nodes and 472 identities, 472 and 473, 722 and 723), as CI measured it; which
/// element that extra one is has not been traced.
///
/// The Launcher's add-track buttons (a "+" and its words painted apart) and the piano roll's
/// Fold, Scale and Highlight buttons (ordinary tool buttons) raised the node counts by 4 in the
/// Launcher and 1 in the FM synth (469 and 716; 481 and 728 on Windows, where the FM synth did not move), while the identities
/// went down (452 and 698).
///
/// The menu bar's titles (#1487 › P8) are elements a theme styles (`menu-bar bar-item[kind="menu"]`),
/// 5 more nodes in each scene and no identity (474 and 721; 486 and 733 on Windows).
///
/// #1718 (the FM Synth's native dials) raised the expanded scene by 15 nodes and 6 identities,
/// and the styles worked out after a theme change by 6 (and after a color change in the
/// see-through theme by 6): its Output, Stereo, Voices and Pitch dials and the device row's LFO
/// rate and Random amount are the SDK's native dial widget, which the style engine paints as knob
/// elements, where the hand-drawn dial was a canvas drawing (736 and 715; 748 and 728 on
/// Windows).
///
/// #1678 (the crossfader) raised the Launcher by 11 nodes and 4 identities, and the expanded FM
/// scene by 11 nodes and 11 identities, and the styles worked out after a theme or color change
/// by the same: the Main strip's crossfader and each track strip's A/B/off button, each a styled
/// level bar or button (485 and 463, 747 and 726; 497 and 476, 759 and 739 on Windows).
///
/// On Windows the window's three buttons are `button` elements a sheet styles (they were egui
/// buttons with no frame), 3 more nodes in the Launcher and 1 in the FM synth, on top of
/// #1678's counts (500 and 760); their identities are shared with the other buttons', so the
/// recorded identities stay.
#[cfg(not(windows))]
const RECORDED: [(&str, Counts); 2] = [
    (
        "launcher",
        Counts {
            nodes: 485,
            identities: 463,
            unstyled: 0,
            fresh: 0,
            paint_callbacks: 0,
            theme_change_frames: 2,
            theme_change_style_frames: 0,
            theme_change_fresh: 463,
            hover_frames: 3,
            hover_style_frames: 0,
            hover_fresh: 1,
            color_change_frames: 2,
            color_change_fresh: 464,
        },
    ),
    (
        "expanded-fm-synth",
        Counts {
            nodes: 747,
            identities: 726,
            unstyled: 0,
            fresh: 0,
            paint_callbacks: 0,
            theme_change_frames: 2,
            theme_change_style_frames: 0,
            theme_change_fresh: 726,
            hover_frames: 4,
            hover_style_frames: 0,
            hover_fresh: 1,
            color_change_frames: 2,
            color_change_fresh: 727,
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
            nodes: 500,
            identities: 476,
            unstyled: 0,
            fresh: 0,
            paint_callbacks: 0,
            theme_change_frames: 2,
            theme_change_style_frames: 0,
            theme_change_fresh: 476,
            hover_frames: 4,
            hover_style_frames: 0,
            hover_fresh: 1,
            color_change_frames: 2,
            color_change_fresh: 477,
        },
    ),
    (
        "expanded-fm-synth",
        Counts {
            nodes: 760,
            identities: 739,
            unstyled: 0,
            fresh: 0,
            paint_callbacks: 0,
            theme_change_frames: 2,
            theme_change_style_frames: 0,
            theme_change_fresh: 739,
            hover_frames: 4,
            hover_style_frames: 0,
            hover_fresh: 1,
            color_change_frames: 2,
            color_change_fresh: 740,
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
        ("frames after a color change", counts.color_change_frames, budget.color_change_frames),
        ("fresh styles after a color change", counts.color_change_fresh as u64, budget.color_change_fresh as u64),
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
    // Suhina Studio, which runs the style engine like every theme (#1448); Glass, a bundled
    // see-through theme with a style sheet; and the see-through test theme.
    let themes = [
        Subject::built_in(suhina_ui::kit::theme::BUILT_IN).expect("a built-in theme"),
        Subject::built_in("suhina-glass").expect("a built-in theme"),
        super::see_through().unwrap(),
    ];
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
