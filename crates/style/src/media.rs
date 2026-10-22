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
                let (name, value) = part
                    .strip_prefix('(')
                    .and_then(|s| s.strip_suffix(')'))
                    .and_then(|s| s.split_once(':'))
                    .ok_or("Unsupported native media condition")?;
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
                    Feature::Motion(reduced) => viewport.reduced_motion == *reduced,
                })
            })
    }
}
