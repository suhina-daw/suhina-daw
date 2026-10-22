// Copyright (C) 2026 The Suhina contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! Resolve computed CSS gradient geometry against the native surface's real size.
use egui::{Vec2, vec2};
use style::values::computed::Length;
use style::values::computed::image::{EndingShape, LineDirection};
use style::values::computed::position::Position;
use style::values::generics::image::{Circle, Ellipse, ShapeExtent};
use style::values::specified::position::{HorizontalPositionKeyword as X, VerticalPositionKeyword as Y};

#[derive(Clone, Debug)]
pub(super) enum Geometry {
    NativeLinear(suhina_style::gradient::Direction),
    Linear(LineDirection),
    Radial(EndingShape, Position),
    Conic { angle: f32, position: Position },
}

pub(super) enum Mapping {
    Linear { axis: Vec2, basis: f32, center: Vec2 },
    Radial { center: Vec2, radii: Vec2 },
    Conic { center: Vec2, angle: f32 },
}

fn center(position: &Position, size: Vec2) -> Vec2 {
    vec2(position.horizontal.resolve(Length::new(size.x)).px(), position.vertical.resolve(Length::new(size.y)).px())
}

fn extent(extent: ShapeExtent, size: Vec2, center: Vec2, circle: bool) -> Vec2 {
    let sides = [center.x.abs(), (size.x - center.x).abs(), center.y.abs(), (size.y - center.y).abs()];
    let close = matches!(extent, ShapeExtent::ClosestSide | ShapeExtent::ClosestCorner | ShapeExtent::Contain);
    let combine = |a: f32, b: f32| if close { a.min(b) } else { a.max(b) };
    let mut radii = vec2(combine(sides[0], sides[1]), combine(sides[2], sides[3]));
    let corners = [vec2(0.0, 0.0), vec2(size.x, 0.0), size, vec2(0.0, size.y)].map(|p| p - center);
    let corner = corners
        .into_iter()
        .reduce(|a, b| if (a.length_sq() <= b.length_sq()) == close { a } else { b })
        .unwrap_or(Vec2::ZERO);
    let to_corner = matches!(extent, ShapeExtent::ClosestCorner | ShapeExtent::FarthestCorner | ShapeExtent::Cover);
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
            Self::NativeLinear(direction) => {
                let [x, y] = direction.axis([size.x, size.y]);
                let axis = vec2(x, y);
                Mapping::Linear { axis, basis: axis.x.abs() * size.x + axis.y.abs() * size.y, center: size * 0.5 }
            }
            Self::Linear(direction) => {
                let x = |value: X| if value == X::Left { -1.0 } else { 1.0 };
                let y = |value: Y| if value == Y::Top { -1.0 } else { 1.0 };
                let axis = match direction {
                    LineDirection::Angle(angle) => vec2(angle.radians().sin(), -angle.radians().cos()),
                    LineDirection::Horizontal(value) => vec2(x(*value), 0.0),
                    LineDirection::Vertical(value) => vec2(0.0, y(*value)),
                    LineDirection::Corner(horizontal, vertical) => {
                        vec2(x(*horizontal) * size.y, y(*vertical) * size.x).normalized()
                    }
                };
                Mapping::Linear { axis, basis: axis.x.abs() * size.x + axis.y.abs() * size.y, center: size * 0.5 }
            }
            Self::Radial(shape, position) => {
                let center = center(position, size);
                let radii = match shape {
                    EndingShape::Circle(Circle::Radius(radius)) => Vec2::splat(radius.0.px()),
                    EndingShape::Circle(Circle::Extent(value)) => extent(*value, size, center, true),
                    EndingShape::Ellipse(Ellipse::Radii(x, y)) => {
                        vec2(x.0.resolve(Length::new(size.x)).px(), y.0.resolve(Length::new(size.y)).px())
                    }
                    EndingShape::Ellipse(Ellipse::Extent(value)) => extent(*value, size, center, false),
                };
                Mapping::Radial { center, radii: radii.max(Vec2::splat(f32::EPSILON)) }
            }
            Self::Conic { angle, position } => Mapping::Conic { center: center(position, size), angle: *angle },
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
