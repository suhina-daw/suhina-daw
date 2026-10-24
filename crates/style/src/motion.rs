// Copyright (C) 2026 The Suhina contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! CSS transitions and keyframe animations: which properties change smoothly, for how long and
//! with which easing, and the values of keyframes at a time. The host owns the clock.
use crate::property::Property;

/// Most transitions on one element.
pub const MAX_TRANSITIONS: usize = 8;
/// Longest duration or delay, in seconds.
pub const MAX_SECONDS: f32 = 10.0;

#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Easing {
    Linear,
    /// `cubic-bezier(x1, y1, x2, y2)`; the keywords are fixed curves of it.
    CubicBezier([f32; 4]),
    /// `steps(n, jump-end)` or `steps(n, jump-start)`.
    Steps(u16, bool),
}

impl Easing {
    fn parse(word: &str) -> Option<Self> {
        Some(match word {
            "linear" => Self::Linear,
            "ease" => Self::CubicBezier([0.25, 0.1, 0.25, 1.0]),
            "ease-in" => Self::CubicBezier([0.42, 0.0, 1.0, 1.0]),
            "ease-out" => Self::CubicBezier([0.0, 0.0, 0.58, 1.0]),
            "ease-in-out" => Self::CubicBezier([0.42, 0.0, 0.58, 1.0]),
            "step-start" => Self::Steps(1, true),
            "step-end" => Self::Steps(1, false),
            _ => {
                if let Some(inner) = word.strip_prefix("cubic-bezier(").and_then(|w| w.strip_suffix(')')) {
                    let values: Vec<f32> = inner.split(',').filter_map(|v| v.trim().parse().ok()).collect();
                    let [x1, y1, x2, y2] = <[f32; 4]>::try_from(values).ok()?;
                    if ![x1, y1, x2, y2].iter().all(|v| v.is_finite())
                        || !(0.0..=1.0).contains(&x1)
                        || !(0.0..=1.0).contains(&x2)
                        || y1.abs() > 16.0
                        || y2.abs() > 16.0
                    {
                        return None;
                    }
                    Self::CubicBezier([x1, y1, x2, y2])
                } else {
                    let inner = word.strip_prefix("steps(").and_then(|w| w.strip_suffix(')'))?;
                    let mut parts = inner.split(',').map(str::trim);
                    let count = parts.next()?.parse::<u16>().ok().filter(|n| (1..=1000).contains(n))?;
                    let start = match parts.next() {
                        None | Some("jump-end" | "end") => false,
                        Some("jump-start" | "start") => true,
                        _ => return None,
                    };
                    if parts.next().is_some() {
                        return None;
                    }
                    Self::Steps(count, start)
                }
            }
        })
    }

    /// The eased progress for linear progress `t` in `0..=1`.
    pub fn apply(self, t: f32) -> f32 {
        let t = if t.is_finite() { t.clamp(0.0, 1.0) } else { 1.0 };
        match self {
            Self::Linear => t,
            Self::Steps(count, start) => {
                let count = f32::from(count);
                let step = (t * count).floor() + if start { 1.0 } else { 0.0 };
                (step / count).clamp(0.0, 1.0)
            }
            Self::CubicBezier([x1, y1, x2, y2]) => {
                // Solve x(s) = t by bisection: bounded work, monotonic for x1, x2 in 0..=1.
                let curve = |s: f32, a: f32, b: f32| {
                    let r = 1.0 - s;
                    3.0 * r * r * s * a + 3.0 * r * s * s * b + s * s * s
                };
                let (mut low, mut high) = (0.0f32, 1.0f32);
                for _ in 0..24 {
                    let middle = (low + high) * 0.5;
                    if curve(middle, x1, x2) < t {
                        low = middle;
                    } else {
                        high = middle;
                    }
                }
                curve((low + high) * 0.5, y1, y2)
            }
        }
    }
}

/// One `transition` item. `property` is `None` for `all`.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Transition {
    pub property: Option<Property>,
    pub duration: f32,
    pub delay: f32,
    pub easing: Easing,
}

/// Properties the native renderer interpolates. Others change at once.
pub fn animatable(property: Property) -> bool {
    matches!(
        property,
        Property::Color
            | Property::BackgroundColor
            | Property::BorderColor
            | Property::Opacity
            | Property::Radius
            | Property::FontSize
    )
}

fn seconds(word: &str) -> Option<f32> {
    let value = if let Some(ms) = word.strip_suffix("ms") {
        ms.parse::<f32>().ok()? / 1000.0
    } else {
        word.strip_suffix('s')?.parse::<f32>().ok()?
    };
    (value.is_finite() && (0.0..=MAX_SECONDS).contains(&value)).then_some(value)
}

