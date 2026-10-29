// Copyright (C) 2026 The Suhina contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! Registered custom properties (`@property`): a type, an initial value and Suhina's own
//! descriptors for the settings panel (`label`, `group`, `range`, `step`).
//!
//! ```css
//! @property --glow { syntax: "<number>"; initial-value: 0.6; label: "Glow"; range: 0 1; }
//! ```
//!
//! The initial value is what an element sees when no rule sets the property, so a theme's
//! settings work before any value is chosen. A value given from outside the sheet (a theme
//! variant) is checked against `syntax` with [`Registered::accepts`].

/// Most `@property` rules one sheet may hold.
pub const MAX_REGISTERED: usize = 256;

/// One part of a `syntax` descriptor.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Component {
    Color,
    Number,
    Integer,
    Length,
    Percentage,
    LengthPercentage,
    Angle,
    String,
    Image,
    Keyword(String),
}

/// A `syntax` descriptor: `*`, or alternatives joined by `|`, each optionally repeated (`+`).
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Syntax {
    Any,
    Alternatives(Vec<(Component, bool)>),
}

impl Syntax {
    pub fn parse(text: &str) -> Result<Self, String> {
        let text = text.trim().trim_matches(['"', '\'']).trim();
        if text == "*" {
            return Ok(Self::Any);
        }
        let mut alternatives = Vec::new();
        for part in text.split('|').map(str::trim) {
            let (part, repeated) = part.strip_suffix('+').map_or((part, false), |p| (p.trim(), true));
            let component = match part {
                "<color>" => Component::Color,
                "<number>" => Component::Number,
                "<integer>" => Component::Integer,
                "<length>" => Component::Length,
                "<percentage>" => Component::Percentage,
                "<length-percentage>" => Component::LengthPercentage,
                "<angle>" => Component::Angle,
                "<string>" => Component::String,
                "<image>" => Component::Image,
                keyword
                    if !keyword.is_empty()
                        && keyword.len() <= 64
                        && keyword.bytes().all(|b| b.is_ascii_alphanumeric() || b == b'-')
                        && !keyword.starts_with(|c: char| c.is_ascii_digit()) =>
                {
                    Component::Keyword(keyword.to_owned())
                }
                other => return Err(format!("Unsupported syntax component {other}")),
            };
            alternatives.push((component, repeated));
            if alternatives.len() > 32 {
                return Err("A syntax has at most 32 alternatives".into());
            }
        }
        Ok(Self::Alternatives(alternatives))
    }

    /// Whether `value` (with no `var()` left in it) matches this syntax.
    pub fn accepts(&self, value: &str) -> bool {
        let value = value.trim();
        match self {
            Self::Any => !value.is_empty(),
            Self::Alternatives(alternatives) => alternatives.iter().any(|(component, repeated)| {
                if *repeated {
                    let parts: Vec<&str> = value.split_whitespace().collect();
                    !parts.is_empty() && parts.len() <= 32 && parts.iter().all(|part| component.accepts(part))
                } else {
                    component.accepts(value)
                }
            }),
        }
    }

    /// The keywords of a syntax made only of keywords, for a choice in the settings panel.
    pub fn keywords(&self) -> Option<Vec<&str>> {
        match self {
            Self::Any => None,
            Self::Alternatives(alternatives) => alternatives
                .iter()
                .map(|(component, repeated)| match component {
                    Component::Keyword(keyword) if !repeated => Some(keyword.as_str()),
                    _ => None,
                })
                .collect(),
        }
    }
}

