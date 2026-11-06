// Copyright (C) 2026 The Suhina contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! Typed native values. No renderer, pixels, or toolkit-specific colour types.
use cssparser::{Parser, Token};

#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Color {
    Rgba([u8; 4]),
    Current,
}

impl Color {
    pub fn parse(source: &str) -> Result<Self, String> {
        let mut parser = Parser::new(source);
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
                        Ok::<(), cssparser::ParseError<()>>(())
                    })
                    .map_err(|_| "Invalid rgb()")?;
                Self::Rgba(value.ok_or("Empty rgb()")??)
            }
            Token::Function(name) if name.eq_ignore_ascii_case("hsl") || name.eq_ignore_ascii_case("hsla") => {
                Self::Rgba(crate::sheet::nested(&mut parser, hsl)?)
            }
            Token::Function(name) if name.eq_ignore_ascii_case("color-mix") => {
                Self::Rgba(crate::sheet::nested(&mut parser, mix)?)
            }
            Token::Function(name) if name.eq_ignore_ascii_case("contrast-color") => {
                Self::Rgba(crate::sheet::nested(&mut parser, contrast)?)
            }
            _ => return Err("Unsupported colour value".into()),
        };
        parser.expect_exhausted().map_err(|_| "Unexpected colour suffix")?;
        Ok(color)
    }
}

fn hsl(parser: &mut Parser<'_>) -> Result<[u8; 4], String> {
    let hue = match parser.next().map_err(|_| "Missing hue")? {
        Token::Number { value, .. } => *value,
        Token::Dimension { value, unit, .. } => match unit.as_ref() {
            "deg" => *value,
            "rad" => value.to_degrees(),
            "turn" => value * 360.0,
            "grad" => value * 0.9,
            _ => return Err("Unknown hue unit".into()),
        },
        _ => return Err("Expected a hue".into()),
    };
    let comma = parser.try_parse(|p| p.expect_comma()).is_ok();
    let percent = |parser: &mut Parser<'_>| -> Result<f32, String> {
        match parser.next().map_err(|_| "Missing hsl component")? {
            Token::Percentage { unit_value, .. } => Ok(*unit_value),
            Token::Number { value, .. } => Ok(*value / 100.0),
            _ => Err("Expected a percentage".into()),
        }
    };
    let saturation = percent(parser)?.clamp(0.0, 1.0);
    if comma {
        parser.expect_comma().map_err(|_| "Expected comma")?;
    }
    let lightness = percent(parser)?.clamp(0.0, 1.0);
    let alpha = if parser.is_exhausted() {
        1.0
    } else {
        if comma {
            parser.expect_comma().map_err(|_| "Expected alpha comma")?;
        } else {
            parser.expect_delim('/').map_err(|_| "Expected alpha slash")?;
        }
        match parser.next().map_err(|_| "Missing alpha")? {
            Token::Number { value, .. } => *value,
            Token::Percentage { unit_value, .. } => *unit_value,
            _ => return Err("Expected alpha".into()),
        }
    };
    parser.expect_exhausted().map_err(|_| "Unexpected hsl component")?;
    if ![hue, alpha].iter().all(|v| v.is_finite()) {
        return Err("Non-finite colour".into());
    }
    let hue = hue.rem_euclid(360.0) / 30.0;
    let a = saturation * lightness.min(1.0 - lightness);
    let channel = |n: f32| {
        let k = (n + hue) % 12.0;
        let value = lightness - a * (k - 3.0).min(9.0 - k).clamp(-1.0, 1.0);
        (value * 255.0).round().clamp(0.0, 255.0) as u8
    };
    Ok([channel(0.0), channel(8.0), channel(4.0), (alpha.clamp(0.0, 1.0) * 255.0).round() as u8])
}

/// Colour spaces `color-mix()` interpolates in, premultiplied by alpha as CSS Color 5 says.
#[derive(Clone, Copy)]
enum Space {
    Srgb,
    SrgbLinear,
    Oklab,
    /// Polar spaces: their hue (degrees) is interpolated along the shorter arc.
    Hsl,
    Oklch,
}

fn rgb_to_hsl([r, g, b]: [f32; 3]) -> [f32; 3] {
    let max = r.max(g).max(b);
    let min = r.min(g).min(b);
    let lightness = (max + min) / 2.0;
    let delta = max - min;
    if delta <= f32::EPSILON {
        return [f32::NAN, 0.0, lightness];
    }
    let saturation = delta / (1.0 - (2.0 * lightness - 1.0).abs()).max(f32::EPSILON);
    let hue = if max == r {
        ((g - b) / delta).rem_euclid(6.0)
    } else if max == g {
        (b - r) / delta + 2.0
    } else {
        (r - g) / delta + 4.0
    } * 60.0;
    [hue, saturation, lightness]
}

