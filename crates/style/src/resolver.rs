// Copyright (C) 2026 The Suhina contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! Retained native style/layout snapshots for immediate-mode hosts.
//!
//! The window's elements (panels, track headers, controls) are a [`Tree`] whose snapshot is kept
//! until the tree, the viewport or the sheets change. What is drawn in bulk (notes, clips, grid
//! lines) is not in the tree: drawing asks for a [`Resolver::leaf_style`] by the element's parent
//! and identity, which is computed once per distinct identity and then served from the cache.
//!
//! **The cache key** is the parent's index and the leaf's whole identity: its kind, id, classes,
//! states, attributes (a clip's `color`), object and host variables. Notes that share these share
//! one style, so a piano roll of thousands of notes in one clip's colour costs a few entries,
//! and an arrangement costs one entry per distinct clip colour and state.
//!
//! **Invalidation** follows the snapshot. A leaf's style depends on its ancestors, which it
//! inherits from and which selectors test, and, when a sheet relates siblings (`+`, `~`), on
//! their preceding siblings. So when a new tree has the same structure as the last one (the
//! same parents) and only some elements changed (a hovered track), the cache drops only the
//! leaves under a changed element, or under a later sibling of one when siblings are related.
//! Another structure, viewport or set of sheets empties it. A leaf's own state changing is
//! another key, not an invalidation.
//!
//! **What a leaf cannot see.** A leaf is styled as its parent's only child, so sibling
//! combinators and child-index pseudo-classes cannot tell one note from the next; the parent's
//! `:empty` does not count leaves. That is the price of a style per identity, not per element.
use crate::{
    cascade::{self, AppliedSheet, ComputedStyle, Layer},
    layout::{self, Rect},
    media::Viewport,
    selector::{Element, Tree},
    sheet::Sheet,
};
use std::{
    collections::HashMap,
    hash::{BuildHasherDefault, Hash, Hasher},
    sync::Arc,
};

/// Most leaf styles kept at once; past it the cache starts again empty.
pub const MAX_LEAF_STYLES: usize = 4096;

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
    leaves: LeafCache,
    /// Whether a sheet relates siblings, so a change reaches a changed element's later siblings.
    siblings: bool,
}

/// How the leaf cache has been used since the resolver was made.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct LeafStats {
    pub hits: u64,
    pub misses: u64,
    /// Styles held now.
    pub entries: usize,
}

/// A leaf's parent, the leaf and its style.
type LeafEntry = (usize, Element, Arc<ComputedStyle>);

#[derive(Default)]
struct LeafCache {
    /// By a hash of the key; each bucket holds the parent, the leaf and its style.
    entries: HashMap<u64, Vec<LeafEntry>, BuildHasherDefault<Identity>>,
    len: usize,
    hits: u64,
    misses: u64,
}

impl LeafCache {
    fn clear(&mut self) {
        self.entries.clear();
        self.len = 0;
    }

    /// Keeps the styles of leaves whose parent is not `dirty`.
    fn retain(&mut self, dirty: &[bool]) {
        self.entries.retain(|_, bucket| {
            bucket.retain(|(parent, _, _)| !dirty.get(*parent).copied().unwrap_or(true));
            !bucket.is_empty()
        });
        self.len = self.entries.values().map(Vec::len).sum();
    }
}

/// For a tree with the same structure as `previous`, which elements' leaves may style
/// differently: the changed elements and their descendants, and with `siblings` also the
/// later siblings of a changed element and theirs. `None` when the structure differs.
fn changed(previous: &Tree, tree: &Tree, siblings: bool) -> Option<Vec<bool>> {
    let (old, new) = (previous.elements(), tree.elements());
    if old.len() != new.len() || old.iter().zip(new).any(|(a, b)| a.parent != b.parent) {
        return None;
    }
    let mut marked = vec![false; new.len()];
    for (index, (a, b)) in old.iter().zip(new).enumerate() {
        if a != b {
            marked[index] = true;
            if siblings {
                for later in tree.siblings(index).iter().skip_while(|own| **own != index) {
                    marked[*later] = true;
                }
            }
        }
    }
    let mut dirty = vec![false; new.len()];
    for (index, element) in new.iter().enumerate() {
        dirty[index] = marked[index] || element.parent.is_some_and(|parent| dirty[parent]);
    }
    Some(dirty)
}

/// A leaf's identity: everything but its parent, which the key holds as an index.
fn same_identity(a: &Element, b: &Element) -> bool {
    a.kind == b.kind
        && a.id == b.id
        && a.classes == b.classes
        && a.states == b.states
        && a.attributes == b.attributes
        && a.object == b.object
        && a.variables == b.variables
}

/// A fast multiplicative hash (as in rustc's `FxHasher`): the keys come from the host, not from
/// a sheet, and a collision costs only a comparison.
#[derive(Default)]
struct LeafHasher(u64);

