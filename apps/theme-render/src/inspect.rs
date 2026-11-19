// Copyright (C) 2026 The Suhina contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! What a theme author asks when a rule does nothing (#1582): which element is at a point and
//! which rules made it look as it does (`--probe`), and which of the theme's rules matched no
//! element in any scene (`--coverage`). [`crate::outline`] draws every element's box and name.
//!
//! Both read the style engine's record of the last frame
//! ([`suhina_ui::kit::native_css::inspector`]): the elements the frame entered, with the box each
//! painted, and the cascade behind any of them, worked out from the tree and sheets the frame was
//! styled with. A part that reads an element's colour without entering the element has no box of
//! its own, so a probe at it names the element around it.

use std::collections::{BTreeMap, BTreeSet};
use std::fmt::Write as _;

use egui::{LayerId, Pos2};
use egui_kittest::Harness;
use suhina_ui::App;
use suhina_ui::kit::native_css::inspector::{self, FrameElement, Layer, MatchedRule, Outcome};

use crate::Subject;
use crate::scenes::Scene;

/// `scene` driven in `subject` without a GPU and left still: its last frame is the one inspected.
pub fn settled(subject: &Subject, scene: &Scene) -> Result<Harness<'static, App>, String> {
    let mut h = crate::headless((scene.app)()?, subject, scene.width);
    (scene.drive)(&mut h)?;
    // As for the pictures: wait for the background picture and the sample map, then settle.
    h.state_mut().wait_for_workers();
    // A scene that keeps repainting is inspected in its last frame all the same.
    let _ = h.try_run();
    h.step();
    Ok(h)
}

/// The element of `elements` painted at `at`: of those whose box holds the point, the one on the
/// topmost layer (`layers` back to front: an open menu over the view), and on that layer the one
/// entered last, which painted over the others.
pub fn hit(elements: &[FrameElement], layers: &[LayerId], at: Pos2) -> Option<usize> {
    elements
        .iter()
        .enumerate()
        .filter_map(|(index, element)| {
            let (rect, layer) = element.painted?;
            rect.contains(at).then(|| (layers.iter().position(|l| *l == layer), index))
        })
        .max()
        .map(|(_, index)| index)
}

/// The element at `index` and the ones it is in, outermost first, as selectors.
pub fn path(elements: &[FrameElement], index: usize) -> Vec<String> {
    let mut path = Vec::new();
    let mut at = Some(index);
    while let Some(element) = at.and_then(|at| elements.get(at)) {
        path.push(element.selector());
        // A parent always comes before its child, so this ends.
        at = element.parent.filter(|parent| Some(*parent) < at);
    }
    path.reverse();
    path
}

/// Where a tier's rules come from, as the probe names them.
fn tier(layer: Layer) -> &'static str {
    match layer {
        Layer::UserAgent => "Suhina's user-agent sheet",
        Layer::Base => "Suhina's base sheet",
        Layer::Theme => "theme.css",
        Layer::Variant => "the chosen variant",
        Layer::Device => "a device panel's sheet",
        Layer::User => "user.css",
    }
}

/// What `--probe` prints for the point `at` of the window `h` last drew: the element there, the
/// elements it is in, and its matching rules, the strongest first, with what became of each
/// declaration. `Err` when nothing styled is painted there.
pub fn probe(h: &Harness<'static, App>, at: Pos2) -> Result<String, String> {
    let elements = inspector::frame_elements();
    let layers: Vec<LayerId> = h.ctx.memory(|m| m.layer_ids().collect());
    let index = hit(&elements, &layers, at)
        .ok_or_else(|| format!("no element is painted at {},{} in this scene", at.x, at.y))?;
    let element = elements.get(index).ok_or("the element left the frame")?;
    let explained = inspector::explain(index)?;
    let mut out = String::new();
    let _ = writeln!(out, "{}", element.selector());
    if let Some((rect, _)) = element.painted {
        let _ = writeln!(
            out,
            "  padding box {:.0},{:.0} to {:.0},{:.0} (as far as it shows)",
            rect.min.x, rect.min.y, rect.max.x, rect.max.y
        );
    }
    let _ = writeln!(out, "  in {}", path(&elements, index).join(" > "));
    if explained.painted_differs {
        let _ = writeln!(
            out,
            "  note: the frame painted it with a style that differs from this one (held for a transition, or \
             styled before the frame changed it)"
        );
    }
    out.push('\n');
    if explained.rules.is_empty() {
        out.push_str("No rule matches it: it has its inherited and initial values.\n");
    }
    for rule in explained.rules.iter().rev() {
        write_rule(&mut out, rule);
    }
    Ok(out)
}

