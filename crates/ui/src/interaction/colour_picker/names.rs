// Copyright (C) 2026 The Suhina contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! Human-readable CSS colour names; matching uses the shared perceptual conversion.

use suhina_engine::model::Rgb;

pub(super) fn describe(Rgb(r, g, b): Rgb) -> String {
    let (name, exact) = suhina_style::value::nearest_named_colour([r, g, b]);
    let label = words(name);
    if exact { label } else { format!("≈ {label}") }
}

fn words(name: &str) -> String {
    for prefix in ["light", "dark", "medium", "pale", "deep"] {
        if let Some(rest) = name.strip_prefix(prefix).filter(|rest| !rest.is_empty()) {
            return format!("{} {}", capitalise(prefix), words(rest));
        }
    }
    match name {
        "lemonchiffon" => "Lemon Chiffon".into(),
        "blanchedalmond" => "Blanched Almond".into(),
        "lavenderblush" => "Lavender Blush".into(),
        "mintcream" => "Mint Cream".into(),
        "mistyrose" => "Misty Rose".into(),
        "oldlace" => "Old Lace".into(),
        "olivedrab" => "Olive Drab".into(),
        "papayawhip" => "Papaya Whip".into(),
        "peachpuff" => "Peach Puff".into(),
        "whitesmoke" => "White Smoke".into(),
        _ => {
            for suffix in ["white", "grey", "blue", "green", "yellow", "red", "brown", "pink", "violet"] {
                if let Some(start) = name.strip_suffix(suffix).filter(|start| !start.is_empty()) {
                    return format!("{} {}", capitalise(start), capitalise(suffix));
                }
            }
            capitalise(name)
        }
    }
}

fn capitalise(name: &str) -> String {
    let mut letters = name.chars();
    match letters.next() {
        Some(first) => first.to_uppercase().chain(letters).collect(),
        None => String::new(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn colour_names_are_readable_and_approximations_are_explicit() {
        assert_eq!(describe(Rgb(255, 215, 0)), "Gold");
        assert_eq!(describe(Rgb(240, 230, 140)), "Khaki");
        assert_eq!(describe(Rgb(254, 214, 1)), "≈ Gold");
        assert_eq!(words("lightgoldenrodyellow"), "Light Goldenrod Yellow");
        assert_eq!(words("mediumslateblue"), "Medium Slate Blue");
        assert_eq!(words("lemonchiffon"), "Lemon Chiffon");
    }
}
