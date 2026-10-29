// Copyright (C) 2026 The Suhina contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! `var()` in SVG artwork. The SVG renderer reads plain colours only, so a value that is a colour
//! (`color-mix(...)`, `contrast-color(...)`, a name) goes in as the computed `#rrggbb[aa]`.
use std::{borrow::Cow, collections::BTreeMap};

/// Deepest `var()` inside another's fallback.
const MAX_DEPTH: usize = 16;

/// Replaces `var(--name)` and `var(--name, fallback)` with the resolved `variables`; a fallback
/// may contain brackets (`var(--x, rgb(0 0 0))`) and further `var()`. An unknown name without a
/// fallback, or an unclosed `var(`, stays as written.
pub fn substitute<'a>(source: &'a str, variables: &BTreeMap<String, String>) -> Cow<'a, str> {
    if !source.contains("var(--") {
        return Cow::Borrowed(source);
    }
    Cow::Owned(expand(source, variables, 0))
}

fn expand(source: &str, variables: &BTreeMap<String, String>, depth: usize) -> String {
    let mut out = String::with_capacity(source.len());
    let mut rest = source;
    while let Some(start) = rest.find("var(--") {
        out.push_str(&rest[..start]);
        let inner_start = start + "var(".len();
        let Some(close) = closing(&rest[inner_start..]) else {
            out.push_str(&rest[start..]);
            return out;
        };
        let inner = &rest[inner_start..inner_start + close];
        let (name, fallback) = match top_level_comma(inner) {
            Some(comma) => (inner[..comma].trim(), Some(inner[comma + 1..].trim())),
            None => (inner.trim(), None),
        };
        let value = match (variables.get(name), fallback) {
            (Some(value), _) => Some(value.clone()),
            (None, Some(fallback)) if depth < MAX_DEPTH => Some(expand(fallback, variables, depth + 1)),
            _ => None,
        };
        match value {
            Some(value) => out.push_str(&computed(&value)),
            None => out.push_str(&rest[start..inner_start + close + 1]),
        }
        rest = &rest[inner_start + close + 1..];
    }
    out.push_str(rest);
    out
}

/// The index of the `)` that closes a bracket opened just before `text`.
fn closing(text: &str) -> Option<usize> {
    let mut depth = 0usize;
    for (index, byte) in text.bytes().enumerate() {
        match byte {
            b'(' => depth += 1,
            b')' if depth == 0 => return Some(index),
            b')' => depth -= 1,
            _ => {}
        }
    }
    None
}

fn top_level_comma(text: &str) -> Option<usize> {
    let mut depth = 0usize;
    for (index, byte) in text.bytes().enumerate() {
        match byte {
            b'(' => depth += 1,
            b')' => depth = depth.saturating_sub(1),
            b',' if depth == 0 => return Some(index),
            _ => {}
        }
    }
    None
}

/// A colour as the renderer reads it; any other value as it is.
fn computed(value: &str) -> Cow<'_, str> {
    match crate::value::Color::parse(value.trim()) {
        Ok(crate::value::Color::Rgba([r, g, b, 255])) => Cow::Owned(format!("#{r:02x}{g:02x}{b:02x}")),
        Ok(crate::value::Color::Rgba([r, g, b, a])) => Cow::Owned(format!("#{r:02x}{g:02x}{b:02x}{a:02x}")),
        _ => Cow::Borrowed(value),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn variables() -> BTreeMap<String, String> {
        [
            ("--accent", "#FF0000"),
            ("--mix", "color-mix(in srgb, red, blue)"),
            ("--glass", "rgb(0 0 255 / 50%)"),
            ("--on-accent", "contrast-color(#ffff00)"),
            ("--width", "2.5"),
        ]
        .into_iter()
        .map(|(name, value)| (name.to_owned(), value.to_owned()))
        .collect()
    }

    #[test]
    fn colours_go_in_computed_and_other_values_as_they_are() {
        let svg = r#"<rect fill="var(--mix)" stroke="var(--accent)" stroke-width="var(--width)"/>"#;
        assert_eq!(substitute(svg, &variables()), r##"<rect fill="#800080" stroke="#ff0000" stroke-width="2.5"/>"##);
        assert_eq!(substitute("var(--glass) var(--on-accent)", &variables()), "#0000ff80 #000000");
        assert!(matches!(substitute("<svg/>", &variables()), Cow::Borrowed(_)));
    }

    #[test]
    fn fallbacks_may_hold_brackets_and_further_variables() {
        let variables = variables();
        assert_eq!(substitute(r#"fill="var(--none, rgb(0 0 0))""#, &variables), r##"fill="#000000""##);
        assert_eq!(substitute("var(--none, color-mix(in srgb, var(--accent), rgb(0 0 255)))", &variables), "#800080");
        assert_eq!(substitute("var(--none, var(--other, 3))", &variables), "3");
        assert_eq!(substitute("a var(--none) b", &variables), "a var(--none) b");
        assert_eq!(substitute("var(--accent, rgb(0 0 0)) var(--width)", &variables), "#ff0000 2.5");
        assert_eq!(substitute("x var(--none, rgb(0 0 0)", &variables), "x var(--none, rgb(0 0 0)");
        let deep = format!("{}1{}", "var(--n, ".repeat(40), ")".repeat(40));
        assert!(substitute(&deep, &variables).contains("var(--n"));
    }
}
