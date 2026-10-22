// Copyright (C) 2026 The Suhina contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! Bounded native linear gradients, with premultiplied sRGB interpolation.
use crate::value::{Color, Length};
use cssparser::{Parser, ParserInput, Token};

#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Direction {
    Angle(f32),
    Corner([f32; 2]),
}
impl Direction {
    pub fn axis(self, size: [f32; 2]) -> [f32; 2] {
        match self {
            Self::Angle(radians) => [radians.sin(), -radians.cos()],
            Self::Corner([x, y]) => {
                let [x, y] = [x * size[1], y * size[0]];
                let length = x.hypot(y).max(f32::EPSILON);
                [x / length, y / length]
            }
        }
    }
}

#[derive(Clone, Debug, PartialEq)]
pub struct Stop {
    pub color: Color,
    pub position: Option<Length>,
}

#[derive(Clone, Debug, PartialEq)]
pub struct Linear {
    pub direction: Direction,
    pub repeating: bool,
    pub stops: Vec<Stop>,
}

/// At most four layers and 64 expanded stops per layer. Unsupported gradient
/// kinds return an error so the migration adapter can preserve the whole sheet.
pub fn parse(source: &str) -> Result<Vec<Linear>, String> {
    crate::syntax::validate(source)?;
    if source.trim().eq_ignore_ascii_case("none") {
        return Ok(Vec::new());
    }
    let mut input = ParserInput::new(source);
    let mut parser = Parser::new(&mut input);
    let mut layers = Vec::new();
    loop {
        if layers.len() >= 4 {
            return Err("At most four gradient layers are supported".into());
        }
        let name = parser.expect_function().map_err(|_| "Expected gradient function")?.to_ascii_lowercase();
        let repeating = match name.as_str() {
            "linear-gradient" => false,
            "repeating-linear-gradient" => true,
            _ => return Err("Gradient kind is not implemented by the native parser yet".into()),
        };
        let gradient = crate::sheet::nested(&mut parser, |parser| {
            let mut parts = Vec::new();
            while !parser.is_exhausted() {
                if parts.len() >= 65 {
                    return Err("Too many gradient items".into());
                }
                let start = parser.position();
                let mut end = start;
                let mut comma = false;
                while !parser.is_exhausted() {
                    let before = parser.position();
                    let token =
                        parser.next_including_whitespace_and_comments().map_err(|_| "Invalid gradient item")?.clone();
                    if matches!(token, Token::Comma) {
                        end = before;
                        comma = true;
                        break;
                    }
                    if matches!(token, Token::Function(_) | Token::ParenthesisBlock) {
                        crate::sheet::nested(parser, crate::sheet::consume)?;
                    }
                    end = parser.position();
                }
                let value = parser.slice(start..end).trim();
                if value.is_empty() || (comma && parser.is_exhausted()) {
                    return Err("Empty gradient item".into());
                }
                parts.push(value.to_string());
            }
            let mut direction = Direction::Angle(std::f32::consts::PI);
            let mut stops = Vec::new();
            for (index, part) in parts.iter().enumerate() {
                if index == 0
                    && let Ok(value) = parse_direction(part)
                {
                    direction = value;
                    continue;
                }
                stops.extend(parse_stop(part)?);
                if stops.len() > 64 {
                    return Err("Gradient exceeds 64 stops".into());
                }
            }
            if stops.len() < 2 {
                return Err("Gradient needs at least two colour stops".into());
            }
            Ok(Linear { direction, repeating, stops })
        })?;
        layers.push(gradient);
        if parser.is_exhausted() {
            break;
        }
        parser.expect_comma().map_err(|_| "Expected comma between gradients")?;
    }
    Ok(layers)
}

fn parse_direction(source: &str) -> Result<Direction, String> {
    if source == "in srgb" {
        return Ok(Direction::Angle(std::f32::consts::PI));
    }
    let source = source.strip_suffix(" in srgb").unwrap_or(source).trim_end();
    let mut input = ParserInput::new(source);
    let mut parser = Parser::new(&mut input);
    if parser.try_parse(|p| p.expect_ident_matching("to")).is_ok() {
        let mut x = 0.0f32;
        let mut y = 0.0f32;
        while !parser.is_exhausted() {
            match parser.expect_ident().map_err(|_| "Expected gradient direction")?.as_ref() {
                "left" if x == 0.0 => x = -1.0,
                "right" if x == 0.0 => x = 1.0,
                "top" if y == 0.0 => y = -1.0,
                "bottom" if y == 0.0 => y = 1.0,
                _ => return Err("Invalid gradient direction".into()),
            }
        }
        return if x == 0.0 && y == 0.0 {
            Err("Missing gradient direction".into())
        } else if x == 0.0 || y == 0.0 {
            Ok(Direction::Angle(x.atan2(-y)))
        } else {
            Ok(Direction::Corner([x, y]))
        };
    }
    let radians = match parser.next().map_err(|_| "Expected gradient angle")? {
        Token::Dimension { value, unit, .. } if value.is_finite() && value.abs() <= 1_000_000.0 => {
            match unit.as_ref() {
                "deg" => value.to_radians(),
                "rad" => *value,
                "turn" => value * std::f32::consts::TAU,
                "grad" => value * std::f32::consts::PI / 200.0,
                _ => return Err("Unknown angle unit".into()),
            }
        }
        Token::Number { value, .. } if *value == 0.0 => 0.0,
        _ => return Err("Expected gradient angle".into()),
    };
    parser.expect_exhausted().map_err(|_| "Unsupported gradient interpolation method")?;
    Ok(Direction::Angle(radians.rem_euclid(std::f32::consts::TAU)))
}

