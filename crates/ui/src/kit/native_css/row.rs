// Copyright (C) 2026 The Suhina contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! A bar's items laid out in a CSS row (#1372): the toolbar, the status bar, the project tabs,
//! the application bar, the piano roll's toolbar and the MIDI tools' tab row all go through
//! [`row`]. The app gives the items in its order, with the sizes it measured; the sheet sets the
//! geometry (gaps, padding, alignment, sizes, `flex-grow`) and may hide an item with
//! `display: none`, which leaves it out of input as well as painting. Under the theme's sheets,
//! or the user-agent sheet alone when their layout fails, as the window's grid is.
use egui::{Rect, Vec2};
use suhina_style::property::Property;

/// Most items one bar lays out.
pub const MAX_ITEMS: usize = 256;
/// Most laid-out rows kept, for every bar together; past it the cache starts again.
const MAX_ROWS: usize = 64;

/// The bar an item row is laid out in: its element and the box the window gave it.
#[derive(Clone, Debug, PartialEq)]
pub struct Bar {
    pub name: &'static str,
    pub id: Option<&'static str>,
    pub attributes: Vec<(&'static str, String)>,
    /// The bar's whole box, its border and padding included.
    pub size: Vec2,
    /// The bar's node in this frame's style tree, when it is entered: its items are then laid out
    /// under the elements around it, as they paint.
    pub node: Option<usize>,
}

impl Bar {
    pub fn new(name: &'static str, size: Vec2) -> Self {
        Self { name, id: None, attributes: Vec::new(), size, node: None }
    }

    /// The bar as `element`, entered, has it: its attributes and the elements around it.
    pub fn of(element: &crate::kit::element::Entered, size: Vec2) -> Self {
        Self { node: element.node(), ..Self::new(element.name().name(), size) }
    }

    pub fn attribute(mut self, name: &'static str, value: impl Into<String>) -> Self {
        self.attributes.push((name, value.into()));
        self
    }
}

/// One item of a bar: an element, in the bar or in a group before it, with the size the app
/// measured for it (its content box; `None` on an axis is the sheet's to decide).
#[derive(Clone, Debug, PartialEq)]
pub struct Item {
    pub name: &'static str,
    pub id: Option<String>,
    pub attributes: Vec<(&'static str, String)>,
    /// The index of the group item it is in; `None` for the bar itself.
    pub parent: Option<usize>,
    pub size: [Option<f32>; 2],
}

impl Item {
    /// An item of element `name`.
    pub fn new(name: &'static str) -> Self {
        Self { name, id: None, attributes: Vec::new(), parent: None, size: [None, None] }
    }

    /// An item of element `name` with `id`, which is also its `kind` (the user-agent sheet
    /// selects by attribute, a theme by either).
    pub fn named(name: &'static str, id: &str) -> Self {
        Self::new(name).id(id).attribute("kind", id)
    }

    pub fn id(mut self, id: &str) -> Self {
        self.id = Some(id.to_owned());
        self
    }

    pub fn attribute(mut self, name: &'static str, value: impl Into<String>) -> Self {
        self.attributes.push((name, value.into()));
        self
    }

    /// The attribute `name` with no value, when `on`: `[selected]`.
    pub fn flag(self, name: &'static str, on: bool) -> Self {
        if on { self.attribute(name, "") } else { self }
    }

    pub fn width(mut self, width: f32) -> Self {
        self.size[0] = Some(width);
        self
    }

    pub fn height(mut self, height: f32) -> Self {
        self.size[1] = Some(height);
        self
    }

    /// In the group item at `group`.
    pub fn in_group(mut self, group: usize) -> Self {
        self.parent = Some(group);
        self
    }
}

/// Where a row's items went.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Row {
    /// Each item's rectangle in the bar's box (its top left at 0, 0), by the items' order; none
    /// for an item the sheet hides.
    pub items: Vec<Option<Rect>>,
    /// For each item, whether it has its measured width and lies inside its group's box, or the
    /// bar's content box (a hidden item does).
    pub inside: Vec<bool>,
    /// Whether every item is [`Row::inside`].
    pub fits: bool,
}

impl Row {
    /// Item `index`'s rectangle, moved to the bar's place on screen at `origin`.
    pub fn at(&self, index: usize, origin: egui::Pos2) -> Option<Rect> {
        self.items.get(index).copied().flatten().map(|rect| rect.translate(origin.to_vec2()))
    }

