// Copyright (C) 2026 The Suhina contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! Toolkit-independent paint values. Parsing and resolution stay off the audio thread.
use crate::value::{Color, Length};
use cssparser::{Parser, Token};

#[derive(Clone, Debug, PartialEq)]
pub struct Shadow {
    pub offset: [Length; 2],
    pub blur: Length,
    pub spread: Length,
    pub color: Color,
    pub inset: bool,
}

/// Resolved logical-point data consumed by both rendering backends.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct ResolvedShadow {
    pub offset: [f32; 2],
    pub blur: f32,
    pub spread: f32,
    pub color: [u8; 4],
    pub inset: bool,
}

impl Shadow {
    pub fn resolve(&self, font_size: f32, foreground: [u8; 4]) -> ResolvedShadow {
        let px = |length| match length {
            Length::Px(value) => value,
            Length::Em(value) => value * font_size,
            _ => 0.0,
        };
        ResolvedShadow {
            offset: self.offset.map(px),
            blur: px(self.blur),
            spread: px(self.spread),
            color: match self.color {
                Color::Rgba(rgba) => rgba,
                Color::Current => foreground,
            },
            inset: self.inset,
        }
    }
}

/// Most shadow layers a declaration may list, and most that are drawn.
pub const MAX_SHADOW_LAYERS: usize = 32;
pub const DRAWN_SHADOW_LAYERS: usize = 4;

/// A bounded subset shared by native app and SDK panels. Up to 32 layers parse; the first
/// four are drawn, so hostile lists cost fixed paint work.
pub fn shadows(source: &str, text: bool) -> Result<Vec<Shadow>, String> {
    crate::syntax::validate(source)?;
    if source.trim().eq_ignore_ascii_case("none") {
        return Ok(Vec::new());
    }
    let mut parser = Parser::new(source);
    let mut result = Vec::new();
    loop {
        if result.len() >= MAX_SHADOW_LAYERS {
            return Err("At most 32 shadow layers".into());
        }
        let mut lengths = Vec::new();
        let mut color = None;
        let mut inset = false;
        let mut finished = false;
        let mut lengths_finished = false;
        while !parser.is_exhausted() {
            parser.skip_whitespace();
            let start = parser.position();
            let token = parser.next().map_err(|_| "Expected shadow value")?.clone();
            if matches!(token, Token::Comma) {
                finished = true;
                break;
            }
            if let Token::Ident(name) = &token
                && name.eq_ignore_ascii_case("inset")
            {
                if text || inset {
                    return Err("Unexpected inset shadow".into());
                }
                inset = true;
                lengths_finished |= !lengths.is_empty();
                continue;
            }
            if matches!(token, Token::Function(_)) {
                crate::sheet::nested(&mut parser, crate::sheet::consume)?;
            }
            let value = parser.slice_from(start);
            if let Ok(length) = Length::parse(value) {
                if lengths_finished
                    || lengths.len() >= if text { 3 } else { 4 }
                    || !matches!(length, Length::Px(_) | Length::Em(_))
                {
                    return Err("Invalid shadow lengths".into());
                }
                lengths.push(length);
            } else {
                if color.is_some() {
                    return Err("More than one shadow colour".into());
                }
                color = Some(Color::parse(value)?);
                lengths_finished |= !lengths.is_empty();
            }
        }
        if lengths.len() < 2 {
            return Err("A shadow needs two offsets".into());
        }
        let blur = lengths.get(2).copied().unwrap_or(Length::Px(0.0));
        if matches!(blur, Length::Px(n) | Length::Em(n) if n < 0.0) {
            return Err("A shadow blur cannot be negative".into());
        }
        result.push(Shadow {
            offset: [
                *lengths.first().ok_or("Missing horizontal offset")?,
                *lengths.get(1).ok_or("Missing vertical offset")?,
            ],
            blur,
            spread: lengths.get(3).copied().unwrap_or(Length::Px(0.0)),
            color: color.unwrap_or(Color::Current),
            inset,
        });
        if !finished {
            break;
        }
        if parser.is_exhausted() {
            return Err("Missing shadow after comma".into());
        }
    }
    result.truncate(DRAWN_SHADOW_LAYERS);
    Ok(result)
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn shadow_lists_preserve_current_ink_relative_sizes_and_negative_spread() {
        let parsed = shadows("inset -1em 2px 3px -4px currentColor, rgba(1, 2, 3, .5) 0 1px", false).unwrap();
        assert_eq!(
            parsed[0].resolve(12.0, [10, 20, 30, 40]),
            ResolvedShadow { offset: [-12.0, 2.0], blur: 3.0, spread: -4.0, color: [10, 20, 30, 40], inset: true }
        );
        assert_eq!(parsed[1].resolve(12.0, [0; 4]).color, [1, 2, 3, 128]);
        assert!(shadows("none", false).unwrap().is_empty());
        assert_eq!(shadows("0 1px", true).unwrap()[0].color, Color::Current);
        assert_eq!(shadows(&["1px 1px red"; 8].join(","), true).unwrap().len(), 4, "drawn layers are bounded");
    }
    #[test]
    fn malformed_or_excessive_shadows_are_rejected() {
        for source in
            ["", "0", "0 0 -1px", "0 0,", "0 0 red blue", "0 red 0", "0 0 5%", "0 0 1e99px", "inset inset 0 0"]
        {
            assert!(shadows(source, false).is_err(), "{source}");
        }
        assert!(shadows(&["0 0"; 33].join(","), false).is_err());
        for source in ["inset 0 0", "0 0 1px 2px"] {
            assert!(shadows(source, true).is_err(), "{source}");
        }
    }
}
