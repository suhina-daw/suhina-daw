// Copyright (C) 2026 The Suhina contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! Native viewport predicates. Layout still resolves dimensions on every size change.
use crate::value::Length;

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Viewport {
    pub width: f32,
    pub height: f32,
    pub dark: bool,
    pub reduced_motion: bool,
}

impl Viewport {
    pub fn valid(self) -> bool {
        self.width.is_finite()
            && self.height.is_finite()
            && (0.0..=16384.0).contains(&self.width)
            && (0.0..=16384.0).contains(&self.height)
    }
}

#[derive(Clone, Debug)]
enum Feature {
    Width(f32, bool),
    Height(f32, bool),
    Dark(bool),
    Motion(bool),
    /// A range comparison of the width (axis 0) or height (axis 1): `size op dimension`.
    Range {
        axis: usize,
        op: Op,
        size: f32,
    },
    Landscape(bool),
}

#[derive(Clone, Copy, Debug)]
enum Op {
    Less,
    LessEqual,
    Greater,
    GreaterEqual,
    Equal,
}

impl Op {
    fn holds(self, a: f32, b: f32) -> bool {
        match self {
            Self::Less => a < b,
            Self::LessEqual => a <= b,
            Self::Greater => a > b,
            Self::GreaterEqual => a >= b,
            Self::Equal => a == b,
        }
    }
    fn flipped(self) -> Self {
        match self {
            Self::Less => Self::Greater,
            Self::LessEqual => Self::GreaterEqual,
            Self::Greater => Self::Less,
            Self::GreaterEqual => Self::LessEqual,
            Self::Equal => Self::Equal,
        }
    }
}

fn media_px(value: &str) -> Result<f32, String> {
    let Length::Px(size) = Length::parse(value.trim())? else {
        return Err("Media dimensions require px".into());
    };
    if size < 0.0 {
        return Err("Negative media dimension".into());
    }
    Ok(size)
}

/// `width > 1px`, `1px < width`, `400px <= width <= 800px`, also for `height`.
fn range(source: &str, features: &mut Vec<Feature>) -> Result<(), String> {
    let mut terms = Vec::new();
    let mut ops = Vec::new();
    let mut current = String::new();
    let mut chars = source.chars().peekable();
    while let Some(c) = chars.next() {
        if matches!(c, '<' | '>' | '=') {
            let equal = c != '=' && chars.peek() == Some(&'=');
            if equal {
                chars.next();
            }
            ops.push(match (c, equal) {
                ('<', false) => Op::Less,
                ('<', true) => Op::LessEqual,
                ('>', false) => Op::Greater,
                ('>', true) => Op::GreaterEqual,
                _ => Op::Equal,
            });
            terms.push(std::mem::take(&mut current).trim().to_owned());
        } else {
            current.push(c);
        }
    }
    terms.push(current.trim().to_owned());
    let axis = |name: &str| match name {
        "width" => Some(0),
        "height" => Some(1),
        _ => None,
    };
    match (terms.as_slice(), ops.as_slice()) {
        ([name, value], [op]) if axis(name).is_some() => {
            features.push(Feature::Range { axis: axis(name).unwrap_or(0), op: *op, size: media_px(value)? });
        }
        ([value, name], [op]) if axis(name).is_some() => {
            features.push(Feature::Range { axis: axis(name).unwrap_or(0), op: op.flipped(), size: media_px(value)? });
        }
        ([low, name, high], [a, b]) if axis(name).is_some() && !matches!(a, Op::Equal) && !matches!(b, Op::Equal) => {
            let axis = axis(name).unwrap_or(0);
            features.push(Feature::Range { axis, op: a.flipped(), size: media_px(low)? });
            features.push(Feature::Range { axis, op: *b, size: media_px(high)? });
        }
        _ => return Err("Unsupported native media range".into()),
    }
    Ok(())
}

#[derive(Clone, Debug)]
pub struct Media {
    alternatives: Vec<Vec<Feature>>,
}

