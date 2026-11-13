// Copyright (C) 2026 The Suhina contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! Taffy geometry shared by drawing, hit testing and the interface editor.
use crate::{
    cascade::ComputedStyle,
    property::Property,
    selector::Tree,
    value::{Layout, Length, Value},
};
use cssparser::Parser;
use std::collections::BTreeMap;
use taffy::prelude::*;

/// Logical-point bounds, keyed by the original element index, never visual order.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Rect {
    pub x: f32,
    pub y: f32,
    pub width: f32,
    pub height: f32,
}

#[derive(Clone, Debug, Default, PartialEq)]
pub struct Areas {
    cells: Vec<Vec<String>>,
}

impl Areas {
    pub fn parse(source: &str) -> Result<Self, String> {
        let mut parser = Parser::new(source);
        let mut cells: Vec<Vec<String>> = Vec::new();
        while !parser.is_exhausted() {
            let row = parser.expect_string_cloned().map_err(|_| "Area rows must be quoted")?;
            let row: Vec<String> = row.split_whitespace().map(str::to_string).collect();
            if row.is_empty()
                || row.len() > 16
                || cells.len() >= 16
                || cells.first().is_some_and(|first| first.len() != row.len())
            {
                return Err("Area template needs equal rows, at most 16 by 16".into());
            }
            if row.iter().any(|name| !valid_area(name)) {
                return Err("Invalid grid area name".into());
            }
            cells.push(row);
        }
        if cells.is_empty() {
            return Err("Empty area template".into());
        }
        let result = Self { cells };
        let bounds = result.bounds();
        for (name, [left, top, right, bottom]) in bounds {
            for y in top..bottom {
                for x in left..right {
                    if result.cells.get(y).and_then(|row| row.get(x)) != Some(&name) {
                        return Err("Named grid areas must be rectangles".into());
                    }
                }
            }
        }
        Ok(result)
    }

    fn bounds(&self) -> BTreeMap<String, [usize; 4]> {
        let mut areas = BTreeMap::new();
        for (y, row) in self.cells.iter().enumerate() {
            for (x, name) in row.iter().enumerate().filter(|(_, name)| name.as_str() != ".") {
                let bounds = areas.entry(name.clone()).or_insert([x, y, x + 1, y + 1]);
                bounds[0] = bounds[0].min(x);
                bounds[1] = bounds[1].min(y);
                bounds[2] = bounds[2].max(x + 1);
                bounds[3] = bounds[3].max(y + 1);
            }
        }
        areas
    }
}

pub(crate) fn valid_area(name: &str) -> bool {
    !name.is_empty()
        && name.len() <= 64
        && name.bytes().all(|b| b.is_ascii_alphanumeric() || matches!(b, b'-' | b'_' | b'.'))
}

pub(crate) fn tracks(source: &str) -> Result<Vec<Length>, String> {
    let result = source.split_whitespace().map(Length::parse).collect::<Result<Vec<_>, _>>()?;
    if result.is_empty()
        || result.len() > 16
        || result.iter().any(|v| matches!(v, Length::Px(n) | Length::Em(n) | Length::Percent(n) if *n < 0.0))
    {
        return Err("Grid tracks need one to sixteen nonnegative sizes".into());
    }
    Ok(result)
}

/// One grid track of `grid-template-columns` or `grid-template-rows`.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Track {
    Size(Length),
    MinMax(Length, Length),
}

/// Most tracks in one grid template, after `repeat()`.
pub const MAX_TRACKS: usize = 32;

fn track_size(source: &str) -> Result<Length, String> {
    let length = Length::parse(source.trim())?;
    if matches!(length, Length::Px(n) | Length::Em(n) | Length::Percent(n) if n < 0.0) {
        return Err("Grid tracks are nonnegative".into());
    }
    Ok(length)
}