impl Hasher for LeafHasher {
    fn write(&mut self, bytes: &[u8]) {
        let (words, rest) = bytes.as_chunks::<8>();
        for word in words {
            self.write_u64(u64::from_le_bytes(*word));
        }
        for byte in rest {
            self.write_u64(u64::from(*byte));
        }
    }

    fn write_u64(&mut self, word: u64) {
        self.0 = (self.0.rotate_left(5) ^ word).wrapping_mul(0x51_7c_c1_b7_27_22_0a_95);
    }

    fn write_usize(&mut self, word: usize) {
        self.write_u64(word as u64);
    }

    fn finish(&self) -> u64 {
        self.0
    }
}

/// Buckets are already keyed by a hash, so the map itself need not hash again.
#[derive(Default)]
struct Identity(u64);

impl Hasher for Identity {
    fn write(&mut self, bytes: &[u8]) {
        for byte in bytes {
            self.0 = self.0.rotate_left(8) ^ u64::from(*byte);
        }
    }

    fn write_u64(&mut self, word: u64) {
        self.0 = word;
    }

    fn finish(&self) -> u64 {
        self.0
    }
}

fn leaf_key(parent: usize, leaf: &Element) -> u64 {
    let mut hasher = LeafHasher::default();
    parent.hash(&mut hasher);
    leaf.kind.hash(&mut hasher);
    leaf.id.hash(&mut hasher);
    leaf.classes.hash(&mut hasher);
    leaf.states.hash(&mut hasher);
    leaf.attributes.hash(&mut hasher);
    leaf.object.hash(&mut hasher);
    leaf.variables.hash(&mut hasher);
    hasher.finish()
}

impl Resolver {
    pub fn new(sheets: Vec<SheetScope>) -> Result<Self, String> {
        if sheets.len() > 32 {
            return Err("Too many native style sheets".into());
        }
        let siblings = sheets.iter().any(|scope| {
            scope.sheet.rules.iter().any(|rule| rule.selectors.iter().any(crate::selector::Selector::relates_siblings))
        });
        Ok(Self { sheets, cached: None, leaves: LeafCache::default(), siblings })
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
        candidate.leaves.hits = self.leaves.hits;
        candidate.leaves.misses = self.leaves.misses;
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
        match self.cached.as_ref().filter(|(_, size, _)| *size == viewport) {
            Some((previous, _, _)) => match changed(previous, tree, self.siblings) {
                Some(dirty) => self.leaves.retain(&dirty),
                None => self.leaves.clear(),
            },
            None => self.leaves.clear(),
        }
        self.cached = Some((tree.clone(), viewport, Arc::clone(&result)));
        Ok(result)
    }

    /// The style of a drawn element under `parent`, a node of the tree last given to
    /// [`Resolver::resolve`]. Call `resolve` once per frame, then this once per note: notes with
    /// the same identity share one computation until the snapshot changes. `leaf.parent` is
    /// ignored.
    pub fn leaf_style(&mut self, parent: usize, leaf: &Element) -> Result<Arc<ComputedStyle>, String> {
        let Self { sheets, cached, leaves, .. } = self;
        let Some((tree, viewport, snapshot)) = cached else {
            return Err("No style snapshot to style a leaf under".into());
        };
        let key = leaf_key(parent, leaf);
        if let Some((_, _, style)) = leaves
            .entries
            .get(&key)
            .and_then(|bucket| bucket.iter().find(|(own, element, _)| *own == parent && same_identity(element, leaf)))
        {
            leaves.hits += 1;
            return Ok(Arc::clone(style));
        }
        leaves.misses += 1;
        let parent_style = snapshot.styles.get(parent).ok_or("Invalid style parent")?;
        let applied = sheets
            .iter()
            .map(|sheet| AppliedSheet { sheet: &sheet.sheet, layer: sheet.layer, root: sheet.root })
            .collect::<Vec<_>>();
        let node = tree.push_leaf(leaf.clone(), parent)?;
        let computed = cascade::element_style(
            tree,
            node,
            Some(parent_style),
            *viewport,
            &applied,
            false,
            &mut cascade::Budget::default(),
        );
        tree.pop_leaf();
        let style = Arc::new(computed?);
        if leaves.len >= MAX_LEAF_STYLES {
            leaves.clear();
        }
        let mut element = leaf.clone();
        element.parent = Some(parent);
        leaves.entries.entry(key).or_default().push((parent, element, Arc::clone(&style)));
        leaves.len += 1;
        Ok(style)
    }

    pub fn leaf_stats(&self) -> LeafStats {
        LeafStats { hits: self.leaves.hits, misses: self.leaves.misses, entries: self.leaves.len }
    }
}

#[cfg(test)]
#[path = "resolver_tests.rs"]
mod tests;
