// Copyright (C) 2026 The Suhina contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! The last frame's elements as the style sheet saw them, for the theme renderer's diagnostics
//! (#1582): where each element was painted, the selector it matches as, and why each of its
//! properties has its value. Read only, on the thread that drew the frame, after the frame and
//! before the next one begins; the app itself never calls it.

use egui::{LayerId, Rect};
use suhina_style::cascade;
pub use suhina_style::cascade::{ExplainedDeclaration, Layer, MatchedRule, Outcome};

use super::{ENGINE, name_of, native};

/// An element of the last frame.
#[derive(Clone, Debug, PartialEq)]
pub struct FrameElement {
    /// The registry's name (`button`).
    pub name: &'static str,
    /// The id a sheet can select it by (`play` for `#play`), when the registry gives it one.
    pub id: Option<String>,
    /// Its attributes, the app's states among them (`checked`, `selected`), as selectors see them.
    pub attributes: Vec<(String, String)>,
    /// Its pointer and focus states (`hover`, `active`, `focus`, `disabled`).
    pub states: Vec<String>,
    /// The element it is in, by its index in [`frame_elements`]; `None` at the top.
    pub parent: Option<usize>,
    /// Its padding box as far as it shows, and the layer it was painted on; `None` for an element
    /// that painted nothing of its own and gave no box.
    pub painted: Option<(Rect, LayerId)>,
}

impl FrameElement {
    /// The element as a selector that matches it: `button#play[checked]:hover`.
    pub fn selector(&self) -> String {
        let mut text = self.name.to_owned();
        if let Some(id) = &self.id {
            text.push('#');
            text.push_str(id);
        }
        for (name, value) in &self.attributes {
            if value.is_empty() {
                text.push_str(&format!("[{name}]"));
            } else {
                text.push_str(&format!("[{name}={value:?}]"));
            }
        }
        for state in &self.states {
            text.push(':');
            text.push_str(state);
        }
        text
    }
}

/// The elements of the last frame, in the order they were entered: an element's children come
/// after it, and of two overlapping elements on one layer the later one is painted over the
/// earlier. Empty when no theme sheet styles the window.
pub fn frame_elements() -> Vec<FrameElement> {
    ENGINE.with(|engine| {
        let engine = engine.borrow();
        let Some((tree, _, _)) = native::frame_tree(&engine) else { return Vec::new() };
        engine
            .nodes
            .iter()
            .enumerate()
            .filter_map(|(at, node)| {
                let element = tree.elements().get(at + native::FRAME_ROOTS)?;
                Some(FrameElement {
                    name: node.element.name(),
                    id: suhina_theme_package::elements::element_id(node.element.name(), name_of(&engine, node)),
                    attributes: element.attributes.clone(),
                    states: element.states.iter().filter(|state| *state != "enabled").cloned().collect(),
                    parent: node.parent,
                    painted: engine.boxes.get(at).copied().flatten(),
                })
            })
            .collect()
    })
}

/// Why the element at `index` of [`frame_elements`] looks as it does.
#[derive(Clone, Debug, PartialEq)]
pub struct Explanation {
    /// The rules that match it, in the order the cascade weighs them (a later one wins a tie),
    /// with what became of each declaration.
    pub rules: Vec<MatchedRule>,
    /// Whether the style it was painted with differs from the one worked out here: it was held
    /// for a transition, or styled before a change the frame made.
    pub painted_differs: bool,
}

/// The rules of the cascade behind the element at `index` of [`frame_elements`], from the same
/// tree and sheets the frame was styled with.
pub fn explain(index: usize) -> Result<Explanation, String> {
    ENGINE.with(|engine| {
        let engine = engine.borrow();
        let (tree, devices, _) = native::frame_tree(&engine).ok_or("no theme sheet styles the window")?;
        if index >= engine.nodes.len() {
            return Err(format!("no element {index} in the last frame"));
        }
        let (style, rules) = native::explain(&engine, &tree, &devices, index + native::FRAME_ROOTS)?;
        let painted = engine.served.get(index).cloned().flatten();
        Ok(Explanation { rules, painted_differs: painted.is_some_and(|painted| *painted != style) })
    })
}

/// The window's theme sheet, as it styled the last frame: its [`suhina_style::sheet::Sheet::id`]
/// and each rule's line and selector, in order. `None` when no theme sheet styles the window.
pub fn theme_rules() -> Option<(u64, Vec<(u32, String)>)> {
    ENGINE.with(|engine| {
        let engine = engine.borrow();
        let sheet = native::theme_sheet(&engine)?;
        Some((sheet.id(), sheet.rule_sources().map(|(line, selector)| (line, selector.to_owned())).collect()))
    })
}

/// Records on this thread, from now on, which rules every style worked out matched (`true`), or
/// stops (`false`): [`cascade::record_matches`].
pub fn record_matches(on: bool) {
    cascade::record_matches(on);
}

/// The rules matched since [`record_matches`] was turned on: [`cascade::matched_rules`].
pub fn matched_rules() -> std::collections::BTreeSet<(u64, usize)> {
    cascade::matched_rules()
}