/// Splits `source` at top-level whitespace, keeping functions whole.
fn words(source: &str) -> Result<Vec<String>, String> {
    let mut result = Vec::new();
    let mut depth = 0usize;
    let mut current = String::new();
    for c in source.chars() {
        match c {
            '(' => depth += 1,
            ')' => depth = depth.checked_sub(1).ok_or("Unbalanced parenthesis")?,
            _ => {}
        }
        if c.is_whitespace() && depth == 0 {
            if !current.is_empty() {
                result.push(std::mem::take(&mut current));
            }
        } else {
            current.push(c);
        }
        if result.len() > MAX_TRACKS * 2 || current.len() > 512 {
            return Err("Grid template is too long".into());
        }
    }
    if depth != 0 {
        return Err("Unbalanced parenthesis".into());
    }
    if !current.is_empty() {
        result.push(current);
    }
    Ok(result)
}

/// `grid-template-columns`/`-rows`: lengths, `fr`, `auto`, `minmax(a, b)` and
/// `repeat(n, ...)` with a whole count, at most [`MAX_TRACKS`] tracks.
pub(crate) fn track_list(source: &str) -> Result<Vec<Track>, String> {
    crate::syntax::validate(source)?;
    if source.trim() == "none" {
        return Ok(Vec::new());
    }
    let mut result = Vec::new();
    fn one(word: &str, result: &mut Vec<Track>, nested: bool) -> Result<(), String> {
        if let Some(inner) = word.strip_prefix("minmax(").and_then(|w| w.strip_suffix(')')) {
            let (min, max) = inner.split_once(',').ok_or("minmax() takes two sizes")?;
            let (min, max) = (track_size(min)?, track_size(max)?);
            if matches!(min, Length::Fr(_)) {
                return Err("minmax() cannot start at fr".into());
            }
            result.push(Track::MinMax(min, max));
        } else if let Some(inner) = word.strip_prefix("repeat(").and_then(|w| w.strip_suffix(')')) {
            if nested {
                return Err("repeat() cannot nest".into());
            }
            let (count, tracks) = inner.split_once(',').ok_or("repeat() takes a count and tracks")?;
            let count = count
                .trim()
                .parse::<usize>()
                .ok()
                .filter(|n| (1..=MAX_TRACKS).contains(n))
                .ok_or("Invalid repeat count")?;
            let mut inner = Vec::new();
            for word in words(tracks)? {
                one(&word, &mut inner, true)?;
            }
            for _ in 0..count {
                if result.len() + inner.len() > MAX_TRACKS {
                    return Err("Grid template exceeds 32 tracks".into());
                }
                result.extend(inner.iter().copied());
            }
        } else {
            result.push(Track::Size(track_size(word)?));
        }
        if result.len() > MAX_TRACKS {
            return Err("Grid template exceeds 32 tracks".into());
        }
        Ok(())
    }
    for word in words(source)? {
        one(&word, &mut result, false)?;
    }
    if result.is_empty() {
        return Err("Empty grid template".into());
    }
    Ok(result)
}

/// One end of a grid placement.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum GridLine {
    Auto,
    /// A line number, negative from the end; never zero.
    Line(i16),
    Span(u16),
}

fn grid_line(source: &str) -> Result<GridLine, String> {
    let source = source.trim();
    if source == "auto" {
        return Ok(GridLine::Auto);
    }
    if let Some(count) = source.strip_prefix("span") {
        return count
            .trim()
            .parse::<u16>()
            .ok()
            .filter(|n| (1..=64).contains(n))
            .map(GridLine::Span)
            .ok_or_else(|| "span takes a count from 1 to 64".into());
    }
    source
        .parse::<i16>()
        .ok()
        .filter(|n| *n != 0 && (-64..=64).contains(n))
        .map(GridLine::Line)
        .ok_or_else(|| "A grid line is auto, a nonzero number from -64 to 64, or span n".into())
}