impl Component {
    fn accepts(&self, value: &str) -> bool {
        use crate::value::{Color, Length};
        let number = |text: &str| text.parse::<f32>().ok().filter(|n| n.is_finite());
        match self {
            Self::Color => Color::parse(value).is_ok(),
            Self::Number => number(value).is_some(),
            Self::Integer => value.parse::<i64>().is_ok(),
            Self::Percentage => value.strip_suffix('%').and_then(number).is_some(),
            Self::Length => {
                value.ends_with(|c: char| c.is_ascii_alphabetic()) && Length::parse(value).is_ok() || value == "0"
            }
            Self::LengthPercentage => {
                Self::Length.accepts(value) || Self::Percentage.accepts(value) || Length::parse(value).is_ok()
            }
            Self::Angle => {
                ["deg", "turn", "rad", "grad"].iter().any(|unit| value.strip_suffix(unit).and_then(number).is_some())
            }
            Self::String => {
                value.len() >= 2
                    && (value.starts_with('"') && value.ends_with('"')
                        || value.starts_with('\'') && value.ends_with('\''))
            }
            Self::Image => crate::gradient::parse(value).is_ok_and(|gradients| !gradients.is_empty()),
            Self::Keyword(keyword) => value == keyword,
        }
    }
}

/// One `@property` rule.
#[derive(Clone, Debug, PartialEq)]
pub struct Registered {
    /// The custom property's name, with its `--`.
    pub name: String,
    pub syntax: Syntax,
    /// `initial-value`; required unless the syntax is `*`.
    pub initial: Option<String>,
    pub inherits: bool,
    /// The settings panel's label; a property without one is not shown there.
    pub label: Option<String>,
    pub group: Option<String>,
    /// `range: <min> <max>` for a number, integer or length.
    pub range: Option<(f32, f32)>,
    pub step: Option<f32>,
}

// `range` and `step` are finite numbers (the parser refuses others), so equality is total.
impl Eq for Registered {}

impl Registered {
    /// Reads the descriptors of `@property <name> { ... }`, given as `(descriptor, value)` pairs.
    pub fn from_descriptors(name: &str, descriptors: &[(String, String)]) -> Result<Self, String> {
        if !name.starts_with("--") || name.len() > 128 || name.len() < 3 {
            return Err(format!("@property needs a custom property name, not {name}"));
        }
        let mut syntax = None;
        let mut registered = Self {
            name: name.to_owned(),
            syntax: Syntax::Any,
            initial: None,
            inherits: true,
            label: None,
            group: None,
            range: None,
            step: None,
        };
        let text = |value: &str| {
            let value = value.trim();
            let inner = value
                .strip_prefix('"')
                .and_then(|v| v.strip_suffix('"'))
                .or_else(|| value.strip_prefix('\'').and_then(|v| v.strip_suffix('\'')))
                .ok_or("expected a quoted string")?;
            (inner.chars().count() <= 64 && !inner.chars().any(char::is_control))
                .then(|| inner.to_owned())
                .ok_or("a label is at most 64 characters")
        };
        let number = |value: &str| value.trim().parse::<f32>().ok().filter(|n| n.is_finite());
        for (descriptor, value) in descriptors {
            match descriptor.as_str() {
                "syntax" => syntax = Some(Syntax::parse(value)?),
                "initial-value" => {
                    if value.contains("var(") || crate::variables::mentions_attr(value) || value.len() > 1024 {
                        return Err(format!("{name}: initial-value may not use var() or attr()"));
                    }
                    registered.initial = Some(value.trim().to_owned());
                }
                "inherits" => {
                    registered.inherits = match value.trim() {
                        "true" => true,
                        "false" => false,
                        _ => return Err(format!("{name}: inherits is true or false")),
                    }
                }
                "label" => registered.label = Some(text(value).map_err(|e| format!("{name}: label: {e}"))?),
                "group" => registered.group = Some(text(value).map_err(|e| format!("{name}: group: {e}"))?),
                "range" => {
                    let parts: Vec<f32> = value.split_whitespace().filter_map(number).collect();
                    match parts[..] {
                        [min, max] if min < max => registered.range = Some((min, max)),
                        _ => return Err(format!("{name}: range is two numbers, the smaller first")),
                    }
                }
                "step" => {
                    registered.step =
                        Some(number(value).filter(|n| *n > 0.0).ok_or(format!("{name}: step is a number"))?)
                }
                other => return Err(format!("{name}: unknown @property descriptor {other}")),
            }
        }
        registered.syntax = syntax.ok_or(format!("{name}: @property needs a syntax"))?;
        match &registered.initial {
            Some(initial) if !registered.accepts(initial) => {
                Err(format!("{name}: initial-value {initial} does not match its syntax"))
            }
            None if registered.syntax != Syntax::Any => Err(format!("{name}: @property needs an initial-value")),
            _ => Ok(registered),
        }
    }

