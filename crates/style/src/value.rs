// Copyright (C) 2026 The Suhina contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! Typed native values. No renderer, pixels, or toolkit-specific colour types.
use cssparser::{Parser, ParserInput, Token};

#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Color {
    Rgba([u8; 4]),
    Current,
}

impl Color {
    pub fn parse(source: &str) -> Result<Self, String> {
        let mut input = ParserInput::new(source);
        let mut parser = Parser::new(&mut input);
        let color = match parser.next().map_err(|_| "Expected colour")?.clone() {
            Token::Hash(hex) | Token::IDHash(hex) => {
                let (r, g, b, a) =
                    cssparser::color::parse_hash_color(hex.as_bytes()).map_err(|_| "Invalid hex colour")?;
                Self::Rgba([r, g, b, cssparser::color::clamp_unit_f32(a)])
            }
            Token::Ident(name) if name.eq_ignore_ascii_case("currentcolor") => Self::Current,
            Token::Ident(name) if name.eq_ignore_ascii_case("transparent") => Self::Rgba([0; 4]),
            Token::Ident(name) => {
                let (r, g, b) =
                    cssparser::color::parse_named_color(&name.to_ascii_lowercase()).map_err(|_| "Unknown colour")?;
                Self::Rgba([r, g, b, 255])
            }
            Token::Function(name) if name.eq_ignore_ascii_case("rgb") || name.eq_ignore_ascii_case("rgba") => {
                let mut value = None;
                parser
                    .parse_nested_block(|parser| {
                        value = Some(rgb(parser));
                        Ok::<(), cssparser::ParseError<'_, ()>>(())
                    })
                    .map_err(|_| "Invalid rgb()")?;
                Self::Rgba(value.ok_or("Empty rgb()")??)
            }
            _ => return Err("Unsupported colour value".into()),
        };
        parser.expect_exhausted().map_err(|_| "Unexpected colour suffix")?;
        Ok(color)
    }
}

fn rgb(parser: &mut Parser<'_, '_>) -> Result<[u8; 4], String> {
    let component = |parser: &mut Parser<'_, '_>, alpha: bool| -> Result<u8, String> {
        let value = match parser.next().map_err(|_| "Missing rgb component")? {
            Token::Number { value, .. } => *value * if alpha { 255.0 } else { 1.0 },
            Token::Percentage { unit_value, .. } => *unit_value * 255.0,
            _ => return Err("Expected a numeric rgb component".into()),
        };
        if !value.is_finite() {
            return Err("Non-finite colour".into());
        }
        Ok(cssparser::color::clamp_floor_256_f32(value))
    };
    let red = component(parser, false)?;
    let comma = parser.try_parse(|p| p.expect_comma()).is_ok();
    let green = component(parser, false)?;
    if comma {
        parser.expect_comma().map_err(|_| "Expected comma")?;
    }
    let blue = component(parser, false)?;
    let alpha = if parser.is_exhausted() {
        255
    } else {
        if comma {
            parser.expect_comma().map_err(|_| "Expected alpha comma")?;
        } else {
            parser.expect_delim('/').map_err(|_| "Expected alpha slash")?;
        }
        component(parser, true)?
    };
    parser.expect_exhausted().map_err(|_| "Unexpected rgb component")?;
    Ok([red, green, blue, alpha])
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Length {
    Px(f32),
    Em(f32),
    Percent(f32),
    Fr(f32),
    Auto,
}

impl Length {
    pub fn parse(source: &str) -> Result<Self, String> {
        let mut input = ParserInput::new(source);
        let mut parser = Parser::new(&mut input);
        let length = match parser.next().map_err(|_| "Expected length")? {
            Token::Number { value, .. } if *value == 0.0 => Self::Px(0.0),
            Token::Dimension { value, unit, .. } if value.is_finite() && value.abs() <= 16384.0 => {
                match unit.as_ref() {
                    "px" => Self::Px(*value),
                    "em" => Self::Em(*value),
                    "fr" if *value >= 0.0 => Self::Fr(*value),
                    _ => return Err("Unsupported length unit".into()),
                }
            }
            Token::Percentage { unit_value, .. } if unit_value.is_finite() && unit_value.abs() <= 100.0 => {
                Self::Percent(*unit_value)
            }
            Token::Ident(value) if *value == "auto" => Self::Auto,
            _ => return Err("Invalid or excessive length".into()),
        };
        parser.expect_exhausted().map_err(|_| "Unexpected length suffix")?;
        Ok(length)
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Layout {
    Row,
    Column,
    Grid,
    Stack,
}

#[derive(Clone, Debug, PartialEq)]
pub enum Value {
    Color(Color),
    Length(Length),
    Number(f32),
    Integer(i32),
    Bool(bool),
    Layout(Layout),
    Tracks(Vec<Length>),
    Areas(crate::layout::Areas),
    Name(String),
    Shadows(Vec<crate::paint::Shadow>),
    Gradients(Vec<crate::gradient::Linear>),
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn native_values_preserve_alpha_and_reject_non_finite_sizes() {
        assert_eq!(Color::parse("#1230"), Ok(Color::Rgba([17, 34, 51, 0])));
        assert_eq!(Color::parse("rgb(100% 0% 50% / 50%)"), Ok(Color::Rgba([255, 0, 128, 128])));
        assert_eq!(Color::parse("rgba(255, 0, 128, .5)"), Ok(Color::Rgba([255, 0, 128, 128])));
        for bad in ["NaNpx", "1e99px", "-1fr", "calc(1px / 0)", "1px garbage"] {
            assert!(Length::parse(bad).is_err(), "{bad}");
        }
        assert!(Color::parse("rgb(1, 2 3)").is_err());
    }
}