/// `grid-column`/`grid-row`: `<line> [/ <line>]`.
pub(crate) fn placement(source: &str) -> Result<[GridLine; 2], String> {
    let mut parts = source.split('/');
    let start = grid_line(parts.next().ok_or("Missing grid line")?)?;
    let end = parts.next().map_or(Ok(GridLine::Auto), grid_line)?;
    if parts.next().is_some() {
        return Err("A grid placement takes at most two lines".into());
    }
    Ok([start, end])
}

fn font(style: &ComputedStyle) -> f32 {
    match style.get(Property::FontSize) {
        Value::Length(Length::Px(size)) => size.clamp(1.0, 256.0),
        _ => 11.0,
    }
}
fn size(style: &ComputedStyle, property: Property) -> Dimension {
    match style.get(property) {
        Value::Length(Length::Px(n)) => length(n),
        Value::Length(Length::Em(n)) => length((n * font(style)).min(16384.0)),
        Value::Length(Length::Percent(n)) => percent(n),
        _ => auto(),
    }
}
fn spacing(style: &ComputedStyle, property: Property) -> LengthPercentage {
    match style.get(property) {
        Value::Length(Length::Percent(n)) => percent(n),
        _ => length(spacing_points(style, property).unwrap_or(0.0)),
    }
}

/// [`spacing_points`], with a percentage taken of `basis`: the width of the box around the
/// element, as the layout takes it. For the CSS layout containers the sheet lets pad in percent
/// (`workspace`, `toolbar`) that is their own box's width, the size they are laid out in.
pub fn spacing_points_of(style: &ComputedStyle, property: Property, basis: f32) -> f32 {
    match style.get(property) {
        Value::Length(Length::Percent(n)) if basis.is_finite() => (n * basis.max(0.0)).clamp(0.0, MAX_SPACING),
        _ => spacing_points(style, property).unwrap_or(0.0),
    }
}

/// The most a padding or gap side resolves to, in points, for the layout and for painters alike.
pub const MAX_SPACING: f32 = 16384.0;