    /// Whether `value` is a value of this property: its syntax, and within its range.
    pub fn accepts(&self, value: &str) -> bool {
        if !self.syntax.accepts(value) {
            return false;
        }
        match self.range {
            Some((min, max)) => {
                let number = value.trim().trim_end_matches(|c: char| c.is_ascii_alphabetic() || c == '%');
                number.parse::<f32>().map_or(true, |n| (min..=max).contains(&n))
            }
            None => true,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn descriptors(pairs: &[(&str, &str)]) -> Vec<(String, String)> {
        pairs.iter().map(|(a, b)| ((*a).to_owned(), (*b).to_owned())).collect()
    }

    #[test]
    fn a_property_reads_its_type_initial_value_and_panel_descriptors() {
        let glow = Registered::from_descriptors(
            "--glow",
            &descriptors(&[
                ("syntax", "\"<number>\""),
                ("initial-value", "0.6"),
                ("label", "\"Glow\""),
                ("group", "\"Effects\""),
                ("range", "0 1"),
                ("step", "0.05"),
            ]),
        )
        .unwrap();
        assert_eq!(glow.initial.as_deref(), Some("0.6"));
        assert_eq!(glow.label.as_deref(), Some("Glow"));
        assert_eq!(glow.range, Some((0.0, 1.0)));
        assert!(glow.accepts("0.25") && !glow.accepts("2") && !glow.accepts("bright"));
    }

    #[test]
    fn syntaxes_accept_their_own_values_only() {
        let color = Syntax::parse("\"<color>\"").unwrap();
        assert!(
            color.accepts("#4af") && color.accepts("color-mix(in oklch, #fff, #000 20%)") && !color.accepts("12px")
        );
        let meter = Syntax::parse("continuous | segmented | needle").unwrap();
        assert!(meter.accepts("needle") && !meter.accepts("frames"));
        assert_eq!(meter.keywords(), Some(vec!["continuous", "segmented", "needle"]));
        let liquid = Syntax::parse("auto | <integer>+").unwrap();
        assert!(liquid.accepts("auto") && liquid.accepts("30 0 10") && !liquid.accepts("30 x"));
        assert!(Syntax::parse("<image> | none").unwrap().accepts("linear-gradient(90deg, #000 0%, #fff 100%)"));
        assert!(
            Syntax::parse("<length>").unwrap().accepts("12px") && !Syntax::parse("<length>").unwrap().accepts("12")
        );
        assert!(Syntax::parse("<string>").unwrap().accepts("\"Nord\""));
        assert!(Syntax::parse("<bogus>").is_err());
    }

    #[test]
    fn a_property_without_a_valid_initial_value_is_refused() {
        let missing = Registered::from_descriptors("--a", &descriptors(&[("syntax", "\"<color>\"")]));
        assert!(missing.is_err());
        let wrong =
            Registered::from_descriptors("--a", &descriptors(&[("syntax", "\"<color>\""), ("initial-value", "12px")]));
        assert!(wrong.is_err());
        let variable =
            Registered::from_descriptors("--a", &descriptors(&[("syntax", "\"*\""), ("initial-value", "var(--b)")]));
        assert!(variable.is_err());
        assert!(Registered::from_descriptors("color", &descriptors(&[("syntax", "\"*\"")])).is_err());
    }
}