    /// Whether item `index` has its measured width and lies inside its group or the bar.
    pub fn inside(&self, index: usize) -> bool {
        self.inside.get(index).copied().unwrap_or(true)
    }
}

/// Lays `items` out in `bar`. `None` only when there is no room (an empty or non-finite box) or
/// the items are malformed (too many, a group after its item).
pub fn row(bar: &Bar, items: &[Item]) -> Option<Row> {
    if !bar.size.is_finite()
        || bar.size.x <= 0.0
        || bar.size.y <= 0.0
        || items.len() > MAX_ITEMS
        || items.iter().enumerate().any(|(index, item)| item.parent.is_some_and(|parent| parent >= index))
    {
        return None;
    }
    super::ENGINE.with(|engine| {
        let mut engine = engine.borrow_mut();
        // A sheet the engine holds for another theme (the theme just changed) is not used.
        let themed = engine.source.is_some() && super::same_source(engine.source.as_ref());
        // The bar and the elements around it: this frame's chain when the bar is entered, else
        // `html` and `body`.
        let mut all = bar.node.and_then(|node| super::native::chain(&engine, node)).unwrap_or_else(|| {
            let mut element = super::native::element(bar.name, bar.id, None, Some(1));
            element.attributes =
                bar.attributes.iter().map(|(name, value)| ((*name).to_owned(), value.clone())).collect();
            let mut all = super::native::roots(false);
            all.push(element);
            all
        });
        let at = all.len().checked_sub(1)?;
        let key = format!("{themed}:{:?}:{:?}:{items:?}", bar.size, all);
        if let Some(row) = engine.rows.get(&key) {
            return Some(row.clone());
        }
        let size = [bar.size.x.min(16384.0), bar.size.y.min(16384.0)];
        let mut sizes = vec![None; at];
        sizes.push(Some([Some(size[0]), Some(size[1])]));
        for item in items {
            let parent = item.parent.map_or(at, |group| group + at + 1);
            let mut element = super::native::element(item.name, item.id.as_deref(), None, Some(parent));
            element.attributes =
                item.attributes.iter().map(|(name, value)| ((*name).to_owned(), value.clone())).collect();
            all.push(element);
            sizes.push(Some(item.size));
        }
        let laid = super::native::tree_layout(themed.then_some(&*engine), all, at, &sizes, size)?;
        let (bar_rect, bar_style) = laid.first()?;
        let origin = bar_rect.map_or(Vec2::ZERO, |rect| rect.min.to_vec2());
        let [top, right, bottom, left] = bar_style.insets_in(size[0]);
        let content = Rect::from_min_max(egui::pos2(left, top), egui::pos2(size[0] - right, size[1] - bottom));
        let rects: Vec<Option<Rect>> = laid
            .iter()
            .skip(1)
            .map(|(rect, style)| rect.filter(|_| style.visible).map(|rect| rect.translate(-origin)))
            .collect();
        let styles: Vec<&super::Computed> = laid.iter().skip(1).map(|(_, style)| style).collect();
        let inside: Vec<bool> = items
            .iter()
            .zip(&rects)
            .zip(styles)
            .map(|((item, rect), style)| {
                let Some(rect) = rect else { return true };
                let parent = item.parent.map_or(Some(content), |group| rects.get(group).copied().flatten());
                let within = parent
                    .is_some_and(|parent| rect.left() + 0.5 >= parent.left() && rect.right() <= parent.right() + 0.5);
                // A width the sheet gives the item is its own, not a lack of room.
                // (`auto` and `max-width: none` give no size.)
                let sized = [Property::Width, Property::MaxWidth, Property::FlexBasis].iter().any(|property| {
                    matches!(
                        style.authored_value(*property),
                        Some(suhina_style::value::Value::Length(length)) if length != suhina_style::value::Length::Auto
                    )
                });
                // The app measured the item's content box: its padding and border are not part of it.
                let [_, right, _, left] = style.insets_in(rect.width());
                let content = rect.width() - left - right;
                within && (sized || !item.size[0].is_some_and(|width| content + 0.5 < width))
            })
            .collect();
        let fits = inside.iter().all(|inside| *inside);
        let row = Row { items: rects, inside, fits };
        if engine.rows.len() >= MAX_ROWS {
            engine.rows.clear();
        }
        engine.rows.insert(key, row.clone());
        Some(row)
    })
}

/// Lays out the first of `candidates` that fits `bar` ([`Row::fits`]): the app lists them from
/// the fullest to the sparsest, so a narrow bar gives way in the app's order while the sheet keeps
/// the geometry. The last when none fits, its overflow clipped. Returns its index and its row.
pub fn fitting(bar: &Bar, candidates: &[Vec<Item>]) -> Option<(usize, Row)> {
    fitting_with(bar, candidates, |_, _, row| row.fits)
}

/// [`fitting`], with `fits` saying whether candidate `index` (its items, its row) fits: for a bar
/// whose parts give way apart, such as the toolbar's start and end.
pub fn fitting_with(
    bar: &Bar,
    candidates: &[Vec<Item>],
    fits: impl Fn(usize, &[Item], &Row) -> bool,
) -> Option<(usize, Row)> {
    let mut last = None;
    for (index, items) in candidates.iter().enumerate() {
        let laid = row(bar, items)?;
        if fits(index, items, &laid) {
            return Some((index, laid));
        }
        last = Some((index, laid));
    }
    last
}

#[cfg(test)]
#[path = "row_tests.rs"]
mod tests;