fn hsl_to_rgb([hue, saturation, lightness]: [f32; 3]) -> [f32; 3] {
    let hue = if hue.is_finite() { hue.rem_euclid(360.0) / 30.0 } else { 0.0 };
    let a = saturation * lightness.min(1.0 - lightness);
    let channel = |n: f32| {
        let k = (n + hue) % 12.0;
        lightness - a * (k - 3.0).min(9.0 - k).clamp(-1.0, 1.0)
    };
    [channel(0.0), channel(8.0), channel(4.0)]
}

fn linear(channel: f32) -> f32 {
    if channel <= 0.04045 { channel / 12.92 } else { ((channel + 0.055) / 1.055).powf(2.4) }
}

fn gamma(channel: f32) -> f32 {
    if channel <= 0.003_130_8 { channel * 12.92 } else { 1.055 * channel.powf(1.0 / 2.4) - 0.055 }
}

impl Space {
    fn to(self, [r, g, b]: [f32; 3]) -> [f32; 3] {
        match self {
            Self::Srgb => [r, g, b],
            Self::SrgbLinear => [linear(r), linear(g), linear(b)],
            Self::Oklab => {
                let [r, g, b] = [linear(r), linear(g), linear(b)];
                let l = (0.412_221_47 * r + 0.536_332_55 * g + 0.051_445_99 * b).cbrt();
                let m = (0.211_903_5 * r + 0.680_699_5 * g + 0.107_396_96 * b).cbrt();
                let s = (0.088_302_46 * r + 0.281_718_85 * g + 0.629_978_7 * b).cbrt();
                [
                    0.210_454_26 * l + 0.793_617_8 * m - 0.004_072_047 * s,
                    1.977_998_5 * l - 2.428_592_2 * m + 0.450_593_7 * s,
                    0.025_904_037 * l + 0.782_771_77 * m - 0.808_675_77 * s,
                ]
            }
            Self::Hsl => rgb_to_hsl([r, g, b]),
            Self::Oklch => {
                let [l, a, b] = Self::Oklab.to([r, g, b]);
                let chroma = a.hypot(b);
                let hue = if chroma < 1e-4 { f32::NAN } else { b.atan2(a).to_degrees().rem_euclid(360.0) };
                [l, chroma, hue]
            }
        }
    }

    /// The index of the hue component in a polar space.
    fn hue(self) -> Option<usize> {
        match self {
            Self::Hsl => Some(0),
            Self::Oklch => Some(2),
            _ => None,
        }
    }

    fn from(self, [x, y, z]: [f32; 3]) -> [f32; 3] {
        match self {
            Self::Srgb => [x, y, z],
            Self::SrgbLinear => [gamma(x), gamma(y), gamma(z)],
            Self::Oklab => {
                let l = (x + 0.396_337_78 * y + 0.215_803_76 * z).powi(3);
                let m = (x - 0.105_561_346 * y - 0.063_854_17 * z).powi(3);
                let s = (x - 0.089_484_18 * y - 1.291_485_5 * z).powi(3);
                [
                    gamma(4.076_741_7 * l - 3.307_711_6 * m + 0.230_969_94 * s),
                    gamma(-1.268_438 * l + 2.609_757_4 * m - 0.341_319_38 * s),
                    gamma(-0.004_196_086_3 * l - 0.703_418_6 * m + 1.707_614_7 * s),
                ]
            }
            Self::Hsl => hsl_to_rgb([x, y, z]),
            Self::Oklch => {
                let hue = if z.is_finite() { z.to_radians() } else { 0.0 };
                Self::Oklab.from([x, y * hue.cos(), y * hue.sin()])
            }
        }
    }
}

/// `contrast-color(<colour>)`: white or black, whichever has the higher WCAG 2 contrast with the
/// colour; white on a tie. Alpha is ignored, as a colour behind the text is not known here.
fn contrast(parser: &mut Parser<'_>) -> Result<[u8; 4], String> {
    let start = parser.position();
    crate::sheet::consume(parser)?;
    let Color::Rgba([r, g, b, _]) = Color::parse(parser.slice_from(start).trim())? else {
        return Err("currentColor in contrast-color is not supported natively".into());
    };
    let channel = |c: u8| linear(f32::from(c) / 255.0);
    let luminance = 0.2126 * channel(r) + 0.7152 * channel(g) + 0.0722 * channel(b);
    let white = 1.05 / (luminance + 0.05);
    let black = (luminance + 0.05) / 0.05;
    Ok(if white >= black { [255, 255, 255, 255] } else { [0, 0, 0, 255] })
}

