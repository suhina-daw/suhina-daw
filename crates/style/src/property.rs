// Copyright (C) 2026 The Suhina contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! One registry for the implemented native properties, defaults and author guidance.
use crate::value::{Color, Layout, Length, Value};

const DISPLAY: &[&str] = &["flex", "grid", "block", "none", "inline-flex", "inline-grid", "inline-block"];
const DIRECTION: &[&str] = &["row", "column", "row-reverse", "column-reverse"];
const WRAP: &[&str] = &["nowrap", "wrap", "wrap-reverse"];
const ALIGN: &[&str] = &[
    "normal",
    "start",
    "end",
    "flex-start",
    "flex-end",
    "center",
    "stretch",
    "baseline",
    "space-between",
    "space-around",
    "space-evenly",
];
const VISIBILITY: &[&str] = &["visible", "hidden", "collapse"];
const TEXT_ALIGN: &[&str] = &["left", "center", "right", "start", "end"];
const OVERFLOW: &[&str] = &["visible", "hidden", "clip"];
const OUTLINE_STYLE: &[&str] = &["none", "solid"];
const APPEARANCE: &[&str] = &["native", "knob", "field"];
const VALUE_POSITION: &[&str] = &["right", "left", "top", "bottom", "inside", "center", "centre", "none", "hidden"];
const DECIMALS: &[&str] = &["0", "1", "2"];
const ON_OFF: &[&str] = &["on", "off"];
const SIDE: &[&str] = &["left", "right"];
const PARAM_LABEL: &[&str] = &["left", "top", "hidden"];
const METER_STYLE: &[&str] = &["continuous", "segmented", "needle", "frames"];
const DRAG_POINTER: &[&str] = &["auto", "hidden", "visible"];

/// A value's unit (`" dB"`): a quoted string of at most ten characters.
fn quoted_unit(text: &str) -> bool {
    let text = text.trim();
    text.len() >= 2
        && (text.starts_with('"') && text.ends_with('"') || text.starts_with('\'') && text.ends_with('\''))
        && text.chars().count() <= 10
}

/// Glass strengths: `auto`, or three integers from 0 to 100 (panels, tracks, clips).
fn liquid(text: &str) -> bool {
    let words: Vec<&str> = text.split_whitespace().collect();
    words == ["auto"] || words.len() == 3 && words.iter().all(|w| w.parse::<u8>().is_ok_and(|n| n <= 100))
}

/// A surface gradient: `none`, or an angle and colour stops (`90deg, #fff2 0%, #0002 100%`).
fn surface_gradient(text: &str) -> bool {
    let text = text.trim();
    text == "none" || (text.len() <= 1024 && text.contains(',') && !text.contains([';', '{', '}']))
}

/// An image of the theme's own folder: `none` or `url("<path>.png")` ([`backdrop_path`]).
fn backdrop_image(text: &str) -> bool {
    text.trim() == "none" || backdrop_path(text).is_some()
}

/// The path `url("<path>.png")` names, when it is a PNG inside the theme's folder: relative, at
/// most 240 bytes, with no empty, `.` or `..` part and no `\`, `:` or NUL. The one check of a
/// `backdrop-image`, for the engine and for the host that loads the picture.
pub fn backdrop_path(text: &str) -> Option<&str> {
    let path = text.trim().strip_prefix("url(")?.strip_suffix(')')?.trim().trim_matches(['"', '\'']);
    (path.len() <= 240
        && path.ends_with(".png")
        && !path.contains(['\\', ':', '\0'])
        && path.split('/').all(|part| !part.is_empty() && part != "." && part != ".."))
    .then_some(path)
}