fn parse_stop(source: &str) -> Result<Vec<Stop>, String> {
    let mut input = ParserInput::new(source);
    let mut parser = Parser::new(&mut input);
    let start = parser.position();
    if matches!(parser.next().map_err(|_| "Missing gradient colour")?, Token::Function(_)) {
        crate::sheet::nested(&mut parser, crate::sheet::consume)?;
    }
    let color = Color::parse(parser.slice_from(start))?;
    let mut stops = Vec::new();
    while !parser.is_exhausted() {
        parser.skip_whitespace();
        if stops.len() >= 2 {
            return Err("Too many positions on a colour stop".into());
        }
        let start = parser.position();
        parser.next().map_err(|_| "Invalid gradient position")?;
        let position = Length::parse(parser.slice_from(start))?;
        if !matches!(position, Length::Px(_) | Length::Em(_) | Length::Percent(_)) {
            return Err("Invalid gradient stop unit".into());
        }
        stops.push(Stop { color, position: Some(position) });
    }
    if stops.is_empty() {
        stops.push(Stop { color, position: None });
    }
    Ok(stops)
}

impl Linear {
    pub fn resolved(&self, font_size: f32, foreground: [u8; 4]) -> Self {
        let mut result = self.clone();
        for stop in &mut result.stops {
            if stop.color == Color::Current {
                stop.color = Color::Rgba(foreground);
            }
            if let Some(Length::Em(value)) = stop.position {
                stop.position = Some(Length::Px(value * font_size));
            }
        }
        result
    }

    pub fn positions(&self, basis: f32) -> Vec<f32> {
        let basis = if basis.is_finite() && basis > 0.0 { basis } else { 1.0 };
        let mut points = self
            .stops
            .iter()
            .take(64)
            .map(|stop| match stop.position {
                Some(Length::Px(value)) => Some(value / basis),
                Some(Length::Percent(value)) => Some(value),
                _ => None,
            })
            .collect::<Vec<_>>();
        if let Some(first) = points.first_mut() {
            *first = Some(first.unwrap_or(0.0));
        }
        if let Some(last) = points.last_mut() {
            *last = Some(last.unwrap_or(1.0));
        }
        let mut maximum = f32::NEG_INFINITY;
        for point in points.iter_mut().flatten() {
            *point = point.max(maximum);
            maximum = *point;
        }
        let mut from = 0;
        while from + 1 < points.len() {
            let next = ((from + 1)..points.len())
                .find(|index| points.get(*index).is_some_and(Option::is_some))
                .unwrap_or(from + 1);
            let a = points.get(from).copied().flatten().unwrap_or(0.0);
            let b = points.get(next).copied().flatten().unwrap_or(1.0);
            for index in (from + 1)..next {
                if let Some(point) = points.get_mut(index) {
                    *point = Some(a + (b - a) * (index - from) as f32 / (next - from) as f32);
                }
            }
            from = next;
        }
        points.into_iter().map(|point| point.unwrap_or(0.0)).collect()
    }

    pub fn sample(&self, points: &[f32], position: f32) -> [u8; 4] {
        let index = points
            .iter()
            .take(64)
            .position(|point| *point > position)
            .unwrap_or(points.len().min(64))
            .saturating_sub(1);
        let Some(left) = self.stops.get(index) else {
            return [0; 4];
        };
        let rgba = |color| match color {
            Color::Rgba(value) => value,
            Color::Current => [0; 4],
        };
        let a = rgba(left.color);
        let Some(right) = self.stops.get(index + 1) else {
            return a;
        };
        let b = rgba(right.color);
        let start = points.get(index).copied().unwrap_or(0.0);
        let end = points.get(index + 1).copied().unwrap_or(1.0);
        let weight = if end > start { ((position - start) / (end - start)).clamp(0.0, 1.0) } else { 1.0 };
        let alpha = f32::from(a[3]) * (1.0 - weight) + f32::from(b[3]) * weight;
        if !alpha.is_finite() || alpha <= 0.0 {
            return [0; 4];
        }
        let mut result = [0; 4];
        for channel in 0..3 {
            result[channel] = ((f32::from(a[channel]) * f32::from(a[3]) * (1.0 - weight)
                + f32::from(b[channel]) * f32::from(b[3]) * weight)
                / alpha)
                .round()
                .clamp(0.0, 255.0) as u8;
        }
        result[3] = alpha.round().clamp(0.0, 255.0) as u8;
        result
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn parses_layers_hard_stops_and_premultiplied_transparency() {
        let layers = parse("linear-gradient(to right, red, transparent), repeating-linear-gradient(90deg, #fff 0 10px, #000 10px 20px)").unwrap();
        assert_eq!(layers.len(), 2);
        assert_eq!(layers[0].sample(&layers[0].positions(100.0), 0.5), [255, 0, 0, 128]);
        assert_eq!(layers[1].positions(100.0), vec![0.0, 0.1, 0.1, 0.2]);
        assert!(layers[1].repeating);
    }
    #[test]
    fn positions_are_distributed_and_invalid_sources_are_rejected() {
        let layer = parse("linear-gradient(red, green, blue)").unwrap().remove(0);
        assert_eq!(layer.positions(100.0), vec![0.0, 0.5, 1.0]);
        for source in [
            "linear-gradient(red)",
            "linear-gradient(red,)",
            "linear-gradient(red, blue),",
            "linear-gradient(1e99deg, red, blue)",
            "linear-gradient(red auto, blue)",
            "linear-gradient(red 0 10px 20px,blue)",
            "radial-gradient(red,blue)",
        ] {
            assert!(parse(source).is_err(), "{source}");
        }
    }
}