/// A padding (or gap) side in points as the layout resolves it, for painters that need the same
/// box without laying the element out: a length in points, or in `em` of the element's own font
/// size, 0 to [`MAX_SPACING`]. `None` for a percentage, which takes the size of the box around
/// the element.
pub fn spacing_points(style: &ComputedStyle, property: Property) -> Option<f32> {
    match style.get(property) {
        Value::Length(Length::Px(n)) if n.is_finite() => Some(n.clamp(0.0, MAX_SPACING)),
        Value::Length(Length::Em(n)) if n.is_finite() => Some((n * font(style)).clamp(0.0, MAX_SPACING)),
        Value::Length(Length::Percent(_)) => None,
        _ => Some(0.0),
    }
}
/// A border side's width, 0 to 16 px as it is painted.
fn border(style: &ComputedStyle, property: Property) -> LengthPercentage {
    match style.get(property) {
        Value::Length(Length::Px(n)) if n.is_finite() => length(n.clamp(0.0, 16.0)),
        _ => length(0.0),
    }
}
fn keyword(style: &ComputedStyle, property: Property) -> &'static str {
    match style.get(property) {
        Value::Keyword(word) => word,
        _ => "",
    }
}
fn bound(style: &ComputedStyle, property: Property) -> LengthPercentageAuto {
    match style.get(property) {
        Value::Length(Length::Px(n)) => length(n),
        Value::Length(Length::Em(n)) => length((n * font(style)).min(16384.0)),
        Value::Length(Length::Percent(n)) => percent(n),
        _ => auto(),
    }
}
fn min_size(style: &ComputedStyle, property: Property) -> LengthPercentageAuto {
    // Native elements may shrink to nothing unless a minimum is written.
    if style.get(property) == Value::Length(Length::Auto) { length(0.0) } else { bound(style, property) }
}
fn margin(style: &ComputedStyle, property: Property) -> LengthPercentageAuto {
    match style.get(property) {
        Value::Length(Length::Px(n)) => length(n),
        Value::Length(Length::Em(n)) => length((n * font(style)).clamp(-16384.0, 16384.0)),
        Value::Length(Length::Percent(n)) => percent(n),
        Value::Length(Length::Auto) => auto(),
        _ => length(0.0),
    }
}
fn content(word: &str) -> Option<AlignContent> {
    Some(match word {
        "start" => AlignContent::START,
        "end" => AlignContent::END,
        "flex-start" => AlignContent::FLEX_START,
        "flex-end" => AlignContent::FLEX_END,
        "center" => AlignContent::CENTER,
        "stretch" => AlignContent::STRETCH,
        "space-between" => AlignContent::SPACE_BETWEEN,
        "space-around" => AlignContent::SPACE_AROUND,
        "space-evenly" => AlignContent::SPACE_EVENLY,
        _ => return None,
    })
}
fn items(word: &str) -> Option<AlignItems> {
    Some(match word {
        "start" => AlignItems::START,
        "end" => AlignItems::END,
        "flex-start" => AlignItems::FLEX_START,
        "flex-end" => AlignItems::FLEX_END,
        "center" => AlignItems::CENTER,
        "stretch" => AlignItems::STRETCH,
        "baseline" => AlignItems::BASELINE,
        _ => return None,
    })
}
fn track(style: &ComputedStyle, value: Length) -> MinTrackSizingFunction {
    match value {
        Length::Px(n) => length(n),
        Length::Em(n) => length((n * font(style)).min(16384.0)),
        Length::Percent(n) => percent(n),
        Length::Fr(_) | Length::Auto => auto(),
    }
}
fn max_track(style: &ComputedStyle, value: Length) -> MaxTrackSizingFunction {
    match value {
        Length::Px(n) => length(n),
        Length::Em(n) => length((n * font(style)).min(16384.0)),
        Length::Percent(n) => percent(n),
        Length::Fr(n) => fr(n),
        Length::Auto => auto(),
    }
}
fn template(style: &ComputedStyle, property: Property) -> Option<Vec<GridTemplateComponent<String>>> {
    let Value::TrackList(tracks) = style.get(property) else { return None };
    if tracks.is_empty() {
        return None;
    }
    Some(
        tracks
            .into_iter()
            .map(|t| match t {
                Track::Size(Length::Fr(n)) => fr(n),
                Track::Size(value) => {
                    GridTemplateComponent::Single(minmax(track(style, value), max_track(style, value)))
                }
                Track::MinMax(min, max) => {
                    GridTemplateComponent::Single(minmax(track(style, min), max_track(style, max)))
                }
            })
            .collect(),
    )
}
fn grid_placement([start, end]: [GridLine; 2]) -> Line<GridPlacement<String>> {
    let one = |line: GridLine| match line {
        GridLine::Auto => GridPlacement::Auto,
        GridLine::Line(n) => taffy::style_helpers::line(n),
        GridLine::Span(n) => taffy::style_helpers::span(n),
    };
    Line { start: one(start), end: one(end) }
}
fn grid_tracks(style: &ComputedStyle, property: Property) -> Vec<GridTemplateComponent<String>> {
    let Value::Tracks(tracks) = style.get(property) else {
        return Vec::new();
    };
    tracks
        .into_iter()
        .map(|track| match track {
            Length::Px(n) => length(n),
            Length::Em(n) => length((n * font(style)).min(16384.0)),
            Length::Percent(n) => percent(n),
            Length::Fr(n) => fr(n),
            Length::Auto => auto(),
        })
        .collect()
}