#[derive(Clone, Copy, Debug)]
pub enum Kind {
    /// One keyword of a fixed list.
    Keyword(&'static [&'static str]),
    /// Text the host reads itself, checked by the function given.
    Text(fn(&str) -> bool),
    /// A nonnegative number, such as a flex factor.
    Factor,
    /// Grid template tracks: lengths, `fr`, `auto`, `minmax()` and `repeat()`.
    TrackList,
    /// A grid line placement: `<line> [/ <line>]`.
    Placement,
    /// `transition` items.
    Transitions,
    /// `animation` items naming `@keyframes`.
    Animations,
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
    ($( $variant:ident, $name:literal, $kind:ident $(($argument:expr))?, $default:expr, $inherits:literal, $description:literal; )*) => {
        #[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
        pub enum Property { $( $variant, )* }
        impl Property {
            pub const ALL: &'static [Self] = &[$(Self::$variant,)*];
            pub fn from_name(name: &str) -> Option<Self> { match name { $( $name => Some(Self::$variant), )* _ => None } }
            pub fn name(self) -> &'static str { match self { $( Self::$variant => $name, )* } }
            pub fn inherits(self) -> bool { match self { $( Self::$variant => $inherits, )* } }
            pub fn initial(self) -> Value { match self { $( Self::$variant => $default, )* } }
            pub fn kind(self) -> Kind { match self { $( Self::$variant => Kind::$kind $(($argument))?, )* } }
            pub fn description(self) -> &'static str { match self { $( Self::$variant => $description, )* } }
        }
    };
}