impl Media {
    pub fn parse(source: &str) -> Result<Self, String> {
        let mut alternatives = Vec::new();
        for alternative in source.split(',') {
            let mut features = Vec::new();
            for part in alternative.trim().split(" and ") {
                let part = part.trim();
                if matches!(part, "screen" | "all") {
                    continue;
                }
                let inner = part
                    .strip_prefix('(')
                    .and_then(|s| s.strip_suffix(')'))
                    .ok_or("Unsupported native media condition")?;
                if !inner.contains(':') && inner.contains(['<', '>', '=']) {
                    range(inner, &mut features)?;
                    if features.len() > 16 {
                        return Err("Too many media conditions".into());
                    }
                    continue;
                }
                let (name, value) = inner.split_once(':').ok_or("Unsupported native media condition")?;
                let name = name.trim();
                let value = value.trim();
                let feature = match name {
                    "prefers-color-scheme" => Feature::Dark(match value {
                        "dark" => true,
                        "light" => false,
                        _ => return Err("Unknown colour scheme".into()),
                    }),
                    "prefers-reduced-motion" => Feature::Motion(match value {
                        "reduce" => true,
                        "no-preference" => false,
                        _ => return Err("Unknown motion preference".into()),
                    }),
                    "orientation" => Feature::Landscape(match value {
                        "landscape" => true,
                        "portrait" => false,
                        _ => return Err("Unknown orientation".into()),
                    }),
                    "min-width" | "max-width" | "min-height" | "max-height" => {
                        let Length::Px(size) = Length::parse(value)? else {
                            return Err("Media dimensions require px".into());
                        };
                        if size < 0.0 {
                            return Err("Negative media dimension".into());
                        }
                        if name.ends_with("width") {
                            Feature::Width(size, name.starts_with("min"))
                        } else {
                            Feature::Height(size, name.starts_with("min"))
                        }
                    }
                    _ => return Err(format!("Unknown native media feature {name}")),
                };
                features.push(feature);
                if features.len() > 16 {
                    return Err("Too many media conditions".into());
                }
            }
            if alternative.trim().is_empty() {
                return Err("Empty media condition".into());
            }
            alternatives.push(features);
            if alternatives.len() > 16 {
                return Err("Too many media alternatives".into());
            }
        }
        Ok(Self { alternatives })
    }

    pub fn matches(&self, viewport: Viewport) -> bool {
        viewport.valid()
            && self.alternatives.iter().any(|features| {
                features.iter().all(|feature| match feature {
                    Feature::Width(size, minimum) => {
                        if *minimum {
                            viewport.width >= *size
                        } else {
                            viewport.width <= *size
                        }
                    }
                    Feature::Height(size, minimum) => {
                        if *minimum {
                            viewport.height >= *size
                        } else {
                            viewport.height <= *size
                        }
                    }
                    Feature::Dark(dark) => viewport.dark == *dark,
                    Feature::Range { axis, op, size } => {
                        op.holds(if *axis == 0 { viewport.width } else { viewport.height }, *size)
                    }
                    Feature::Landscape(landscape) => (viewport.width >= viewport.height) == *landscape,
                    Feature::Motion(reduced) => viewport.reduced_motion == *reduced,
                })
            })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    fn at(width: f32, height: f32) -> Viewport {
        Viewport { width, height, dark: true, reduced_motion: false }
    }
    #[test]
    fn ranges_orientation_and_classic_forms_agree() {
        let wide = Media::parse("(width > 1px)").unwrap();
        assert!(wide.matches(at(800.0, 600.0)));
        let band = Media::parse("(400px <= width <= 800px)").unwrap();
        assert!(band.matches(at(400.0, 1.0)) && band.matches(at(800.0, 1.0)) && !band.matches(at(801.0, 1.0)));
        let reversed = Media::parse("(760px >= height)").unwrap();
        assert!(reversed.matches(at(1.0, 700.0)) && !reversed.matches(at(1.0, 800.0)));
        assert!(Media::parse("(orientation: portrait)").unwrap().matches(at(400.0, 800.0)));
        assert!(Media::parse("(max-width: 500px) and (orientation: landscape)").unwrap().matches(at(500.0, 400.0)));
        for bad in ["(depth > 1px)", "(width >)", "(1px < width = 2px)", "(width > 1em)"] {
            assert!(Media::parse(bad).is_err(), "{bad}");
        }
    }
}
