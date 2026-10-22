// Copyright (C) 2026 The Suhina contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! Taffy geometry shared by drawing, hit testing and the interface editor.
use crate::{
    cascade::ComputedStyle,
    property::Property,
    selector::Tree,
    value::{Layout, Length, Value},
};
use cssparser::{Parser, ParserInput};
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
        let mut input = ParserInput::new(source);
        let mut parser = Parser::new(&mut input);
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
        Value::Length(Length::Px(n)) => length(n),
        Value::Length(Length::Em(n)) => length((n * font(style)).min(16384.0)),
        Value::Length(Length::Percent(n)) => percent(n),
        _ => length(0.0),
    }
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
pub fn compute(tree: &Tree, styles: &[ComputedStyle], viewport: [f32; 2]) -> Result<Vec<Option<Rect>>, String> {
    if styles.len() != tree.elements().len()
        || styles.is_empty()
        || viewport.iter().any(|n| !n.is_finite() || !(0.0..=16384.0).contains(n))
    {
        return Err("Invalid native layout inputs".into());
    }
    if tree.elements().iter().filter(|e| e.parent.is_none()).count() != 1 {
        return Err("Native layout needs one root".into());
    }
    let mut taffy: TaffyTree<()> = TaffyTree::new();
    taffy.disable_rounding();
    let mut nodes = Vec::with_capacity(styles.len());
    let mut children = vec![Vec::new(); styles.len()];
    let mut visible = Vec::with_capacity(styles.len());
    for (index, (element, computed)) in tree.elements().iter().zip(styles).enumerate() {
        let shown = computed.get(Property::Visible) != Value::Bool(false)
            && element.parent.is_none_or(|parent| visible.get(parent).copied().unwrap_or(false));
        visible.push(shown);
        let layout = computed.get(Property::Layout);
        let mut style = Style {
            display: if !shown {
                Display::None
            } else if matches!(layout, Value::Layout(Layout::Grid | Layout::Stack)) {
                Display::Grid
            } else {
                Display::Flex
            },
            flex_direction: if layout == Value::Layout(Layout::Row) {
                FlexDirection::Row
            } else {
                FlexDirection::Column
            },
            size: Size { width: size(computed, Property::Width), height: size(computed, Property::Height) },
            min_size: Size { width: length(0.0), height: length(0.0) },
            gap: Size { width: spacing(computed, Property::Gap), height: spacing(computed, Property::Gap) },
            padding: taffy::Rect {
                left: spacing(computed, Property::Padding),
                right: spacing(computed, Property::Padding),
                top: spacing(computed, Property::Padding),
                bottom: spacing(computed, Property::Padding),
            },
            grid_template_columns: grid_tracks(computed, Property::Columns),
            grid_template_rows: grid_tracks(computed, Property::Rows),
            ..Style::default()
        };
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
            if let (Value::Areas(areas), Value::Name(area)) =
                (parent_style.get(Property::Areas), computed.get(Property::Area))
                && !area.is_empty()
            {
                let [left, top, right, bottom] =
                    areas.bounds().get(&area).copied().ok_or_else(|| format!("Missing layout area {area}"))?;
                style.grid_column = Line { start: line((left + 1) as i16), end: line((right + 1) as i16) };
                style.grid_row = Line { start: line((top + 1) as i16), end: line((bottom + 1) as i16) };
            }
            if parent_style.get(Property::Layout) == Value::Layout(Layout::Stack) {
                style.grid_column = line(1);
                style.grid_row = line(1);
            }
            children.get_mut(parent).ok_or("Missing layout parent")?.push(index);
        } else {
            style.size = Size { width: length(viewport[0]), height: length(viewport[1]) };
        }
        nodes.push(taffy.new_leaf(style).map_err(|e| e.to_string())?);
    }
    for (index, children) in children.iter_mut().enumerate() {
        children.sort_by_key(|child| match styles.get(*child).map(|s| s.get(Property::Order)) {
            Some(Value::Integer(order)) => order,
            _ => 0,
        });
        let ordered = children.iter().filter_map(|index| nodes.get(*index).copied()).collect::<Vec<_>>();
        taffy.set_children(*nodes.get(index).ok_or("Missing layout node")?, &ordered).map_err(|e| e.to_string())?;
    }
    let root = *nodes.first().ok_or("Missing layout root")?;
    taffy
        .compute_layout(
            root,
            Size { width: AvailableSpace::Definite(viewport[0]), height: AvailableSpace::Definite(viewport[1]) },
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
mod tests {
    use super::*;
    use crate::{
        cascade::{self, AppliedSheet, Layer},
        media::Viewport,
        selector::Element,
        sheet::Sheet,
    };
    fn layout(source: &str, elements: Vec<Element>, width: f32) -> Vec<Option<Rect>> {
        let sheet = Sheet::parse(source).unwrap();
        assert!(sheet.notices.is_empty(), "{:?}", sheet.notices);
        let tree = Tree::new(elements).unwrap();
        let styles = cascade::compute(
            &tree,
            Viewport { width, height: 600.0, dark: true, reduced_motion: false },
            &[AppliedSheet { sheet: &sheet, layer: Layer::Theme, root: None }],
        )
        .unwrap();
        compute(&tree, &styles, [width, 600.0]).unwrap()
    }
    #[test]
    fn absent_panel_auto_track_collapses_and_resize_keeps_real_bounds() {
        let css = "window { layout: grid; columns: auto 1fr; rows: 1fr; areas: 'browser main'; } #browser { area: browser; width: 248px; } #main { area: main; }";
        let tree = vec![Element::new("window", None), Element::new("pane", Some(0)).with_id("main")];
        for width in [1138.0, 800.0, 1.0, 0.0, 1138.0] {
            let rects = layout(css, tree.clone(), width);
            assert_eq!(rects[1].unwrap().width, width);
            assert_eq!(rects[1].unwrap().x, 0.0);
        }
        let mut open = tree;
        open.push(Element::new("pane", Some(0)).with_id("browser"));
        let rects = layout(css, open, 800.0);
        assert_eq!(rects[1].unwrap().x, 248.0);
        assert_eq!(rects[1].unwrap().width, 552.0);
    }
    #[test]
    fn proportional_row_and_order_preserve_control_identity() {
        let elements = vec![
            Element::new("device-panel", None),
            Element::new("knob", Some(0)).with_id("cutoff"),
            Element::new("knob", Some(0)).with_id("resonance"),
        ];
        let rects = layout(
            "device-panel { layout: row; } #cutoff { width: 1fr; } #resonance { width: 4fr; order: -1; }",
            elements,
            500.0,
        );
        assert_eq!(rects[1].unwrap().width, 100.0);
        assert_eq!(rects[1].unwrap().x, 400.0);
        assert_eq!(rects[2].unwrap().width, 400.0);
        assert_eq!(rects[2].unwrap().x, 0.0);
    }
    #[test]
    fn disjoint_named_area_is_refused() {
        assert!(Areas::parse("'a b' 'b a'").is_err());
    }
}