/// `transition: <property> <duration> [<easing>] [<delay>], ...` or `none`.
pub(crate) fn parse(source: &str) -> Result<Vec<Transition>, String> {
    crate::syntax::validate(source)?;
    let source = source.trim();
    if source == "none" {
        return Ok(Vec::new());
    }
    let mut result = Vec::new();
    let mut depth = 0usize;
    let mut items = vec![String::new()];
    for c in source.chars() {
        match c {
            '(' => depth += 1,
            ')' => depth = depth.saturating_sub(1),
            ',' if depth == 0 => {
                items.push(String::new());
                continue;
            }
            _ => {}
        }
        if let Some(item) = items.last_mut() {
            item.push(c);
        }
    }
    if items.len() > MAX_TRANSITIONS {
        return Err("At most eight transitions".into());
    }
    for item in items {
        let mut property = None;
        let mut named = false;
        let mut times = Vec::new();
        let mut easing = None;
        // Split at whitespace outside parentheses so cubic-bezier(...) stays whole.
        let mut words = Vec::new();
        let mut current = String::new();
        let mut depth = 0usize;
        for c in item.trim().chars() {
            match c {
                '(' => depth += 1,
                ')' => depth = depth.saturating_sub(1),
                _ => {}
            }
            if c.is_whitespace() && depth == 0 {
                if !current.is_empty() {
                    words.push(std::mem::take(&mut current));
                }
            } else {
                current.push(c);
            }
        }
        if !current.is_empty() {
            words.push(current);
        }
        if words.is_empty() {
            return Err("Empty transition".into());
        }
        for word in &words {
            let word = word.to_ascii_lowercase();
            if let Some(time) = seconds(&word) {
                if times.len() >= 2 {
                    return Err("A transition takes a duration and a delay".into());
                }
                times.push(time);
            } else if let Some(curve) = Easing::parse(&word) {
                if easing.replace(curve).is_some() {
                    return Err("A transition takes one easing".into());
                }
            } else if !named {
                named = true;
                property = match word.as_str() {
                    "all" => None,
                    "background" => Some(Property::BackgroundColor),
                    name => Some(Property::from_name(name).ok_or("Unknown transition property")?),
                };
            } else {
                return Err("Invalid transition".into());
            }
        }
        result.push(Transition {
            property,
            duration: times.first().copied().unwrap_or(0.0),
            delay: times.get(1).copied().unwrap_or(0.0),
            easing: easing.unwrap_or(Easing::CubicBezier([0.25, 0.1, 0.25, 1.0])),
        });
    }
    Ok(result)
}

/// The transition that applies to `property`, the last one listed for it, as in CSS.
pub fn find(transitions: &[Transition], property: Property) -> Option<Transition> {
    transitions
        .iter()
        .rev()
        .find(|t| t.property.is_none_or(|p| p == property) && t.duration > 0.0)
        .copied()
        .filter(|_| animatable(property))
}

