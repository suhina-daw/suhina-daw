// Copyright (C) 2026 The Suhina contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! Familiar survey color names; matching uses the shared perceptual conversion.

use suhina_engine::model::Rgb;

pub(super) fn describe(Rgb(r, g, b): Rgb) -> String {
    suhina_style::value::nearest_named_color([r, g, b]).0.to_owned()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn color_names_are_the_nearest_without_a_mark() {
        assert_eq!(describe(Rgb(219, 180, 12)), "Gold");
        assert_eq!(describe(Rgb(170, 166, 98)), "Khaki");
        assert_eq!(describe(Rgb(206, 179, 1)), "Mustard");
        assert_eq!(describe(Rgb(219, 180, 13)), "Gold");
    }
}