/// Most characters of a selector list the probe prints.
const SELECTOR_CHARS: usize = 100;

fn write_rule(out: &mut String, rule: &MatchedRule) {
    let mut selector: String = rule.selector.chars().take(SELECTOR_CHARS).collect();
    if selector.len() < rule.selector.len() {
        selector.push('…');
    }
    let _ = writeln!(out, "{} line {}: {selector}", tier(rule.layer), rule.line);
    for declaration in &rule.declarations {
        let name = match declaration.shorthand {
            Some(shorthand) => format!("{} (from {shorthand})", declaration.name),
            None => declaration.name.clone(),
        };
        let important = if declaration.important { " !important" } else { "" };
        let outcome = match &declaration.outcome {
            Outcome::Applied(value) if value.trim() == declaration.value.trim() => "applies".to_owned(),
            Outcome::Applied(value) => format!("applies as {value}"),
            Outcome::Dropped(why) => format!("dropped: {why}; the element keeps the inherited or initial value"),
            Outcome::Overridden => "overridden".to_owned(),
        };
        let _ = writeln!(out, "  {name}: {}{important}  ({outcome})", declaration.value);
    }
}

/// A rule of the theme's sheet that matched nothing.
#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub struct Unused {
    pub line: u32,
    pub selector: String,
    /// For a rule that names artwork (`icon[kind="play"]`, or `fader.cap` in the earlier form),
    /// whose `image` Suhina reads when it loads the theme whether an element matches it or not:
    /// the selector Suhina writes for that artwork now (`fader-cap`).
    pub artwork: Option<String>,
}

/// The selector Suhina writes now for the artwork that `selector` names, when it names some.
fn artwork(selector: &str) -> Option<String> {
    use suhina_ui::kit::{skin, theme_css};
    if selector.contains(',') {
        return None;
    }
    let role = theme_css::artwork_role(selector);
    skin::valid_role(&role).then(|| theme_css::artwork_selector(&role))
}

/// The rules of `subject`'s own sheet (not its variant's) that matched no element in any of
/// `scenes`, with the scenes that could not be driven. The scenes are driven on this thread,
/// which records the matches while they run.
pub fn coverage(subject: &Subject, scenes: &[Scene]) -> (Vec<Unused>, Vec<String>) {
    inspector::record_matches(true);
    // The theme's sheet as each window styled with it: the same text, so the same id, unless a
    // window changed it.
    let mut sheets: BTreeMap<u64, Vec<(u32, String)>> = BTreeMap::new();
    let mut failed = Vec::new();
    for scene in scenes {
        match settled(subject, scene) {
            Ok(_) => {
                if let Some((id, rules)) = inspector::theme_rules() {
                    sheets.insert(id, rules);
                }
            }
            Err(e) => failed.push(format!("{}: {e}", scene.name)),
        }
    }
    let matched = inspector::matched_rules();
    inspector::record_matches(false);
    let used: BTreeSet<usize> =
        matched.iter().filter(|(sheet, _)| sheets.contains_key(sheet)).map(|(_, rule)| *rule).collect();
    let unused = sheets
        .values()
        .next()
        .map(|rules| {
            rules
                .iter()
                .enumerate()
                .filter(|(index, _)| !used.contains(index))
                .map(|(_, (line, selector))| Unused {
                    line: *line,
                    selector: selector.clone(),
                    artwork: artwork(selector),
                })
                .collect()
        })
        .unwrap_or_default();
    (unused, failed)
}