properties! {
    BackgroundImage, "background-image", Gradients, Value::Gradients(Vec::new()), false, "Native linear and repeating linear gradient layers.";
    Color, "color", Color, Value::Color(Color::Rgba([255; 4])), true, "Foreground ink; inherited by child controls.";
    BackgroundColor, "background-color", Color, Value::Color(Color::Rgba([0; 4])), false, "Surface colour. Desktop opacity is a separate material property.";
    AccentColor, "accent-color", Color, Value::Color(Color::Current), true, "Accent of controls: a knob's arc, a switched-on button; inherited.";
    BorderColor, "border-color", Color, Value::Color(Color::Current), false, "Border ink, defaulting to the current foreground.";
    Radius, "radius", Length, Value::Length(Length::Px(0.0)), false, "Corner radius.";
    BorderWidth, "border-width", Length, Value::Length(Length::Px(0.0)), false, "Width of a control's border; none is drawn at 0.";
    FontSize, "font-size", Length, Value::Length(Length::Px(11.0)), true, "Inherited text size.";
    Opacity, "opacity", Number, Value::Number(1.0), false, "Paint opacity from zero to one.";
    Width, "width", Length, Value::Length(Length::Auto), false, "Preferred element width.";
    Height, "height", Length, Value::Length(Length::Auto), false, "Preferred element height.";
    MinWidth, "min-width", Length, Value::Length(Length::Auto), false, "Smallest element width.";
    MinHeight, "min-height", Length, Value::Length(Length::Auto), false, "Smallest element height.";
    MaxWidth, "max-width", Length, Value::Length(Length::Auto), false, "Largest element width; none or auto for no limit.";
    MaxHeight, "max-height", Length, Value::Length(Length::Auto), false, "Largest element height; none or auto for no limit.";
    RowGap, "row-gap", Length, Value::Length(Length::Px(0.0)), false, "Space between rows of children; `gap` sets both gaps.";
    ColumnGap, "column-gap", Length, Value::Length(Length::Px(0.0)), false, "Space between columns of children.";
    PaddingTop, "padding-top", Length, Value::Length(Length::Px(0.0)), false, "Space inside a container; `padding` sets all four sides.";
    PaddingRight, "padding-right", Length, Value::Length(Length::Px(0.0)), false, "Space inside a container, right side.";
    PaddingBottom, "padding-bottom", Length, Value::Length(Length::Px(0.0)), false, "Space inside a container, bottom side.";
    PaddingLeft, "padding-left", Length, Value::Length(Length::Px(0.0)), false, "Space inside a container, left side.";
    MarginTop, "margin-top", Length, Value::Length(Length::Px(0.0)), false, "Space outside an element; `margin` sets all four sides; auto takes the free space.";
    MarginRight, "margin-right", Length, Value::Length(Length::Px(0.0)), false, "Space outside an element, right side.";
    MarginBottom, "margin-bottom", Length, Value::Length(Length::Px(0.0)), false, "Space outside an element, bottom side.";
    MarginLeft, "margin-left", Length, Value::Length(Length::Px(0.0)), false, "Space outside an element, left side.";
    Display, "display", Keyword(DISPLAY), Value::Keyword(""), false, "flex, grid, block or none; overrides `layout` when written.";
    FlexDirection, "flex-direction", Keyword(DIRECTION), Value::Keyword(""), false, "Main axis of a flex container.";
    FlexWrap, "flex-wrap", Keyword(WRAP), Value::Keyword("nowrap"), false, "Whether flex children wrap onto more lines.";
    FlexGrow, "flex-grow", Factor, Value::Number(0.0), false, "Share of free space a flex child takes; `flex` sets grow, shrink and basis.";
    FlexShrink, "flex-shrink", Factor, Value::Number(1.0), false, "How much a flex child shrinks when space is short.";
    FlexBasis, "flex-basis", Length, Value::Length(Length::Auto), false, "A flex child's size before free space is shared.";
    JustifyContent, "justify-content", Keyword(ALIGN), Value::Keyword(""), false, "Distribution of children along the main axis.";
    AlignItems, "align-items", Keyword(ALIGN), Value::Keyword(""), false, "Alignment of children across the main axis.";
    AlignSelf, "align-self", Keyword(ALIGN), Value::Keyword(""), false, "This child's own cross-axis alignment.";
    AlignContent, "align-content", Keyword(ALIGN), Value::Keyword(""), false, "Distribution of wrapped lines or grid tracks.";
    GridTemplateColumns, "grid-template-columns", TrackList, Value::TrackList(Vec::new()), false, "Grid columns: lengths, fr, auto, minmax() and repeat().";
    GridTemplateRows, "grid-template-rows", TrackList, Value::TrackList(Vec::new()), false, "Grid rows: lengths, fr, auto, minmax() and repeat().";
    GridColumn, "grid-column", Placement, Value::Placement([crate::layout::GridLine::Auto; 2]), false, "Grid column lines: a line, a negative line from the end, or span n, and an optional / end.";
    GridRow, "grid-row", Placement, Value::Placement([crate::layout::GridLine::Auto; 2]), false, "Grid row lines, as grid-column.";
    Transition, "transition", Transitions, Value::Transitions(Vec::new()), false, "Colours, opacity, radius and font size change smoothly: property, duration, easing, delay.";
    Animation, "animation", Animations, Value::Animations(Vec::new()), false, "Keyframe animations: name, duration, easing, delay, count, direction, fill.";
    TextAlign, "text-align", Keyword(TEXT_ALIGN), Value::Keyword("start"), true, "Where a control's value text sits: left, center or right.";
    Visibility, "visibility", Keyword(VISIBILITY), Value::Keyword("visible"), true, "hidden keeps the element's space but neither draws it nor lets it take input.";
    Layout, "layout", Layout, Value::Layout(Layout::Column), false, "Native row, column, grid or stack.";
    Order, "order", Integer, Value::Integer(0), false, "Visual order within the same parent; bindings do not change.";
    Visible, "visible", Bool, Value::Bool(true), false, "Whether the element participates in drawing and input.";
    Columns, "columns", Tracks, Value::Tracks(Vec::new()), false, "Grid column sizes: px, em, percent, fr or auto.";
    Rows, "rows", Tracks, Value::Tracks(Vec::new()), false, "Grid row sizes: px, em, percent, fr or auto.";
    Areas, "areas", Areas, Value::Areas(crate::layout::Areas::default()), false, "Quoted rows of rectangular named grid areas.";
    Area, "area", Name, Value::Name(String::new()), false, "The named grid area occupied by this element.";
    BoxShadow, "box-shadow", BoxShadows, Value::Shadows(Vec::new()), false, "Up to four inset/outset surface shadows.";
    TextShadow, "text-shadow", TextShadows, Value::Shadows(Vec::new()), true, "Up to four shadows behind native glyphs.";
    Overflow, "overflow", Keyword(OVERFLOW), Value::Keyword("visible"), false, "hidden (or clip) keeps what the element's children paint, their shadows too, inside its box; regions are hidden.";
    OutlineWidth, "outline-width", Length, Value::Length(Length::Px(0.0)), false, "Width of a line drawn around the element, outside its border; `outline` sets width, style and colour.";
    OutlineStyle, "outline-style", Keyword(OUTLINE_STYLE), Value::Keyword("none"), false, "solid draws the outline; none (the default) draws none.";
    OutlineColor, "outline-color", Color, Value::Color(Color::Current), false, "Ink of the outline, defaulting to the current foreground.";
    OutlineOffset, "outline-offset", Length, Value::Length(Length::Px(0.0)), false, "Space between the border and the outline; negative draws it inside.";
    Appearance, "appearance", Keyword(APPEARANCE), Value::Keyword(""), true, "A value control drawn as a knob or a field, over the user's choice; native keeps it.";
    ValuePosition, "value-position", Keyword(VALUE_POSITION), Value::Keyword(""), true, "Where a knob's value goes: right, left, top, bottom, inside or none.";
    ValueDecimals, "value-decimals", Keyword(DECIMALS), Value::Keyword(""), true, "Decimals a level's value shows: 0, 1 or 2.";
    ValueUnit, "value-unit", Text(quoted_unit), Value::Text(String::new()), true, "A unit after a level's value, quoted: \" dB\".";
    DragPointer, "drag-pointer", Keyword(DRAG_POINTER), Value::Keyword(""), true, "While a value control is dragged: hidden hides the pointer, visible shows the arrow of the drag's direction, auto follows the user's setting.";
    TrackArtwork, "track-artwork", Keyword(ON_OFF), Value::Keyword(""), true, "on draws a track's artwork behind its Launcher column and clips.";
    TrackHeaders, "track-headers", Keyword(SIDE), Value::Keyword(""), false, "The side of the Arrangement view its track headers are on.";
    TrackHeight, "track-height", Length, Value::Length(Length::Auto), false, "The height of an Arrangement track that has none of its own, 20 to 400 points.";
    HeaderMinWidth, "header-min-width", Length, Value::Length(Length::Auto), false, "The least width of the Arrangement view's header column, 160 to 520 points.";
    HeaderTint, "header-tint", Integer, Value::Integer(-1), false, "How strongly a track's colour tints its Arrangement header, 0 to 100.";
    CompactBelow, "compact-below", Length, Value::Length(Length::Auto), false, "A track lower than this keeps Suhina's own header layout, up to 200 points.";
    ParamLabel, "param-label", Keyword(PARAM_LABEL), Value::Keyword(""), true, "Where a device's generated parameters show their names: left, top or hidden.";
    MeterStyle, "meter-style", Keyword(METER_STYLE), Value::Keyword(""), false, "How level meters draw: continuous, segmented, needle or frames (artwork).";
    MeterSegments, "meter-segments", Integer, Value::Integer(0), false, "Segments of a segmented meter, 4 to 64.";
    MeterIntensity, "meter-intensity", Integer, Value::Integer(-1), false, "How strongly a meter's level shows, 0 to 100.";
    ArtworkFps, "artwork-fps", Integer, Value::Integer(0), false, "Frames per second of animated artwork, 1 to 30.";
    Liquid, "liquid", Text(liquid), Value::Text(String::new()), false, "Glass strengths: auto, or three integers from 0 to 100 for panels, tracks and clips.";
    PanelGradient, "panel-gradient", Text(surface_gradient), Value::Text(String::new()), false, "A gradient over the panels: none, or an angle and colour stops.";
    TrackGradient, "track-gradient", Text(surface_gradient), Value::Text(String::new()), false, "A gradient over the tracks: none, or an angle and colour stops.";
    ClipGradient, "clip-gradient", Text(surface_gradient), Value::Text(String::new()), false, "A gradient over the clips: none, or an angle and colour stops.";
    BackdropImage, "backdrop-image", Text(backdrop_image), Value::Text(String::new()), false, "The theme's picture behind the window: none or url(\"<file>.png\").";
    BackdropOpacity, "backdrop-opacity", Integer, Value::Integer(-1), false, "How much the backdrop picture shows, 0 to 100.";
    BackdropBlur, "backdrop-blur", Integer, Value::Integer(-1), false, "How much the backdrop picture is blurred, 0 to 100.";
    SessionTrackWidth, "track-width", Length, Value::Length(Length::Auto), false, "The width of a track's column in the Launcher view; auto is Suhina's.";
    SessionReturnWidth, "return-width", Length, Value::Length(Length::Auto), false, "The width of a return track's column in the Launcher view.";
    SessionSceneWidth, "scene-width", Length, Value::Length(Length::Auto), false, "The width of the Launcher view's scene column.";
    SessionSlotHeight, "slot-height", Length, Value::Length(Length::Auto), false, "The height of a clip slot in the Launcher view.";
    SessionHeaderHeight, "header-height", Length, Value::Length(Length::Auto), false, "The height of the Launcher view's track headers.";
}

