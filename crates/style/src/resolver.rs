// Copyright (C) 2026 The Suhina contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! Retained native style/layout snapshots for immediate-mode hosts.
use crate::{
    cascade::{self, AppliedSheet, ComputedStyle, Layer},
    layout::{self, Rect},
    media::Viewport,
    selector::Tree,
    sheet::Sheet,
};
use std::sync::Arc;

#[derive(Clone)]
pub struct SheetScope {
    pub sheet: Sheet,
    pub layer: Layer,
    pub root: Option<usize>,
}

/// One result is consumed by both painting and hit testing.
#[derive(Clone, Debug)]
pub struct Snapshot {
    pub styles: Vec<ComputedStyle>,
    pub rectangles: Vec<Option<Rect>>,
}

#[derive(Default)]
pub struct Resolver {
    sheets: Vec<SheetScope>,
    cached: Option<(Tree, Viewport, Arc<Snapshot>)>,
}

impl Resolver {
    pub fn new(sheets: Vec<SheetScope>) -> Result<Self, String> {
        if sheets.len() > 32 {
            return Err("Too many native style sheets".into());
        }
        Ok(Self { sheets, cached: None })
    }

    /// Called only after a source change has parsed. Failure keeps the last working
    /// sheets and snapshot, so a partially saved theme cannot blank a live panel.
    pub fn replace(
        &mut self,
        sheets: Vec<SheetScope>,
        tree: &Tree,
        viewport: Viewport,
    ) -> Result<Arc<Snapshot>, String> {
        let mut candidate = Self::new(sheets)?;
        let snapshot = candidate.resolve(tree, viewport)?;
        *self = candidate;
        Ok(snapshot)
    }

    pub fn resolve(&mut self, tree: &Tree, viewport: Viewport) -> Result<Arc<Snapshot>, String> {
        if let Some((previous, size, result)) = &self.cached
            && previous == tree
            && *size == viewport
        {
            return Ok(Arc::clone(result));
        }
        let sheets = self
            .sheets
            .iter()
            .map(|sheet| AppliedSheet { sheet: &sheet.sheet, layer: sheet.layer, root: sheet.root })
            .collect::<Vec<_>>();
        let styles = cascade::compute(tree, viewport, &sheets)?;
        // Keep exact viewport dimensions in the cache key. Percentage sizes change
        // even without crossing a media breakpoint.
        let rectangles = layout::compute(tree, &styles, [viewport.width, viewport.height])?;
        let result = Arc::new(Snapshot { styles, rectangles });
        self.cached = Some((tree.clone(), viewport, Arc::clone(&result)));
        Ok(result)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::selector::Element;
    fn scope(source: &str) -> SheetScope {
        SheetScope { sheet: Sheet::parse(source).unwrap(), layer: Layer::Theme, root: None }
    }
    #[test]
    fn cached_snapshot_invalidates_on_dimensions_state_and_source() {
        let tree =
            Tree::new(vec![Element::new("window", None), Element::new("knob", Some(0)).with_id("cutoff")]).unwrap();
        let viewport = Viewport { width: 800.0, height: 600.0, dark: true, reduced_motion: false };
        let mut resolver =
            Resolver::new(vec![scope("knob { width: 50%; height: 40px; } knob:hover { width: 25%; }")]).unwrap();
        let first = resolver.resolve(&tree, viewport).unwrap();
        assert!(Arc::ptr_eq(&first, &resolver.resolve(&tree, viewport).unwrap()));
        let resized = resolver.resolve(&tree, Viewport { width: 820.0, ..viewport }).unwrap();
        assert_eq!(first.rectangles[1].unwrap().width, 400.0);
        assert_eq!(resized.rectangles[1].unwrap().width, 410.0);
        let mut elements = tree.elements().to_vec();
        elements[1].states.push("hover".into());
        let hovered = resolver.resolve(&Tree::new(elements).unwrap(), viewport).unwrap();
        assert_eq!(hovered.rectangles[1].unwrap().width, 200.0);
        let replacement = resolver.replace(vec![scope("knob { width: 10px; }")], &tree, viewport).unwrap();
        assert_eq!(replacement.rectangles[1].unwrap().width, 10.0);
        assert!(resolver.replace(vec![scope("knob { area: missing; }")], &tree, viewport).is_err());
        assert!(Arc::ptr_eq(&replacement, &resolver.resolve(&tree, viewport).unwrap()));
    }
}