/// `color-mix(in <space>, <colour> [<p>%], <colour> [<p>%])` for literal colours.
fn mix(parser: &mut Parser<'_>) -> Result<[u8; 4], String> {
    parser.expect_ident_matching("in").map_err(|_| "color-mix needs an interpolation space")?;
    let space = match parser.expect_ident().map_err(|_| "Expected a colour space")?.to_ascii_lowercase().as_str() {
        "srgb" => Space::Srgb,
        "srgb-linear" => Space::SrgbLinear,
        "oklab" => Space::Oklab,
        "hsl" => Space::Hsl,
        "oklch" => Space::Oklch,
        _ => return Err("color-mix supports srgb, srgb-linear, oklab, hsl and oklch".into()),
    };
    let mut items = Vec::new();
    for _ in 0..2 {
        parser.expect_comma().map_err(|_| "Expected a comma in color-mix")?;
        let start = parser.position();
        let mut percent = None;
        let mut end = start;
        while !parser.is_exhausted() {
            let before = parser.state();
            match parser.next().map_err(|_| "Invalid color-mix item")?.clone() {
                Token::Comma => {
                    parser.reset(&before);
                    break;
                }
                Token::Percentage { unit_value, .. } if percent.is_none() => {
                    percent = Some(unit_value);
                    continue;
                }
                Token::Function(_) => crate::sheet::nested(parser, crate::sheet::consume)?,
                _ => {}
            }
            end = parser.position();
        }
        let text = parser.slice(start..end).trim();
        let Color::Rgba(color) = Color::parse(text)? else {
            return Err("currentColor in color-mix is not supported natively".into());
        };
        items.push((color, percent));
    }
    parser.expect_exhausted().map_err(|_| "color-mix takes two colours")?;
    let [(a, pa), (b, pb)] = <[_; 2]>::try_from(items).map_err(|_| "color-mix takes two colours")?;
    let (pa, pb) = match (pa, pb) {
        (None, None) => (0.5, 0.5),
        (Some(p), None) => (p, 1.0 - p),
        (None, Some(p)) => (1.0 - p, p),
        (Some(a), Some(b)) => (a, b),
    };
    if !(pa.is_finite() && pb.is_finite()) || pa < 0.0 || pb < 0.0 || pa + pb <= 0.0 {
        return Err("Invalid color-mix percentages".into());
    }
    let total = pa + pb;
    let scale = total.min(1.0);
    let weight = pb / total;
    let unit = |c: [u8; 4]| [f32::from(c[0]) / 255.0, f32::from(c[1]) / 255.0, f32::from(c[2]) / 255.0];
    let alpha = |c: [u8; 4]| f32::from(c[3]) / 255.0;
    let (ca, cb) = (space.to(unit(a)), space.to(unit(b)));
    let mixed_alpha = alpha(a) * (1.0 - weight) + alpha(b) * weight;
    let mut out = [0.0f32; 3];
    for i in 0..3 {
        if Some(i) == space.hue() {
            // Hue is not premultiplied; a missing (powerless) hue takes the other's; the
            // shorter arc is taken.
            let (h1, h2) = match (ca[i].is_finite(), cb[i].is_finite()) {
                (true, true) => (ca[i], cb[i]),
                (true, false) => (ca[i], ca[i]),
                (false, true) => (cb[i], cb[i]),
                (false, false) => (0.0, 0.0),
            };
            let mut delta = h2 - h1;
            if delta > 180.0 {
                delta -= 360.0;
            } else if delta < -180.0 {
                delta += 360.0;
            }
            out[i] = (h1 + delta * weight).rem_euclid(360.0);
            continue;
        }
        let premultiplied = ca[i] * alpha(a) * (1.0 - weight) + cb[i] * alpha(b) * weight;
        out[i] = if mixed_alpha > 0.0 { premultiplied / mixed_alpha } else { 0.0 };
    }
    let rgb = space.from(out);
    let byte = |v: f32| if v.is_finite() { (v.clamp(0.0, 1.0) * 255.0).round() as u8 } else { 0 };
    Ok([byte(rgb[0]), byte(rgb[1]), byte(rgb[2]), byte(mixed_alpha * scale)])
}

