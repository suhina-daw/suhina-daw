// Copyright (C) 2026 The Suhina contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! Familiar survey colour names; matching uses the shared perceptual conversion.

use suhina_engine::model::Rgb;

pub(super) fn describe(Rgb(r, g, b): Rgb) -> String {
    let (name, exact) = suhina_style::value::nearest_named_colour([r, g, b]);
    if exact { name.to_owned() } else { format!("≈ {name}") }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn colour_names_are_readable_and_approximations_are_explicit() {
        assert_eq!(describe(Rgb(219, 180, 12)), "Gold");
        assert_eq!(describe(Rgb(170, 166, 98)), "Khaki");
        assert_eq!(describe(Rgb(206, 179, 1)), "Mustard");
        assert_eq!(describe(Rgb(219, 180, 13)), "≈ Gold");
    }
}
