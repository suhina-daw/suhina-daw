// Copyright (C) 2026 The Suhina contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! Resolve computed CSS gradient geometry against the native surface's real size.
use egui::{Vec2, vec2};

#[derive(Clone, Debug)]
pub(super) enum Geometry {
    Native(suhina_style::gradient::Direction, suhina_style::gradient::Shape),
}

pub(super) enum Mapping {
    Linear { axis: Vec2, basis: f32, center: Vec2 },
    Radial { center: Vec2, radii: Vec2 },
    Conic { center: Vec2, angle: f32 },
}

/// A length of the box's `size` on one axis, in points.
fn resolve_length(length: suhina_style::value::Length, size: f32) -> f32 {
    use suhina_style::value::Length as L;
    match length {
        L::Px(value) | L::Em(value) => value,
        L::Percent(value) => value * size,
        L::Fr(_) | L::Auto => size * 0.5,
    }
}

fn native_extent(extent: suhina_style::gradient::Extent, size: Vec2, center: Vec2, circle: bool) -> Vec2 {
    use suhina_style::gradient::Extent as E;
    let sides = [center.x.abs(), (size.x - center.x).abs(), center.y.abs(), (size.y - center.y).abs()];
    let close = matches!(extent, E::ClosestSide | E::ClosestCorner);
    let combine = |a: f32, b: f32| if close { a.min(b) } else { a.max(b) };
    let mut radii = vec2(combine(sides[0], sides[1]), combine(sides[2], sides[3]));
    let corners = [vec2(0.0, 0.0), vec2(size.x, 0.0), size, vec2(0.0, size.y)].map(|p| p - center);
    let corner = corners
        .into_iter()
        .reduce(|a, b| if (a.length_sq() <= b.length_sq()) == close { a } else { b })
        .unwrap_or(Vec2::ZERO);
    let to_corner = matches!(extent, E::ClosestCorner | E::FarthestCorner);
    if circle {
        Vec2::splat(if to_corner { corner.length() } else { combine(radii.x, radii.y) })
    } else {
        if to_corner {
            let ratio = corner / radii.max(Vec2::splat(f32::EPSILON));
            radii *= ratio.length();
        }
        radii
    }
}

impl Geometry {
    pub fn resolve(&self, size: Vec2) -> Mapping {
        match self {
            Self::Native(direction, shape) => {
                use suhina_style::gradient::{RadialSize, Shape};
                let at = |at: &[suhina_style::value::Length; 2]| {
                    vec2(resolve_length(at[0], size.x), resolve_length(at[1], size.y))
                };
                match shape {
                    Shape::Linear => {
                        let [x, y] = direction.axis([size.x, size.y]);
                        let axis = vec2(x, y);
                        Mapping::Linear {
                            axis,
                            basis: axis.x.abs() * size.x + axis.y.abs() * size.y,
                            center: size * 0.5,
                        }
                    }
                    Shape::Radial { circle, size: radius, at: position } => {
                        let center = at(position);
                        let radii = match radius {
                            RadialSize::Extent(value) => native_extent(*value, size, center, *circle),
                            RadialSize::Radii(x, y) => vec2(resolve_length(*x, size.x), resolve_length(*y, size.y)),
                        };
                        Mapping::Radial { center, radii: radii.max(Vec2::splat(f32::EPSILON)) }
                    }
                    Shape::Conic { from, at: position } => Mapping::Conic { center: at(position), angle: *from },
                }
            }
        }
    }
}

impl Mapping {
    pub fn basis(&self) -> f32 {
        match self {
            Self::Linear { basis, .. } => *basis,
            Self::Radial { radii, .. } => radii.x,
            Self::Conic { .. } => 1.0,
        }
    }
    pub fn position(&self, point: Vec2) -> f32 {
        match self {
            Self::Linear { axis, basis, center } => (point - *center).dot(*axis) / basis + 0.5,
            Self::Radial { center, radii } => ((point - *center) / *radii).length(),
            Self::Conic { center, angle } => {
                let delta = point - *center;
                (delta.x.atan2(-delta.y) - angle).rem_euclid(std::f32::consts::TAU) / std::f32::consts::TAU
            }
        }
    }
}