/// A host supplies a bounded tree and its computed styles. Invisible elements keep
/// their identities but have no rectangle, so callers also suppress their hit targets.
/// Lays out `tree` in `viewport`. `sizes` gives an element's content size where the host
/// knows it (a bar's height from its text, a panel's saved width), by element index in `sizes`; a missing or
/// `None` entry is none. Like a replaced element's in CSS, the size is the content box's: an
/// `auto` size is it plus the element's padding and border, and a size the sheet sets wins. On an
/// element with children the size is its whole box instead, whatever the sheet says: a bar whose
/// place the window's grid gave lays its items out in that box (#1372).
pub fn compute(
    tree: &Tree,
    styles: &[ComputedStyle],
    viewport: [f32; 2],
    sizes: &[Option<[Option<f32>; 2]>],
) -> Result<Vec<Option<Rect>>, String> {
    compute_with(tree, styles, viewport, sizes, true)
}

/// [`compute`] with the children of every element in the order the tree gives them, whatever
/// their `order`: for a host whose items have an order of their own (a bar's, #1372).
pub fn compute_in_tree_order(
    tree: &Tree,
    styles: &[ComputedStyle],
    viewport: [f32; 2],
    sizes: &[Option<[Option<f32>; 2]>],
) -> Result<Vec<Option<Rect>>, String> {
    compute_with(tree, styles, viewport, sizes, false)
}