/// Most animations on one element, keyframe rules in a sheet and frames in a rule.
pub const MAX_ANIMATIONS: usize = 4;
pub const MAX_KEYFRAMES: usize = 64;
pub const MAX_FRAMES: usize = 32;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Direction {
    Normal,
    Reverse,
    Alternate,
    AlternateReverse,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Fill {
    None,
    Forwards,
    Backwards,
    Both,
}

/// One `animation` item.
#[derive(Clone, Debug, PartialEq)]
pub struct Animation {
    pub name: String,
    pub duration: f32,
    pub delay: f32,
    pub easing: Easing,
    /// Iterations; infinite is `f32::INFINITY`.
    pub iterations: f32,
    pub direction: Direction,
    pub fill: Fill,
    pub paused: bool,
}

/// `animation: <name> <duration> [<easing>] [<delay>] [<count>|infinite] [<direction>]
/// [<fill-mode>] [running|paused], ...` or `none`.
pub(crate) fn parse_animations(source: &str) -> Result<Vec<Animation>, String> {
    crate::syntax::validate(source)?;
    let source = source.trim();
    if source == "none" {
        return Ok(Vec::new());
    }
    let mut result = Vec::new();
    for item in split_top(source, ',') {
        if result.len() >= MAX_ANIMATIONS {
            return Err("At most four animations".into());
        }
        let mut name = None;
        let mut times = Vec::new();
        let mut easing = None;
        let mut iterations = None;
        let mut direction = None;
        let mut fill = None;
        let mut paused = None;
        for word in split_top(&item, ' ').into_iter().filter(|w| !w.is_empty()) {
            let lower = word.to_ascii_lowercase();
            if let Some(time) = seconds(&lower) {
                if times.len() >= 2 {
                    return Err("An animation takes a duration and a delay".into());
                }
                times.push(time);
                continue;
            }
            if easing.is_none()
                && let Some(curve) = Easing::parse(&lower)
            {
                easing = Some(curve);
                continue;
            }
            let keyword_slot = match lower.as_str() {
                "infinite" if iterations.is_none() => {
                    iterations = Some(f32::INFINITY);
                    true
                }
                "normal" if direction.is_none() => {
                    direction = Some(Direction::Normal);
                    true
                }
                "reverse" if direction.is_none() => {
                    direction = Some(Direction::Reverse);
                    true
                }
                "alternate" if direction.is_none() => {
                    direction = Some(Direction::Alternate);
                    true
                }
                "alternate-reverse" if direction.is_none() => {
                    direction = Some(Direction::AlternateReverse);
                    true
                }
                "forwards" if fill.is_none() => {
                    fill = Some(Fill::Forwards);
                    true
                }
                "backwards" if fill.is_none() => {
                    fill = Some(Fill::Backwards);
                    true
                }
                "both" if fill.is_none() => {
                    fill = Some(Fill::Both);
                    true
                }
                "running" if paused.is_none() => {
                    paused = Some(false);
                    true
                }
                "paused" if paused.is_none() => {
                    paused = Some(true);
                    true
                }
                _ => false,
            };
            if keyword_slot {
                continue;
            }
            if iterations.is_none()
                && let Ok(count) = lower.parse::<f32>()
            {
                if !count.is_finite() || !(0.0..=1000.0).contains(&count) {
                    return Err("Invalid iteration count".into());
                }
                iterations = Some(count);
                continue;
            }
            if name.is_none() && valid_name(&word) {
                name = Some(word);
                continue;
            }
            return Err("Invalid animation".into());
        }
        result.push(Animation {
            name: name.ok_or("An animation needs a keyframes name")?,
            duration: times.first().copied().unwrap_or(0.0),
            delay: times.get(1).copied().unwrap_or(0.0),
            easing: easing.unwrap_or(Easing::CubicBezier([0.25, 0.1, 0.25, 1.0])),
            iterations: iterations.unwrap_or(1.0),
            direction: direction.unwrap_or(Direction::Normal),
            fill: fill.unwrap_or(Fill::None),
            paused: paused.unwrap_or(false),
        });
    }
    Ok(result)
}

/// A keyframes rule's name: an identifier that is not a CSS-wide keyword.
pub(crate) fn valid_name(name: &str) -> bool {
    !name.is_empty()
        && name.len() <= 64
        && !name.starts_with(|c: char| c.is_ascii_digit())
        && name.bytes().all(|b| b.is_ascii_alphanumeric() || b == b'-' || b == b'_')
        && !matches!(name, "none" | "initial" | "inherit" | "unset" | "default")
}

fn split_top(source: &str, separator: char) -> Vec<String> {
    let mut items = vec![String::new()];
    let mut depth = 0usize;
    for c in source.chars() {
        match c {
            '(' => depth += 1,
            ')' => depth = depth.saturating_sub(1),
            _ => {}
        }
        let split = if separator == ' ' { c.is_whitespace() } else { c == separator };
        if split && depth == 0 {
            items.push(String::new());
        } else if let Some(item) = items.last_mut() {
            item.push(c);
        }
    }
    items.into_iter().map(|item| item.trim().to_owned()).collect()
}

/// Keyframes resolved for one element: offsets in `0..=1`, sorted, with typed values.
pub type Frames = Vec<(f32, Vec<(Property, crate::value::Value)>)>;

/// Where an animation is at `elapsed` seconds since it started, as a fraction of one
/// iteration with the direction applied, or `None` when it does not apply.
pub fn progress(animation: &Animation, elapsed: f32) -> Option<f32> {
    if !elapsed.is_finite() || animation.duration <= 0.0 {
        return None;
    }
    let active = elapsed - animation.delay;
    let total = animation.duration * animation.iterations;
    let (iteration, fraction) = if active < 0.0 {
        if !matches!(animation.fill, Fill::Backwards | Fill::Both) {
            return None;
        }
        (0.0, 0.0)
    } else if active >= total {
        if !matches!(animation.fill, Fill::Forwards | Fill::Both) {
            return None;
        }
        let end = animation.iterations;
        // A whole number of iterations ends at the end of the last one.
        if end.fract() == 0.0 { (end - 1.0, 1.0) } else { (end.floor(), end.fract()) }
    } else {
        let at = active / animation.duration;
        (at.floor(), at.fract())
    };
    let forward = match animation.direction {
        Direction::Normal => true,
        Direction::Reverse => false,
        Direction::Alternate => iteration % 2.0 == 0.0,
        Direction::AlternateReverse => iteration % 2.0 != 0.0,
    };
    Some(if forward { fraction } else { 1.0 - fraction })
}

/// Whether the animation still changes after `elapsed` seconds.
pub fn running(animation: &Animation, elapsed: f32) -> bool {
    !animation.paused
        && animation.duration > 0.0
        && elapsed < animation.delay + animation.duration * animation.iterations
}

fn mix(a: &crate::value::Value, b: &crate::value::Value, t: f32) -> crate::value::Value {
    use crate::value::{Color, Length, Value};
    match (a, b) {
        (Value::Color(Color::Rgba(a)), Value::Color(Color::Rgba(b))) => {
            let alpha = f32::from(a[3]) * (1.0 - t) + f32::from(b[3]) * t;
            let mut result = [0u8; 4];
            for channel in 0..3 {
                let value = if alpha > 0.0 {
                    (f32::from(a[channel]) * f32::from(a[3]) * (1.0 - t) + f32::from(b[channel]) * f32::from(b[3]) * t)
                        / alpha
                } else {
                    0.0
                };
                result[channel] = value.round().clamp(0.0, 255.0) as u8;
            }
            result[3] = alpha.round().clamp(0.0, 255.0) as u8;
            Value::Color(Color::Rgba(result))
        }
        (Value::Number(a), Value::Number(b)) => Value::Number(a + (b - a) * t),
        (Value::Shadows(first), Value::Shadows(second))
            if first.len() == second.len() && first.iter().zip(second).all(|(a, b)| a.inset == b.inset) =>
        {
            let length = |a: Length, b: Length| match (a, b) {
                (Length::Px(a), Length::Px(b)) => Some(Length::Px(a + (b - a) * t)),
                (Length::Em(a), Length::Em(b)) => Some(Length::Em(a + (b - a) * t)),
                _ => None,
            };
            let mut shadows = Vec::with_capacity(first.len());
            for (a, b) in first.iter().zip(second) {
                let color = match mix(&Value::Color(a.color), &Value::Color(b.color), t) {
                    Value::Color(color) => color,
                    _ => b.color,
                };
                let lengths = (
                    length(a.offset[0], b.offset[0]),
                    length(a.offset[1], b.offset[1]),
                    length(a.blur, b.blur),
                    length(a.spread, b.spread),
                );
                let (Some(x), Some(y), Some(blur), Some(spread)) = lengths else {
                    // Lengths of different units switch halfway, as other discrete values do.
                    return if t < 0.5 { Value::Shadows(first.clone()) } else { Value::Shadows(second.clone()) };
                };
                shadows.push(crate::paint::Shadow { offset: [x, y], blur, spread, color, inset: a.inset });
            }
            Value::Shadows(shadows)
        }
        (Value::Length(Length::Px(a)), Value::Length(Length::Px(b))) => Value::Length(Length::Px(a + (b - a) * t)),
        (Value::Length(Length::Em(a)), Value::Length(Length::Em(b))) => Value::Length(Length::Em(a + (b - a) * t)),
        _ => {
            if t < 0.5 {
                a.clone()
            } else {
                b.clone()
            }
        }
    }
}

/// The values of `frames` at `progress`, eased per segment with `easing`. Properties missing
/// from the first or last frame take `base`, the element's own value, as CSS does.
pub fn sample(
    frames: &Frames,
    progress: f32,
    easing: Easing,
    base: impl Fn(Property) -> crate::value::Value,
) -> Vec<(Property, crate::value::Value)> {
    let mut properties: Vec<Property> = Vec::new();
    for (_, values) in frames {
        for (property, _) in values {
            if !properties.contains(property) {
                properties.push(*property);
            }
        }
    }
    properties
        .into_iter()
        .map(|property| {
            let mut points: Vec<(f32, crate::value::Value)> = frames
                .iter()
                .filter_map(|(offset, values)| {
                    values.iter().find(|(p, _)| *p == property).map(|(_, v)| (*offset, v.clone()))
                })
                .collect();
            if points.first().is_none_or(|(offset, _)| *offset > 0.0) {
                points.insert(0, (0.0, base(property)));
            }
            if points.last().is_none_or(|(offset, _)| *offset < 1.0) {
                points.push((1.0, base(property)));
            }
            let index = points.iter().rposition(|(offset, _)| *offset <= progress).unwrap_or(0);
            let value = match (points.get(index), points.get(index + 1)) {
                (Some((a_at, a)), Some((b_at, b))) if *b_at > *a_at => {
                    mix(a, b, easing.apply((progress - a_at) / (b_at - a_at)))
                }
                (Some((_, a)), _) => a.clone(),
                _ => base(property),
            };
            (property, value)
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn transitions_parse_durations_easings_and_delays() {
        let list = parse("background-color 140ms ease-out, border-color 0.2s cubic-bezier(.1, .7, 1, .1) 50ms, all 1s")
            .unwrap();
        assert_eq!(list.len(), 3);
        assert_eq!(list[0].property, Some(Property::BackgroundColor));
        assert!((list[0].duration - 0.14).abs() < 1e-6);
        assert_eq!(list[1].easing, Easing::CubicBezier([0.1, 0.7, 1.0, 0.1]));
        assert!((list[1].delay - 0.05).abs() < 1e-6);
        assert_eq!(find(&list, Property::Color).map(|t| t.duration), Some(1.0));
        assert_eq!(find(&list, Property::BoxShadow), None, "shadows switch at once");
        assert!(parse("none").unwrap().is_empty());
        for bad in ["color 99s", "color 1s 1s 1s", "nothing 1s", "color 1s ease ease", "cubic-bezier(2,0,0,1) 1s"] {
            assert!(parse(bad).is_err(), "{bad}");
        }
    }

    #[test]
    fn easing_curves_start_at_zero_and_end_at_one() {
        for easing in ["linear", "ease", "ease-in", "ease-out", "ease-in-out", "steps(4)", "steps(2, jump-start)"] {
            let curve = Easing::parse(easing).unwrap();
            assert!(curve.apply(1.0) > 0.999, "{easing}");
            assert!(curve.apply(f32::NAN) > 0.999);
        }
        assert!(Easing::parse("ease").unwrap().apply(0.0).abs() < 1e-4);
        assert!((Easing::Linear.apply(0.25) - 0.25).abs() < 1e-6);
        assert_eq!(Easing::Steps(4, false).apply(0.3), 0.25);
    }

    #[test]
    fn animations_parse_and_progress_through_iterations_directions_and_fills() {
        let list = parse_animations("ink 1s linear forwards, pulse 200ms ease-in 100ms infinite alternate").unwrap();
        assert_eq!(list[0].name, "ink");
        assert_eq!(list[0].fill, Fill::Forwards);
        assert_eq!(list[1].iterations, f32::INFINITY);
        assert_eq!(list[1].direction, Direction::Alternate);
        assert!((list[1].delay - 0.1).abs() < 1e-6);
        assert_eq!(progress(&list[0], 0.5), Some(0.5));
        assert_eq!(progress(&list[0], 2.0), Some(1.0), "forwards holds the end");
        assert!(running(&list[0], 0.9) && !running(&list[0], 1.1));
        assert_eq!(progress(&list[1], 0.05), None, "no backwards fill during the delay");
        let second = progress(&list[1], 0.1 + 0.2 + 0.05).unwrap();
        assert!((second - 0.75).abs() < 1e-4, "alternate runs backwards: {second}");
        assert!(running(&list[1], 1000.0));
        for bad in ["1s", "ink 1s 2s 3s", "none 1s", "ink 1s -1", "a b 1s"] {
            assert!(parse_animations(bad).is_err(), "{bad}");
        }
        assert!(parse_animations("none").unwrap().is_empty());
    }

    #[test]
    fn samples_interpolate_and_fall_back_to_the_base_value() {
        use crate::value::{Color, Value};
        let red = Value::Color(Color::Rgba([255, 0, 0, 255]));
        let blue = Value::Color(Color::Rgba([0, 0, 255, 255]));
        let frames: Frames = vec![(1.0, vec![(Property::Color, blue)])];
        let at = |p| sample(&frames, p, Easing::Linear, |_| red.clone());
        assert_eq!(at(0.0), vec![(Property::Color, red.clone())]);
        assert_eq!(at(0.5), vec![(Property::Color, Value::Color(Color::Rgba([128, 0, 128, 255])))]);
        assert_eq!(at(1.0)[0].1, Value::Color(Color::Rgba([0, 0, 255, 255])));
    }
}