fn rgb(parser: &mut Parser<'_>) -> Result<[u8; 4], String> {
    let component = |parser: &mut Parser<'_>, alpha: bool| -> Result<u8, String> {
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

/// `source` with viewport units (`vw`, `vh`, `vmin`, `vmax`) written as points of a
/// `width` by `height` window, so lengths stay single-unit. Other text is kept as written.
pub fn viewport_units(source: &str, width: f32, height: f32) -> String {
    if !source.contains("vw") && !source.contains("vh") && !source.contains("vmin") && !source.contains("vmax") {
        return source.to_owned();
    }
    fn walk(parser: &mut Parser<'_>, out: &mut String, width: f32, height: f32, depth: usize) {
        while let Ok(token) = parser.next_including_whitespace_and_comments().cloned() {
            match token {
                Token::Dimension { value, ref unit, .. }
                    if matches!(unit.to_ascii_lowercase().as_str(), "vw" | "vh" | "vmin" | "vmax") =>
                {
                    let basis = match unit.to_ascii_lowercase().as_str() {
                        "vw" => width,
                        "vh" => height,
                        "vmin" => width.min(height),
                        _ => width.max(height),
                    };
                    out.push_str(&format!("{}px", value * basis / 100.0));
                }
                Token::Function(_) | Token::ParenthesisBlock | Token::SquareBracketBlock | Token::CurlyBracketBlock
                    if depth < 32 =>
                {
                    let (open, close) = match &token {
                        Token::Function(name) => (format!("{name}("), ')'),
                        Token::SquareBracketBlock => ("[".into(), ']'),
                        Token::CurlyBracketBlock => ("{".into(), '}'),
                        _ => ("(".into(), ')'),
                    };
                    out.push_str(&open);
                    let _ = parser.parse_nested_block(|inner| {
                        walk(inner, out, width, height, depth + 1);
                        Ok::<(), cssparser::ParseError<()>>(())
                    });
                    out.push(close);
                }
                other => out.push_str(&cssparser::ToCss::to_css_string(&other)),
            }
        }
    }
    let mut parser = Parser::new(source);
    let mut out = String::with_capacity(source.len());
    walk(&mut parser, &mut out, width, height, 0);
    out
}

/// A bounded `calc()` operand: an amount with one unit, or a plain number.
#[derive(Clone, Copy, PartialEq)]
enum Unit {
    Number,
    Px,
    Em,
    Percent,
}

fn math_function(parser: &mut Parser<'_>, name: &str, depth: usize, work: &mut usize) -> Result<(f32, Unit), String> {
    match name {
        "calc" => crate::sheet::nested(parser, |p| sum(p, depth + 1, work)),
        "min" | "max" | "clamp" => crate::sheet::nested(parser, |p| {
            let mut values = vec![sum(p, depth + 1, work)?];
            while !p.is_exhausted() {
                p.expect_comma().map_err(|_| "Expected a comma")?;
                values.push(sum(p, depth + 1, work)?);
                if values.len() > 16 {
                    return Err("Too many math arguments".into());
                }
            }
            let unit = values.first().map(|v| v.1).ok_or("Empty math function")?;
            if values.iter().any(|v| v.1 != unit) {
                return Err("Math arguments mix units".into());
            }
            let amounts: Vec<f32> = values.iter().map(|v| v.0).collect();
            let amount = match (name, amounts.as_slice()) {
                ("min", _) => amounts.iter().copied().fold(f32::INFINITY, f32::min),
                ("max", _) => amounts.iter().copied().fold(f32::NEG_INFINITY, f32::max),
                ("clamp", [low, value, high]) => value.min(*high).max(*low),
                _ => return Err("clamp() takes three values".into()),
            };
            Ok((amount, unit))
        }),
        _ => Err("Unknown math function".into()),
    }
}

fn operand(parser: &mut Parser<'_>, depth: usize, work: &mut usize) -> Result<(f32, Unit), String> {
    if depth > 16 {
        return Err("calc() nests too deeply".into());
    }
    *work = work.checked_sub(1).ok_or("calc() is too long")?;
    let token = parser.next().map_err(|_| "Expected a calc() value")?.clone();
    match token {
        Token::Number { value, .. } => Ok((value, Unit::Number)),
        Token::Percentage { unit_value, .. } => Ok((unit_value, Unit::Percent)),
        Token::Dimension { value, unit, .. } => match unit.as_ref() {
            "px" => Ok((value, Unit::Px)),
            "em" => Ok((value, Unit::Em)),
            _ => Err("Unsupported calc() unit".into()),
        },
        Token::ParenthesisBlock => crate::sheet::nested(parser, |p| sum(p, depth + 1, work)),
        Token::Function(name) => {
            let name = name.to_ascii_lowercase();
            math_function(parser, &name, depth, work)
        }
        _ => Err("Unexpected calc() token".into()),
    }
}

fn product(parser: &mut Parser<'_>, depth: usize, work: &mut usize) -> Result<(f32, Unit), String> {
    let mut left = operand(parser, depth, work)?;
    loop {
        let state = parser.state();
        match parser.next() {
            Ok(Token::Delim('*')) => {
                let right = operand(parser, depth, work)?;
                left = match (left.1, right.1) {
                    (Unit::Number, unit) => (left.0 * right.0, unit),
                    (unit, Unit::Number) => (left.0 * right.0, unit),
                    _ => return Err("calc() multiplies by a number only".into()),
                };
            }
            Ok(Token::Delim('/')) => {
                let right = operand(parser, depth, work)?;
                if right.1 != Unit::Number || right.0 == 0.0 {
                    return Err("calc() divides by a nonzero number only".into());
                }
                left.0 /= right.0;
            }
            _ => {
                parser.reset(&state);
                return Ok(left);
            }
        }
    }
}

fn sum(parser: &mut Parser<'_>, depth: usize, work: &mut usize) -> Result<(f32, Unit), String> {
    let mut left = product(parser, depth, work)?;
    loop {
        let state = parser.state();
        let sign = match parser.next() {
            Ok(Token::Delim('+')) => 1.0,
            Ok(Token::Delim('-')) => -1.0,
            _ => {
                parser.reset(&state);
                return Ok(left);
            }
        };
        let right = product(parser, depth, work)?;
        // Zero lengths mix with any unit, as `0` does in CSS.
        let unit = match (left.1, right.1) {
            (a, b) if a == b => a,
            (a, _) if right.0 == 0.0 => a,
            (_, b) if left.0 == 0.0 => b,
            _ => return Err("calc() mixes units that only resolve at layout".into()),
        };
        left = (left.0 + sign * right.0, unit);
    }
}

impl Length {
    pub fn parse(source: &str) -> Result<Self, String> {
        let mut parser = Parser::new(source);
        let lowered = source.trim_start().to_ascii_lowercase();
        if ["calc(", "min(", "max(", "clamp("].iter().any(|name| lowered.starts_with(name)) {
            let mut work = 256;
            let (amount, unit) = operand(&mut parser, 0, &mut work)?;
            parser.expect_exhausted().map_err(|_| "Unexpected length suffix")?;
            if !amount.is_finite() || amount.abs() > 16384.0 {
                return Err("Invalid or excessive length".into());
            }
            return match unit {
                Unit::Px => Ok(Self::Px(amount)),
                Unit::Em => Ok(Self::Em(amount)),
                Unit::Percent => Ok(Self::Percent(amount)),
                Unit::Number if amount == 0.0 => Ok(Self::Px(0.0)),
                Unit::Number => Err("calc() of a length needs a unit".into()),
            };
        }
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
    /// One keyword of a property's fixed list; empty when the property was never written.
    Keyword(&'static str),
    TrackList(Vec<crate::layout::Track>),
    Placement([crate::layout::GridLine; 2]),
    Transitions(Vec<crate::motion::Transition>),
    Animations(Vec<crate::motion::Animation>),
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

    #[test]
    fn viewport_units_become_points_of_the_window() {
        assert_eq!(viewport_units("clamp(10px, 1vw, 14px)", 1200.0, 800.0), "clamp(10px, 12px, 14px)");
        assert_eq!(Length::parse(&viewport_units("clamp(10px, 1vw, 14px)", 600.0, 800.0)), Ok(Length::Px(10.0)));
        assert_eq!(viewport_units("50vh", 100.0, 300.0), "150px");
        assert_eq!(viewport_units("10px red", 1.0, 1.0), "10px red");
    }

    #[test]
    fn math_functions_resolve_lengths_of_one_unit() {
        assert_eq!(Length::parse("calc(10px + 2px)"), Ok(Length::Px(12.0)));
        assert_eq!(Length::parse("calc((1em + 0.5em) * 2)"), Ok(Length::Em(3.0)));
        assert_eq!(Length::parse("calc(100% / 4)"), Ok(Length::Percent(0.25)));
        assert_eq!(Length::parse("max(4px, 2px * 3)"), Ok(Length::Px(6.0)));
        assert_eq!(Length::parse("clamp(2px, 10px, 8px)"), Ok(Length::Px(8.0)));
        assert_eq!(Length::parse("calc(0 + 3px)"), Ok(Length::Px(3.0)));
        for bad in [
            "calc(10px + 1em)",
            "calc(10px * 2px)",
            "calc(10px / 0)",
            "calc(1e30px * 1e30)",
            "calc(2)",
            "clamp(1px, 2px)",
            "calc(10deg)",
        ] {
            assert!(Length::parse(bad).is_err(), "{bad}");
        }
        let deep = format!("{}1px{}", "calc(".repeat(40), ")".repeat(40));
        assert!(Length::parse(&deep).is_err());
    }

    #[test]
    fn contrast_color_picks_white_or_black() {
        let white = Ok(Color::Rgba([255, 255, 255, 255]));
        let black = Ok(Color::Rgba([0, 0, 0, 255]));
        assert_eq!(Color::parse("contrast-color(#000)"), white);
        assert_eq!(Color::parse("contrast-color(navy)"), white);
        assert_eq!(Color::parse("contrast-color(#ffff00)"), black);
        assert_eq!(Color::parse("contrast-color(rgb(0 0 255 / 10%))"), white);
        // Black and white contrast equally at a luminance of 0.179, between greys 117 and 118.
        assert_eq!(Color::parse("contrast-color(#757575)"), white);
        assert_eq!(Color::parse("contrast-color(#767676)"), black);
        assert_eq!(Color::parse("contrast-color(#808080)"), black);
        assert_eq!(Color::parse("contrast-color(color-mix(in oklab, white, black 10%))"), black);
        assert_eq!(
            Color::parse("color-mix(in srgb, contrast-color(white), white)"),
            Ok(Color::Rgba([128, 128, 128, 255]))
        );
        for bad in
            ["contrast-color()", "contrast-color(currentColor)", "contrast-color(red, blue)", "contrast-color(1px)"]
        {
            assert!(Color::parse(bad).is_err(), "{bad}");
        }
    }

    #[test]
    fn hsl_and_color_mix_resolve_to_srgb() {
        assert_eq!(Color::parse("hsl(120 100% 50%)"), Ok(Color::Rgba([0, 255, 0, 255])));
        assert_eq!(Color::parse("hsla(0, 100%, 50%, .5)"), Ok(Color::Rgba([255, 0, 0, 128])));
        assert_eq!(Color::parse("color-mix(in srgb, red, blue)"), Ok(Color::Rgba([128, 0, 128, 255])));
        assert_eq!(Color::parse("color-mix(in srgb, red 25%, blue)"), Ok(Color::Rgba([64, 0, 191, 255])));
        // Percentages under 100% scale alpha.
        assert_eq!(Color::parse("color-mix(in srgb, red 20%, blue 30%)"), Ok(Color::Rgba([102, 0, 153, 128])));
        // Oklab keeps white white and mixes in a perceptual space.
        assert_eq!(Color::parse("color-mix(in oklab, white, white)"), Ok(Color::Rgba([255, 255, 255, 255])));
        let Ok(Color::Rgba(mixed)) = Color::parse("color-mix(in oklab, #163440, #7CF0C4 14%)") else {
            panic!("oklab mix")
        };
        assert!(mixed[1] > 0x34 && mixed[1] < 0xf0, "{mixed:?}");
        // Hue spaces take the shorter arc: red and blue meet at magenta, not green.
        let Ok(Color::Rgba(hsl)) = Color::parse("color-mix(in hsl, red, blue)") else { panic!("hsl") };
        assert_eq!(hsl, [255, 0, 255, 255]);
        let Ok(Color::Rgba(oklch)) = Color::parse("color-mix(in oklch, red, blue)") else { panic!("oklch") };
        assert!(oklch[0] > 100 && oklch[2] > 100 && oklch[1] < 100, "{oklch:?}");
        assert_eq!(Color::parse("color-mix(in hsl, white, white)"), Ok(Color::Rgba([255, 255, 255, 255])));
        for bad in [
            "color-mix(in lab, red, blue)",
            "color-mix(red, blue)",
            "color-mix(in srgb, currentColor, blue)",
            "color-mix(in srgb, red)",
            "color-mix(in srgb, red -5%, blue)",
            "hsl(nan 1% 1%)",
        ] {
            assert!(Color::parse(bad).is_err(), "{bad}");
        }
    }
}
