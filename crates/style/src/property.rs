// Copyright (C) 2026 The Suhina contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! One registry for the implemented native properties, defaults and author guidance.
use crate::value::{Color, Layout, Length, Value};

#[derive(Clone, Copy, Debug)]
pub enum Kind {
    Color,
    Length,
    Number,
    Integer,
    Bool,
    Layout,
    Tracks,
    Areas,
    Name,
    BoxShadows,
    TextShadows,
    Gradients,
}

macro_rules! properties {
    ($( $variant:ident, $name:literal, $kind:ident, $default:expr, $inherits:literal, $description:literal; )*) => {
        #[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
        pub enum Property { $( $variant, )* }
        impl Property {
            pub const ALL: &'static [Self] = &[$(Self::$variant,)*];
            pub fn from_name(name: &str) -> Option<Self> { match name { $( $name => Some(Self::$variant), )* _ => None } }
            pub fn name(self) -> &'static str { match self { $( Self::$variant => $name, )* } }
            pub fn inherits(self) -> bool { match self { $( Self::$variant => $inherits, )* } }
            pub fn initial(self) -> Value { match self { $( Self::$variant => $default, )* } }
            pub fn kind(self) -> Kind { match self { $( Self::$variant => Kind::$kind, )* } }
            pub fn description(self) -> &'static str { match self { $( Self::$variant => $description, )* } }
        }
    };
}

properties! {
    BackgroundImage, "background-image", Gradients, Value::Gradients(Vec::new()), false, "Native linear and repeating linear gradient layers.";
    Color, "color", Color, Value::Color(Color::Rgba([255; 4])), true, "Foreground ink; inherited by child controls.";
    BackgroundColor, "background-color", Color, Value::Color(Color::Rgba([0; 4])), false, "Surface colour. Desktop opacity is a separate material property.";
    BorderColor, "border-color", Color, Value::Color(Color::Current), false, "Border ink, defaulting to the current foreground.";
    Radius, "radius", Length, Value::Length(Length::Px(0.0)), false, "Corner radius.";
    FontSize, "font-size", Length, Value::Length(Length::Px(11.0)), true, "Inherited text size.";
    Opacity, "opacity", Number, Value::Number(1.0), false, "Paint opacity from zero to one.";
    Width, "width", Length, Value::Length(Length::Auto), false, "Preferred element width.";
    Height, "height", Length, Value::Length(Length::Auto), false, "Preferred element height.";
    Gap, "gap", Length, Value::Length(Length::Px(0.0)), false, "Space between children.";
    Padding, "padding", Length, Value::Length(Length::Px(0.0)), false, "Space inside a container.";
    Layout, "layout", Layout, Value::Layout(Layout::Column), false, "Native row, column, grid or stack.";
    Order, "order", Integer, Value::Integer(0), false, "Visual order within the same parent; bindings do not change.";
    Visible, "visible", Bool, Value::Bool(true), false, "Whether the element participates in drawing and input.";
    Columns, "columns", Tracks, Value::Tracks(Vec::new()), false, "Grid column sizes: px, em, percent, fr or auto.";
    Rows, "rows", Tracks, Value::Tracks(Vec::new()), false, "Grid row sizes: px, em, percent, fr or auto.";
    Areas, "areas", Areas, Value::Areas(crate::layout::Areas::default()), false, "Quoted rows of rectangular named grid areas.";
    Area, "area", Name, Value::Name(String::new()), false, "The named grid area occupied by this element.";
    BoxShadow, "box-shadow", BoxShadows, Value::Shadows(Vec::new()), false, "Up to four inset/outset surface shadows.";
    TextShadow, "text-shadow", TextShadows, Value::Shadows(Vec::new()), true, "Up to four shadows behind native glyphs.";
}

impl Property {
    pub fn parse(self, source: &str) -> Result<Value, String> {
        let value = match self.kind() {
            Kind::Gradients => Value::Gradients(crate::gradient::parse(source)?),
            Kind::BoxShadows => Value::Shadows(crate::paint::shadows(source, false)?),
            Kind::TextShadows => Value::Shadows(crate::paint::shadows(source, true)?),
            Kind::Tracks => Value::Tracks(crate::layout::tracks(source)?),
            Kind::Areas => Value::Areas(crate::layout::Areas::parse(source)?),
            Kind::Name => {
                if !crate::layout::valid_area(source) {
                    return Err("Invalid area name".into());
                }
                Value::Name(source.into())
            }
            Kind::Color => Value::Color(Color::parse(source)?),
            Kind::Length => Value::Length(Length::parse(source)?),
            Kind::Number => {
                let number = source
                    .parse::<f32>()
                    .ok()
                    .filter(|v| v.is_finite() && (0.0..=1.0).contains(v))
                    .ok_or("Expected a number from zero to one")?;
                Value::Number(number)
            }
            Kind::Integer => Value::Integer(
                source.parse::<i32>().ok().filter(|v| (-16384..=16384).contains(v)).ok_or("Invalid native order")?,
            ),
            Kind::Bool => Value::Bool(match source {
                "true" => true,
                "false" => false,
                _ => return Err("Expected true or false".into()),
            }),
            Kind::Layout => Value::Layout(match source {
                "row" => Layout::Row,
                "column" => Layout::Column,
                "grid" => Layout::Grid,
                "stack" => Layout::Stack,
                _ => return Err("Unknown native layout".into()),
            }),
        };
        if let Value::Length(length) = value {
            let dimensions = matches!(self, Self::Width | Self::Height);
            if (!dimensions && matches!(length, Length::Auto | Length::Fr(_)))
                || matches!(length, Length::Px(n) | Length::Em(n) | Length::Percent(n) if n < 0.0)
            {
                return Err("Length is not allowed for this property".into());
            }
        }
        Ok(value)
    }

    /// Generated from the executable registry, not a separately maintained list.
    pub fn reference() -> String {
        let mut text = String::from("| Property | Inherited | Meaning |\n|---|---|---|\n");
        for property in Self::ALL {
            text.push_str(&format!(
                "| `{}` | {} | {} |\n",
                property.name(),
                property.inherits(),
                property.description()
            ));
        }
        text
    }
}