fn compute_with(
    tree: &Tree,
    styles: &[ComputedStyle],
    viewport: [f32; 2],
    sizes: &[Option<[Option<f32>; 2]>],
    by_order: bool,
) -> Result<Vec<Option<Rect>>, String> {
    if styles.len() != tree.elements().len()
        || styles.is_empty()
        || viewport.iter().any(|n| !n.is_finite() || !(0.0..=16384.0).contains(n))
    {
        return Err("Invalid native layout inputs".into());
    }
    if tree.elements().iter().filter(|e| e.parent.is_none()).count() != 1 {
        return Err("Native layout needs one root".into());
    }
    let mut taffy: TaffyTree<[Option<f32>; 2]> = TaffyTree::new();
    taffy.disable_rounding();
    let mut nodes = Vec::with_capacity(styles.len());
    let mut children = vec![Vec::new(); styles.len()];
    let mut visible = Vec::with_capacity(styles.len());
    let mut parents = vec![false; styles.len()];
    for parent in tree.elements().iter().filter_map(|element| element.parent) {
        if let Some(slot) = parents.get_mut(parent) {
            *slot = true;
        }
    }
    let clean = |n: f32| n.is_finite().then(|| n.clamp(0.0, 16384.0));
    for (index, (element, computed)) in tree.elements().iter().zip(styles).enumerate() {
        // A `grid-area` its parent's template does not name leaves the element out, so a theme's
        // typo or a template without the area hides it rather than adding an implicit track.
        let area = match computed.get(Property::GridArea) {
            Value::Name(area) if !area.is_empty() && area != "auto" => Some(area),
            _ => None,
        };
        let parent_areas =
            element.parent.and_then(|parent| match styles.get(parent)?.get(Property::GridTemplateAreas) {
                Value::Areas(areas) if areas != Areas::default() => Some(areas),
                _ => None,
            });
        // Under a parent without a template, the name means nothing and is ignored.
        let placed = area.as_ref().zip(parent_areas.as_ref()).map(|(area, areas)| areas.bounds().get(area).copied());
        let shown = computed.get(Property::Visible) != Value::Bool(false)
            && computed.get(Property::Display) != Value::Keyword("none")
            && !matches!(placed, Some(None))
            && element.parent.is_none_or(|parent| visible.get(parent).copied().unwrap_or(false));
        visible.push(shown);
        let layout = computed.get(Property::Layout);
        let display = keyword(computed, Property::Display);
        let mut style = Style {
            display: if !shown || display == "none" {
                Display::None
            } else {
                match display {
                    "flex" | "inline-flex" => Display::Flex,
                    "grid" | "inline-grid" => Display::Grid,
                    "block" | "inline-block" => Display::Block,
                    _ if matches!(layout, Value::Layout(Layout::Grid | Layout::Stack)) => Display::Grid,
                    _ => Display::Flex,
                }
            },
            flex_direction: match keyword(computed, Property::FlexDirection) {
                "row" => FlexDirection::Row,
                "column" => FlexDirection::Column,
                "row-reverse" => FlexDirection::RowReverse,
                "column-reverse" => FlexDirection::ColumnReverse,
                // Written `display: flex` lays out a row, as in CSS; the native `layout` a column.
                _ if matches!(display, "flex" | "inline-flex") => FlexDirection::Row,
                _ if layout == Value::Layout(Layout::Row) => FlexDirection::Row,
                _ => FlexDirection::Column,
            },
            flex_wrap: match keyword(computed, Property::FlexWrap) {
                "wrap" => FlexWrap::Wrap,
                "wrap-reverse" => FlexWrap::WrapReverse,
                _ => FlexWrap::NoWrap,
            },
            size: Size { width: size(computed, Property::Width), height: size(computed, Property::Height) },
            min_size: Size {
                width: min_size(computed, Property::MinWidth),
                height: min_size(computed, Property::MinHeight),
            },
            max_size: Size { width: bound(computed, Property::MaxWidth), height: bound(computed, Property::MaxHeight) },
            gap: Size { width: spacing(computed, Property::ColumnGap), height: spacing(computed, Property::RowGap) },
            padding: taffy::Rect {
                left: spacing(computed, Property::PaddingLeft),
                right: spacing(computed, Property::PaddingRight),
                top: spacing(computed, Property::PaddingTop),
                bottom: spacing(computed, Property::PaddingBottom),
            },
            // A box's border takes room inside it, as its padding does: children go in its content box.
            border: taffy::Rect {
                left: border(computed, Property::BorderLeftWidth),
                right: border(computed, Property::BorderRightWidth),
                top: border(computed, Property::BorderTopWidth),
                bottom: border(computed, Property::BorderBottomWidth),
            },
            margin: taffy::Rect {
                left: margin(computed, Property::MarginLeft),
                right: margin(computed, Property::MarginRight),
                top: margin(computed, Property::MarginTop),
                bottom: margin(computed, Property::MarginBottom),
            },
            justify_content: content(keyword(computed, Property::JustifyContent)).unwrap_or(AlignContent::NORMAL),
            align_content: content(keyword(computed, Property::AlignContent)).unwrap_or(AlignContent::NORMAL),
            align_items: items(keyword(computed, Property::AlignItems)).unwrap_or(AlignItems::NORMAL),
            align_self: items(keyword(computed, Property::AlignSelf)),
            grid_template_columns: template(computed, Property::GridTemplateColumns)
                .unwrap_or_else(|| grid_tracks(computed, Property::Columns)),
            grid_template_rows: template(computed, Property::GridTemplateRows)
                .unwrap_or_else(|| grid_tracks(computed, Property::Rows)),
            ..Style::default()
        };
        // Their initial values are CSS's, so a sheet at any layer, the user-agent sheet's
        // included, sets them as a browser would (#1372).
        if let Value::Number(grow) = computed.get(Property::FlexGrow) {
            style.flex_grow = grow.clamp(0.0, 1.0e6);
        }
        if let Value::Number(shrink) = computed.get(Property::FlexShrink) {
            style.flex_shrink = shrink.clamp(0.0, 1.0e6);
        }
        style.flex_basis = size(computed, Property::FlexBasis);
        if let Some(parent) = element.parent {
            let parent_style = styles.get(parent).ok_or("Missing parent style")?;
            let axis = if parent_style.get(Property::Layout) == Value::Layout(Layout::Row) {
                Property::Width
            } else {
                Property::Height
            };
            if let Value::Length(Length::Fr(grow)) = computed.get(axis) {
                style.flex_grow = grow;
                style.flex_basis = length(0.0);
            }
            if let Some(Some([left, top, right, bottom])) = placed {
                style.grid_column = Line { start: line((left + 1) as i16), end: line((right + 1) as i16) };
                style.grid_row = Line { start: line((top + 1) as i16), end: line((bottom + 1) as i16) };
            }
            if parent_style.get(Property::Layout) == Value::Layout(Layout::Stack) {
                style.grid_column = line(1);
                style.grid_row = line(1);
            }
            if let Value::Placement(lines) = computed.get(Property::GridColumn)
                && lines != [GridLine::Auto; 2]
            {
                style.grid_column = grid_placement(lines);
            }
            if let Value::Placement(lines) = computed.get(Property::GridRow)
                && lines != [GridLine::Auto; 2]
            {
                style.grid_row = grid_placement(lines);
            }
            children.get_mut(parent).ok_or("Missing layout parent")?.push(index);
        } else {
            style.size = Size { width: length(viewport[0]), height: length(viewport[1]) };
        }
        let size = sizes.get(index).copied().flatten().unwrap_or([None, None]);
        if parents.get(index).copied().unwrap_or(false) {
            if let Some(width) = size[0].and_then(clean) {
                style.size.width = length(width);
                style.min_size.width = length(width);
                style.max_size.width = length(width);
            }
            if let Some(height) = size[1].and_then(clean) {
                style.size.height = length(height);
                style.min_size.height = length(height);
                style.max_size.height = length(height);
            }
        }
        nodes.push(taffy.new_leaf_with_context(style, size).map_err(|e| e.to_string())?);
    }
    for (index, children) in children.iter_mut().enumerate() {
        if by_order {
            children.sort_by_key(|child| match styles.get(*child).map(|s| s.get(Property::Order)) {
                Some(Value::Integer(order)) => order,
                _ => 0,
            });
        }
        let ordered = children.iter().filter_map(|index| nodes.get(*index).copied()).collect::<Vec<_>>();
        taffy.set_children(*nodes.get(index).ok_or("Missing layout node")?, &ordered).map_err(|e| e.to_string())?;
    }
    let root = *nodes.first().ok_or("Missing layout root")?;
    taffy
        .compute_layout_with_measure(
            root,
            Size { width: AvailableSpace::Definite(viewport[0]), height: AvailableSpace::Definite(viewport[1]) },
            // Taffy measures only leaves, and only for an axis the style leaves to the content.
            |inputs, _, size, style| {
                let [width, height] = size.map_or([None, None], |size| *size);
                let clean = |n: Option<f32>| n.filter(|n| n.is_finite()).map_or(0.0, |n| n.clamp(0.0, 16384.0));
                taffy::compute_leaf_layout(
                    inputs,
                    style,
                    |_, _| 0.0,
                    |known, _| Size {
                        width: known.width.unwrap_or(clean(width)),
                        height: known.height.unwrap_or(clean(height)),
                    },
                )
            },
        )
        .map_err(|e| e.to_string())?;
    let mut rectangles: Vec<Option<Rect>> = Vec::with_capacity(nodes.len());
    for (index, node) in nodes.iter().enumerate() {
        if !visible.get(index).copied().unwrap_or(false) {
            rectangles.push(None);
            continue;
        }
        let layout = taffy.layout(*node).map_err(|e| e.to_string())?;
        let parent = tree
            .elements()
            .get(index)
            .and_then(|e| e.parent)
            .and_then(|index| rectangles.get(index))
            .copied()
            .flatten()
            .unwrap_or_default();
        let rect = Rect {
            x: parent.x + layout.location.x,
            y: parent.y + layout.location.y,
            width: layout.size.width,
            height: layout.size.height,
        };
        if [rect.x, rect.y, rect.width, rect.height].iter().any(|n| !n.is_finite() || n.abs() > 16384.0) {
            return Err("Layout result exceeds native coordinate limits".into());
        }
        rectangles.push(Some(rect));
    }
    Ok(rectangles)
}

#[cfg(test)]
#[path = "layout_tests.rs"]
mod tests;
