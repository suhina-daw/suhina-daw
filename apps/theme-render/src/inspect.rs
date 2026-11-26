// Copyright (C) 2026 The Suhina contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! What a theme author asks when a rule does nothing (#1582): which element is at a point and
//! which rules made it look as it does (`--probe`), and which of the theme's rules matched no
//! element in any scene (`--coverage`). [`crate::outline`] draws every element's box and name.
//!
//! Both read the style engine's record of the last frame
//! ([`suhina_ui::kit::native_css::inspector`]): the elements the frame entered, with the box each
//! painted, and the cascade behind any of them, worked out from the tree and sheets the frame was
//! styled with. A part that reads an element's color without entering the element has no box of
//! its own, so a probe at it names the element around it.

use std::collections::BTreeSet;
use std::fmt::Write as _;

use egui::{LayerId, Pos2};
use egui_kittest::Harness;
use suhina_ui::App;
use suhina_ui::kit::native_css::inspector::{self, FrameElement, Layer, MatchedRule, Outcome, ThemeRule};

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

/// Where `layer` is painted among the window's layers, back to front: by its order (background,
/// middle, foreground, tooltip), then by its place among the windows' areas (`layers`, back to
/// front), a layer that is no area first.
pub fn layer_rank(layers: &[LayerId], layer: LayerId) -> (egui::Order, Option<usize>) {
    (layer.order, layers.iter().position(|l| *l == layer))
}

/// The element of `elements` painted at `at`: of those whose box holds the point, the one on the
/// topmost layer ([`layer_rank`]: an open menu over the view), and on that layer the one entered
/// last, which painted over the others.
pub fn hit(elements: &[FrameElement], layers: &[LayerId], at: Pos2) -> Option<usize> {
    elements
        .iter()
        .enumerate()
        .filter_map(|(index, element)| {
            let (rect, layer) = element.painted?;
            rect.contains(at).then(|| (layer_rank(layers, layer), index))
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
    /// For a rule that sets the `image` of an artwork role (`icon[kind="play"]`, or `fader.cap`
    /// in the earlier form), which Suhina reads when it loads the theme whether an element matches
    /// it or not: the selector Suhina writes for that artwork now (`fader-cap`).
    pub artwork: Option<String>,
}

/// The selector Suhina writes now for the artwork that `rule` names, when it is an artwork rule:
/// one selector that names an artwork role, and an `image`.
fn artwork(rule: &ThemeRule) -> Option<String> {
    use suhina_ui::kit::{skin, theme_css};
    if rule.selector.contains(',') || !rule.properties.iter().any(|p| p == "image") {
        return None;
    }
    let role = theme_css::artwork_role(&rule.selector);
    skin::valid_role(&role).then(|| theme_css::artwork_selector(&role))
}

/// What [`coverage`] found.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Coverage {
    /// The theme's rules that matched no element in any scene driven.
    pub unused: Vec<Unused>,
    /// How many scenes were driven, and the ones that could not be, with why.
    pub driven: usize,
    pub failed: Vec<String>,
}

/// The rules of `subject`'s own sheet (not its variant's) that matched no element in any of
/// `scenes`. The scenes are driven on this thread, which records the matches while they run.
/// `Err` when no scene was styled with a theme sheet at all (none could be driven, or the
/// theme's CSS does not parse), so that nothing is called unused or used unchecked.
pub fn coverage(subject: &Subject, scenes: &[Scene]) -> Result<Coverage, String> {
    inspector::record_matches(true);
    // The theme's sheet as the first window styled with it, by its id: every window parses the
    // same text, so the same id.
    let mut sheet: Option<(u64, Vec<ThemeRule>)> = None;
    let mut found = Coverage::default();
    for scene in scenes {
        match settled(subject, scene) {
            Ok(_) => {
                found.driven += 1;
                match (inspector::theme_rules(), &sheet) {
                    (Some(rules), None) => sheet = Some(rules),
                    (Some((id, _)), Some((first, _))) if id != *first => {
                        found.failed.push(format!("{}: styled with another theme sheet, not counted", scene.name));
                    }
                    _ => {}
                }
            }
            Err(e) => found.failed.push(format!("{}: {e}", scene.name)),
        }
    }
    let matched = inspector::matched_rules();
    inspector::record_matches(false);
    let (id, rules) = sheet.ok_or("no scene was styled with the theme's sheet: does its CSS parse?")?;
    let used: BTreeSet<usize> = matched.iter().filter(|(sheet, _)| *sheet == id).map(|(_, rule)| *rule).collect();
    found.unused = rules
        .iter()
        .enumerate()
        .filter(|(index, _)| !used.contains(index))
        .map(|(_, rule)| Unused { line: rule.line, selector: rule.selector.clone(), artwork: artwork(rule) })
        .collect();
    Ok(found)
}