impl Property {
    pub fn parse(self, source: &str) -> Result<Value, String> {
        // Corner radii: the top-left horizontal radius is the one native boxes draw, as the
        // compatibility adapter does; the remaining values are validated and kept in the source.
        if self == Self::Radius && source.trim().contains([' ', '/']) {
            let mut parts = source.split(['/', ' ']).filter(|part| !part.trim().is_empty());
            let first = parts.next().ok_or("Missing radius")?;
            let rest: Vec<&str> = parts.collect();
            if rest.len() > 7 {
                return Err("At most four radii on each axis".into());
            }
            for part in &rest {
                Length::parse(part)?;
            }
            return self.parse(first);
        }
        if matches!(self, Self::MaxWidth | Self::MaxHeight) && source.trim() == "none" {
            return Ok(Value::Length(Length::Auto));
        }
        let value = match self.kind() {
            Kind::Keyword(list) => {
                let word = source.trim().to_ascii_lowercase();
                Value::Keyword(list.iter().copied().find(|k| *k == word).ok_or("Unknown keyword")?)
            }
            Kind::Factor => Value::Number(
                source
                    .trim()
                    .parse::<f32>()
                    .ok()
                    .filter(|v| v.is_finite() && (0.0..=16384.0).contains(v))
                    .ok_or("Expected a nonnegative number")?,
            ),
            Kind::TrackList => Value::TrackList(crate::layout::track_list(source)?),
            Kind::Placement => Value::Placement(crate::layout::placement(source)?),
            Kind::Transitions => Value::Transitions(crate::motion::parse(source)?),
            Kind::Animations => Value::Animations(crate::motion::parse_animations(source)?),
            Kind::Gradients => Value::Gradients(crate::gradient::parse(source)?),
            Kind::BoxShadows => Value::Shadows(crate::paint::shadows(source, false)?),
            Kind::TextShadows => Value::Shadows(crate::paint::shadows(source, true)?),
            Kind::Tracks => Value::Tracks(crate::layout::tracks(source)?),
            Kind::Areas => Value::Areas(crate::layout::Areas::parse(source)?),
            Kind::Text(valid) => {
                if !valid(source) {
                    return Err("Invalid value".into());
                }
                Value::Text(source.trim().into())
            }
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
            let fr = matches!(self, Self::Width | Self::Height);
            let auto = fr
                || matches!(
                    self,
                    Self::TrackHeight
                        | Self::HeaderMinWidth
                        | Self::CompactBelow
                        | Self::SessionTrackWidth
                        | Self::SessionReturnWidth
                        | Self::SessionSceneWidth
                        | Self::SessionSlotHeight
                        | Self::SessionHeaderHeight
                        | Self::MinWidth
                        | Self::MinHeight
                        | Self::MaxWidth
                        | Self::MaxHeight
                        | Self::FlexBasis
                        | Self::MarginTop
                        | Self::MarginRight
                        | Self::MarginBottom
                        | Self::MarginLeft
                );
            let negative = matches!(
                self,
                Self::MarginTop | Self::MarginRight | Self::MarginBottom | Self::MarginLeft | Self::OutlineOffset
            );
            if (!auto && length == Length::Auto)
                || (!fr && matches!(length, Length::Fr(_)))
                || (!negative && matches!(length, Length::Px(n) | Length::Em(n) | Length::Percent(n) if n < 0.0))
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_backdrop_image_is_a_png_inside_the_theme_s_folder_and_its_one_check_is_the_engine_s() {
        for good in [r#"url("back.png")"#, "url('art/back.png')", "url(back.png)", "none"] {
            assert!(Property::BackdropImage.parse(good).is_ok(), "{good}");
        }
        assert_eq!(backdrop_path(r#" url( "art/back.png" ) "#), Some("art/back.png"));
        let long = format!("url({}.png)", "a".repeat(240));
        for bad in [
            r#"url("back.svg")"#,
            r#"url("/back.png")"#,
            r#"url("../back.png")"#,
            r#"url("./back.png")"#,
            r#"url("art//back.png")"#,
            r#"url("C:back.png")"#,
            r#"url("art\back.png")"#,
            "back.png",
            long.as_str(),
        ] {
            assert!(Property::BackdropImage.parse(bad).is_err(), "{bad}");
            assert_eq!(backdrop_path(bad), None, "{bad}");
        }
    }
}
