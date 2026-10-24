// Copyright (C) 2026 The Suhina contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! Bounded native linear, radial and conic gradients, with premultiplied sRGB interpolation.
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

/// How far a radial gradient's ending shape reaches, by the box's sides or corners.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Extent {
    ClosestSide,
    ClosestCorner,
    FarthestSide,
    FarthestCorner,
}

/// The size of a radial gradient's ending shape.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum RadialSize {
    Extent(Extent),
    /// A circle's radius, or an ellipse's horizontal and vertical radii.
    Radii(Length, Length),
}

/// The geometry of one gradient layer; a linear gradient's is its `direction`.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Shape {
    Linear,
    Radial {
        circle: bool,
        size: RadialSize,
        at: [Length; 2],
    },
    /// `from` in radians clockwise from the top; stop positions are fractions of a turn.
    Conic {
        from: f32,
        at: [Length; 2],
    },
}

/// One gradient layer. (Named for the first kind it supported; it holds every kind.)
#[derive(Clone, Debug, PartialEq)]
pub struct Linear {
    pub direction: Direction,
    pub repeating: bool,
    pub stops: Vec<Stop>,
    pub shape: Shape,
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
        let (kind, repeating) = match name.as_str() {
            "linear-gradient" => (Kind::Linear, false),
            "repeating-linear-gradient" => (Kind::Linear, true),
            "radial-gradient" => (Kind::Radial, false),
            "repeating-radial-gradient" => (Kind::Radial, true),
            "conic-gradient" => (Kind::Conic, false),
            "repeating-conic-gradient" => (Kind::Conic, true),
            _ => return Err("Unknown gradient kind".into()),
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
            let mut shape = match kind {
                Kind::Linear => Shape::Linear,
                Kind::Radial => Shape::Radial {
                    circle: false,
                    size: RadialSize::Extent(Extent::FarthestCorner),
                    at: [Length::Percent(0.5); 2],
                },
                Kind::Conic => Shape::Conic { from: 0.0, at: [Length::Percent(0.5); 2] },
            };
            let mut stops = Vec::new();
            for (index, part) in parts.iter().enumerate() {
                if index == 0 {
                    let prelude = match kind {
                        Kind::Linear => parse_direction(part).map(|value| direction = value),
                        Kind::Radial => parse_radial(part).map(|value| shape = value),
                        Kind::Conic => parse_conic(part).map(|value| shape = value),
                    };
                    if prelude.is_ok() {
                        continue;
                    }
                }
                stops.extend(parse_stop(part, kind == Kind::Conic)?);
                if stops.len() > 64 {
                    return Err("Gradient exceeds 64 stops".into());
                }
            }
            if stops.len() < 2 {
                return Err("Gradient needs at least two colour stops".into());
            }
            Ok(Linear { direction, repeating, stops, shape })
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

#[derive(Clone, Copy, PartialEq, Eq)]
enum Kind {
    Linear,
    Radial,
    Conic,
}

/// Removes a trailing `in srgb`, the one interpolation space native gradients use.
fn without_space(source: &str) -> Result<&str, String> {
    match source.find(" in ").map(|at| source.split_at(at)) {
        Some((before, space)) if space.trim() == "in srgb" => Ok(before.trim()),
        Some(_) => Err("Gradients interpolate in srgb natively".into()),
        None if source.trim() == "in srgb" => Ok(""),
        None => Ok(source.trim()),
    }
}

/// `at <position>`: one or two keywords, lengths or percentages.
fn parse_position(words: &[&str]) -> Result<[Length; 2], String> {
    let mut x = None;
    let mut y = None;
    let mut loose = Vec::new();
    if words.is_empty() || words.len() > 2 {
        return Err("A gradient position takes one or two values".into());
    }
    for word in words {
        match *word {
            "left" => x = Some(Length::Percent(0.0)),
            "right" => x = Some(Length::Percent(1.0)),
            "top" => y = Some(Length::Percent(0.0)),
            "bottom" => y = Some(Length::Percent(1.0)),
            "center" => loose.push(Length::Percent(0.5)),
            other => loose.push(Length::parse(other)?),
        }
    }
    if loose.iter().any(|l| matches!(l, Length::Auto | Length::Fr(_))) {
        return Err("Invalid gradient position".into());
    }
    let mut loose = loose.into_iter();
    let x = x.or_else(|| loose.next()).unwrap_or(Length::Percent(0.5));
    let y = y.or_else(|| loose.next()).unwrap_or(Length::Percent(0.5));
    if loose.next().is_some() {
        return Err("Invalid gradient position".into());
    }
    Ok([x, y])
}

fn parse_radial(source: &str) -> Result<Shape, String> {
    let source = without_space(source)?;
    let (shape, at) = match source.split_once("at") {
        Some((shape, at)) if shape.is_empty() || shape.ends_with(' ') => (shape.trim(), Some(at.trim())),
        _ => (source, None),
    };
    let at = match at {
        Some(at) => parse_position(&at.split_whitespace().collect::<Vec<_>>())?,
        None => [Length::Percent(0.5); 2],
    };
    let mut circle = None;
    let mut extent = None;
    let mut lengths = Vec::new();
    for word in shape.split_whitespace() {
        match word {
            "circle" if circle.is_none() => circle = Some(true),
            "ellipse" if circle.is_none() => circle = Some(false),
            "closest-side" => extent = Some(Extent::ClosestSide),
            "closest-corner" => extent = Some(Extent::ClosestCorner),
            "farthest-side" => extent = Some(Extent::FarthestSide),
            "farthest-corner" => extent = Some(Extent::FarthestCorner),
            other if lengths.len() < 2 => lengths.push(Length::parse(other)?),
            _ => return Err("Invalid radial gradient shape".into()),
        }
    }
    if shape.is_empty() && at == [Length::Percent(0.5); 2] && !source.contains("at") && !source.is_empty() {
        return Err("Not a radial prelude".into());
    }
    if extent.is_some() && !lengths.is_empty() {
        return Err("A radial gradient takes an extent or lengths, not both".into());
    }
    let circle = circle.unwrap_or(lengths.len() == 1);
    let size = match (lengths.as_slice(), circle) {
        ([], _) => RadialSize::Extent(extent.unwrap_or(Extent::FarthestCorner)),
        ([r], true) if matches!(r, Length::Px(_) | Length::Em(_)) => RadialSize::Radii(*r, *r),
        ([x, y], false) if [x, y].iter().all(|l| matches!(l, Length::Px(_) | Length::Em(_) | Length::Percent(_))) => {
            RadialSize::Radii(*x, *y)
        }
        _ => return Err("Invalid radial gradient size".into()),
    };
    Ok(Shape::Radial { circle, size, at })
}

fn parse_conic(source: &str) -> Result<Shape, String> {
    let source = without_space(source)?;
    let words: Vec<&str> = source.split_whitespace().collect();
    let mut from = 0.0;
    let mut at = [Length::Percent(0.5); 2];
    let mut rest = words.as_slice();
    if let ["from", angle, more @ ..] = rest {
        from = angle_radians(angle)?;
        rest = more;
    }
    if let ["at", position @ ..] = rest {
        at = parse_position(position)?;
        rest = &[];
    }
    if !rest.is_empty() || (words.is_empty() && !source.is_empty()) {
        return Err("Not a conic prelude".into());
    }
    if words.is_empty() {
        return Err("Not a conic prelude".into());
    }
    Ok(Shape::Conic { from: from.rem_euclid(std::f32::consts::TAU), at })
}

fn angle_radians(source: &str) -> Result<f32, String> {
    let mut input = ParserInput::new(source);
    let mut parser = Parser::new(&mut input);
    let radians = match parser.next().map_err(|_| "Expected an angle")? {
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
        _ => return Err("Expected an angle".into()),
    };
    parser.expect_exhausted().map_err(|_| "Unexpected angle suffix")?;
    Ok(radians)
}

fn parse_stop(source: &str, conic: bool) -> Result<Vec<Stop>, String> {
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
        let text = parser.slice_from(start);
        // A conic stop is an angle or a percentage of the turn.
        let position = if conic && !text.ends_with('%') {
            Length::Percent(angle_radians(text)? / std::f32::consts::TAU)
        } else {
            Length::parse(text)?
        };
        if !matches!(position, Length::Px(_) | Length::Em(_) | Length::Percent(_))
            || (conic && !matches!(position, Length::Percent(_)))
        {
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
        let em = |length: &mut Length| {
            if let Length::Em(value) = *length {
                *length = Length::Px(value * font_size);
            }
        };
        match &mut result.shape {
            Shape::Linear => {}
            Shape::Radial { size, at, .. } => {
                at.iter_mut().for_each(em);
                if let RadialSize::Radii(x, y) = size {
                    em(x);
                    em(y);
                }
            }
            Shape::Conic { at, .. } => at.iter_mut().for_each(em),
        }
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
    fn radial_and_conic_preludes_parse() {
        let layers = parse(
            "radial-gradient(ellipse at 65% 45%, #29424280, transparent 70%), radial-gradient(circle 10px at left top, red, blue), conic-gradient(from 90deg at 25% 75%, red, blue 0.25turn, green 50%), repeating-radial-gradient(red, blue 10px)",
        )
        .unwrap();
        assert_eq!(
            layers[0].shape,
            Shape::Radial {
                circle: false,
                size: RadialSize::Extent(Extent::FarthestCorner),
                at: [Length::Percent(0.65), Length::Percent(0.45)]
            }
        );
        assert_eq!(
            layers[1].shape,
            Shape::Radial {
                circle: true,
                size: RadialSize::Radii(Length::Px(10.0), Length::Px(10.0)),
                at: [Length::Percent(0.0), Length::Percent(0.0)]
            }
        );
        let Shape::Conic { from, at } = layers[2].shape else { panic!("conic") };
        assert!((from - std::f32::consts::FRAC_PI_2).abs() < 1e-6);
        assert_eq!(at, [Length::Percent(0.25), Length::Percent(0.75)]);
        assert_eq!(layers[2].positions(1.0), vec![0.0, 0.25, 0.5]);
        assert!(layers[3].repeating);
        // No prelude: the first part is a colour stop.
        assert_eq!(parse("radial-gradient(red, blue)").unwrap()[0].stops.len(), 2);
        assert_eq!(parse("conic-gradient(red, blue)").unwrap()[0].stops.len(), 2);
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
            "radial-gradient(circle 10% , red, blue)",
            "radial-gradient(in oklab, red, blue)",
            "conic-gradient(from 1px, red, blue)",
            "conic-gradient(red 10px, blue)",
            "radial-gradient(closest-side 10px, red, blue)",
        ] {
            assert!(parse(source).is_err(), "{source}");
        }
    }
}
