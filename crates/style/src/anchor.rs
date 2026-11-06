// Copyright (C) 2026 The Suhina contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! Persistent component anchors: one edge of an element follows an edge of another element in
//! the same container, with a gap. Toolkit-independent: names, parsing, validation of the anchor
//! graph and the arithmetic of one axis. The UI resolves the target's rectangle.
use std::collections::{BTreeMap, BTreeSet, VecDeque};
use std::fmt;

/// Longest name of an element, in bytes.
pub const MAX_NAME: usize = 64;
/// Longest chain of anchors (A follows B follows C ...), in links.
pub const MAX_CHAIN: usize = 8;
/// Largest gap in either direction, in logical points.
pub const MAX_GAP: i32 = 16384;
/// Most elements an anchor graph may hold; larger graphs anchor nothing.
pub const MAX_NODES: usize = 1024;

/// A bounded element name: lowercase ASCII letters, digits and `-`, 1 to [`MAX_NAME`] bytes.
/// Fixed-size so a placement holding anchors stays `Copy`.
#[derive(Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct Name {
    len: u8,
    bytes: [u8; MAX_NAME],
}

impl Name {
    pub fn new(text: &str) -> Option<Self> {
        if !valid_name(text) {
            return None;
        }
        let mut bytes = [0; MAX_NAME];
        bytes.get_mut(..text.len())?.copy_from_slice(text.as_bytes());
        Some(Self { len: u8::try_from(text.len()).ok()?, bytes })
    }

    pub fn as_str(&self) -> &str {
        self.bytes.get(..usize::from(self.len)).and_then(|bytes| std::str::from_utf8(bytes).ok()).unwrap_or("")
    }
}

impl fmt::Debug for Name {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{:?}", self.as_str())
    }
}

/// Whether `name` can name an element.
pub fn valid_name(name: &str) -> bool {
    !name.is_empty()
        && name.len() <= MAX_NAME
        && name.bytes().all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == b'-')
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum Edge {
    Left,
    Top,
    Right,
    Bottom,
}

impl Edge {
    pub const ALL: [Self; 4] = [Self::Left, Self::Top, Self::Right, Self::Bottom];

    pub fn parse(text: &str) -> Option<Self> {
        match text {
            "left" => Some(Self::Left),
            "top" => Some(Self::Top),
            "right" => Some(Self::Right),
            "bottom" => Some(Self::Bottom),
            _ => None,
        }
    }

    pub fn name(self) -> &'static str {
        match self {
            Self::Left => "left",
            Self::Top => "top",
            Self::Right => "right",
            Self::Bottom => "bottom",
        }
    }

    /// 0 for horizontal edges' axis (x), 1 for vertical (y).
    pub fn axis(self) -> usize {
        match self {
            Self::Left | Self::Right => 0,
            Self::Top | Self::Bottom => 1,
        }
    }

    /// Whether this is the edge at the larger coordinate of its axis.
    pub fn is_max(self) -> bool {
        matches!(self, Self::Right | Self::Bottom)
    }

    fn index(self) -> usize {
        match self {
            Self::Left => 0,
            Self::Top => 1,
            Self::Right => 2,
            Self::Bottom => 3,
        }
    }
}

/// One anchored edge: it lies `gap` points away from `edge` of `target`, on the side away from the
/// target for opposite edges (a bottom edge above a top edge), inside for matching edges.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Anchor {
    pub target: Name,
    pub edge: Edge,
    pub gap: i32,
}

impl Anchor {
    /// Parses `<name> <edge> [<gap>px]` for the element's own edge `own`, e.g.
    /// `mixer-volume top 4px`. The target's edge must lie on the same axis.
    pub fn parse(own: Edge, text: &str) -> Result<Self, &'static str> {
        let mut words = text.split_whitespace();
        let target = words.next().and_then(Name::new).ok_or("anchor needs a valid element name")?;
        let edge = words.next().and_then(Edge::parse).ok_or("anchor needs an edge: left, top, right or bottom")?;
        if edge.axis() != own.axis() {
            return Err("an anchor must follow an edge on the same axis");
        }
        let gap = match words.next() {
            None => 0,
            Some(gap) => gap
                .strip_suffix("px")
                .and_then(|gap| gap.parse::<i32>().ok())
                .filter(|gap| (-MAX_GAP..=MAX_GAP).contains(gap))
                .ok_or("anchor gap must be whole px within -16384..16384")?,
        };
        if words.next().is_some() {
            return Err("anchor takes a name, an edge and an optional gap");
        }
        Ok(Self { target, edge, gap })
    }

    /// The coordinate of the anchored edge, given the target's edge coordinate.
    pub fn place(self, own: Edge, target_edge: f32) -> f32 {
        if own.is_max() { target_edge - self.gap as f32 } else { target_edge + self.gap as f32 }
    }
}

impl fmt::Display for Anchor {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{} {} {}px", self.target.as_str(), self.edge.name(), self.gap)
    }
}

/// The anchors of one element, at most one per own edge.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Anchors([Option<Anchor>; 4]);

impl Anchors {
    pub fn get(&self, own: Edge) -> Option<Anchor> {
        self.0.get(own.index()).copied().flatten()
    }

    pub fn set(&mut self, own: Edge, anchor: Option<Anchor>) {
        if let Some(slot) = self.0.get_mut(own.index()) {
            *slot = anchor;
        }
    }

    pub fn is_empty(&self) -> bool {
        self.0.iter().all(Option::is_none)
    }

    /// Own edges with their anchors.
    pub fn iter(&self) -> impl Iterator<Item = (Edge, Anchor)> + '_ {
        Edge::ALL.into_iter().filter_map(|edge| self.get(edge).map(|anchor| (edge, anchor)))
    }
}

/// Why an element's anchors were not accepted.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Rejected {
    /// The element is part of a cycle of anchors, itself included.
    Cycle,
    /// It starts a chain longer than [`MAX_CHAIN`].
    TooDeep,
    /// The graph holds more than [`MAX_NODES`] elements.
    TooLarge,
}

impl fmt::Display for Rejected {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(match self {
            Self::Cycle => "its anchors form a cycle",
            Self::TooDeep => "its anchor chain is longer than 8 links",
            Self::TooLarge => "too many anchored elements",
        })
    }
}

/// The elements whose anchors must be dropped so the rest form short, acyclic chains: every
/// element on a cycle, then every element starting a chain of more than [`MAX_CHAIN`] links
/// among the remaining anchors. Deterministic; work is bounded by [`MAX_NODES`].
pub fn rejected<'a>(graph: impl IntoIterator<Item = (&'a str, Anchors)>) -> BTreeMap<String, Rejected> {
    // Each axis on its own: an element whose x follows another's while that one's y follows it
    // is no cycle, since an edge only ever follows an edge on its own axis.
    let graph: Vec<(&str, Anchors)> = graph.into_iter().collect();
    let mut result = BTreeMap::new();
    for vertical in [false, true] {
        let axis = graph.iter().map(|(name, anchors)| {
            let mut only = Anchors::default();
            for (edge, anchor) in anchors.iter() {
                if matches!(edge, Edge::Top | Edge::Bottom) == vertical {
                    only.set(edge, Some(anchor));
                }
            }
            (*name, only)
        });
        for (name, reason) in rejected_on_axis(axis) {
            result.entry(name).or_insert(reason);
        }
    }
    result
}

fn rejected_on_axis<'a>(graph: impl IntoIterator<Item = (&'a str, Anchors)>) -> BTreeMap<String, Rejected> {
    let mut edges: BTreeMap<String, Vec<String>> = BTreeMap::new();
    for (name, anchors) in graph {
        if anchors.is_empty() {
            continue;
        }
        if edges.len() >= MAX_NODES {
            let mut all: BTreeMap<String, Rejected> = edges.into_keys().map(|n| (n, Rejected::TooLarge)).collect();
            all.insert(name.to_owned(), Rejected::TooLarge);
            return all;
        }
        let targets = anchors.iter().map(|(_, anchor)| anchor.target.as_str().to_owned()).collect();
        edges.insert(name.to_owned(), targets);
    }
    let mut result = BTreeMap::new();
    // A node is on a cycle when it reaches itself; each search visits at most every node once.
    for start in edges.keys() {
        let mut seen = BTreeSet::new();
        let mut queue: VecDeque<&str> = edges.get(start).into_iter().flatten().map(String::as_str).collect();
        while let Some(node) = queue.pop_front() {
            if node == start {
                result.insert(start.clone(), Rejected::Cycle);
                break;
            }
            if seen.insert(node) {
                queue.extend(edges.get(node).into_iter().flatten().map(String::as_str));
            }
        }
    }
    for name in result.keys() {
        edges.remove(name);
    }
    // The remaining graph is acyclic: longest chains by post-order, without recursion.
    let mut depth: BTreeMap<&str, usize> = BTreeMap::new();
    for start in edges.keys() {
        let mut stack = vec![(start.as_str(), false)];
        while let Some((node, expanded)) = stack.pop() {
            if depth.contains_key(node) {
                continue;
            }
            let children = edges.get(node).map(Vec::as_slice).unwrap_or_default();
            if expanded {
                let longest =
                    children.iter().map(|child| depth.get(child.as_str()).map_or(1, |d| d + 1)).max().unwrap_or(0);
                depth.insert(node, longest);
            } else {
                stack.push((node, true));
                stack.extend(
                    children.iter().filter(|child| edges.contains_key(*child)).map(|child| (child.as_str(), false)),
                );
            }
        }
    }
    for (name, links) in depth {
        if links > MAX_CHAIN {
            result.insert(name.to_owned(), Rejected::TooDeep);
        }
    }
    result
}

/// The span `(min, max)` of an element on one axis after its anchored edges: `min` and `max` are
/// the anchored coordinates of its smaller and larger edge, if any. With one edge anchored a
/// `stretch` span (full width or height) keeps its other edge; otherwise it moves and keeps its
/// size. The result stays inside `bounds` and may be empty when the anchors leave no room.
pub fn resolve_axis(
    span: (f32, f32),
    min: Option<f32>,
    max: Option<f32>,
    stretch: bool,
    bounds: (f32, f32),
) -> (f32, f32) {
    let finite = |v: f32| v.is_finite();
    let (lo, hi) = bounds;
    if !finite(lo) || !finite(hi) || hi < lo || !finite(span.0) || !finite(span.1) {
        return span;
    }
    let min = min.filter(|v| finite(*v));
    let max = max.filter(|v| finite(*v));
    let size = (span.1 - span.0).max(0.0);
    let (start, end) = match (min, max) {
        (None, None) => return span,
        (Some(a), Some(b)) => (a, b),
        (Some(a), None) if stretch => (a, span.1),
        (None, Some(b)) if stretch => (span.0, b),
        (Some(a), None) => {
            let size = size.min(hi - lo);
            let a = a.clamp(lo, hi - size);
            return (a, a + size);
        }
        (None, Some(b)) => {
            let size = size.min(hi - lo);
            let b = b.clamp(lo + size, hi);
            return (b - size, b);
        }
    };
    let start = start.clamp(lo, hi);
    (start, end.clamp(start, hi))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn name(text: &str) -> Name {
        Name::new(text).unwrap_or_else(|| panic!("{text}"))
    }

    fn anchors(list: &[(Edge, &str, Edge, i32)]) -> Anchors {
        let mut result = Anchors::default();
        for &(own, target, edge, gap) in list {
            result.set(own, Some(Anchor { target: name(target), edge, gap }));
        }
        result
    }

    #[test]
    fn names_are_bounded_and_round_trip() {
        assert_eq!(name("mixer-volume").as_str(), "mixer-volume");
        assert!(Name::new("").is_none());
        assert!(Name::new("Mixer").is_none());
        assert!(Name::new("a b").is_none());
        assert!(Name::new(&"x".repeat(MAX_NAME + 1)).is_none());
        assert_eq!(name(&"x".repeat(MAX_NAME)).as_str().len(), MAX_NAME);
    }

    #[test]
    fn anchors_parse_and_print_on_the_same_axis_only() {
        let anchor = Anchor::parse(Edge::Bottom, "arrangement-volume top 4px");
        assert_eq!(anchor, Ok(Anchor { target: name("arrangement-volume"), edge: Edge::Top, gap: 4 }));
        assert_eq!(anchor.map(|a| a.to_string()), Ok("arrangement-volume top 4px".into()));
        assert_eq!(Anchor::parse(Edge::Left, "meter right").map(|a| a.to_string()), Ok("meter right 0px".into()));
        for bad in [
            "",
            "meter",
            "meter left",
            "meter top 1.5px",
            "meter top 99999px",
            "meter top 4",
            "meter top 4px extra",
            "Meter top",
        ] {
            assert!(Anchor::parse(Edge::Bottom, bad).is_err(), "{bad}");
        }
    }

    #[test]
    fn opposite_edges_keep_the_gap_between_and_matching_edges_inset() {
        let anchor = Anchor { target: name("t"), edge: Edge::Top, gap: 4 };
        assert_eq!(anchor.place(Edge::Bottom, 100.0), 96.0);
        assert_eq!(anchor.place(Edge::Top, 100.0), 104.0);
        let negative = Anchor { gap: -2, ..anchor };
        assert_eq!(negative.place(Edge::Bottom, 100.0), 102.0);
    }

    #[test]
    fn a_stretched_span_ends_above_its_target_and_a_fixed_one_moves() {
        let bounds = (0.0, 100.0);
        // A full-height meter: from its top inset down to 4 points above the control under it.
        assert_eq!(resolve_axis((2.0, 100.0), None, Some(76.0), true, bounds), (2.0, 76.0));
        // A fixed meter keeps its height and moves up.
        assert_eq!(resolve_axis((10.0, 30.0), None, Some(76.0), false, bounds), (56.0, 76.0));
        assert_eq!(resolve_axis((10.0, 30.0), Some(40.0), None, false, bounds), (40.0, 60.0));
        // Two anchors decide both edges.
        assert_eq!(resolve_axis((10.0, 30.0), Some(20.0), Some(50.0), false, bounds), (20.0, 50.0));
        // Never outside the container; exhausted room is an empty span, not an inverted one.
        assert_eq!(resolve_axis((10.0, 30.0), None, Some(5.0), false, bounds), (0.0, 20.0));
        assert_eq!(resolve_axis((50.0, 90.0), None, Some(20.0), true, bounds), (50.0, 50.0));
        assert_eq!(resolve_axis((10.0, 30.0), Some(f32::NAN), None, false, bounds), (10.0, 30.0));
        assert_eq!(resolve_axis((10.0, 30.0), None, None, true, bounds), (10.0, 30.0));
    }

    #[test]
    fn cycles_and_long_chains_are_rejected_and_the_rest_kept() {
        let graph = [
            ("meter", anchors(&[(Edge::Bottom, "volume", Edge::Top, 4)])),
            ("volume", anchors(&[(Edge::Top, "pan", Edge::Bottom, 0)])),
            ("a", anchors(&[(Edge::Left, "b", Edge::Right, 0)])),
            ("b", anchors(&[(Edge::Left, "a", Edge::Right, 0)])),
            ("self", anchors(&[(Edge::Top, "self", Edge::Bottom, 0)])),
            ("follows-cycle", anchors(&[(Edge::Left, "a", Edge::Right, 0)])),
        ];
        let rejected = rejected(graph.iter().map(|(n, a)| (*n, *a)));
        assert_eq!(rejected.get("a"), Some(&Rejected::Cycle));
        assert_eq!(rejected.get("b"), Some(&Rejected::Cycle));
        assert_eq!(rejected.get("self"), Some(&Rejected::Cycle));
        // Pointing at a cycle is not a cycle: its target falls back at run time.
        assert!(!rejected.contains_key("follows-cycle"));
        assert!(!rejected.contains_key("meter"));
        assert!(!rejected.contains_key("volume"));
    }

    #[test]
    fn following_each_other_on_different_axes_is_no_cycle() {
        // The fader's right follows the meter's left while the meter's top follows the fader's.
        let graph = [
            ("volume", anchors(&[(Edge::Right, "meter", Edge::Left, 0)])),
            ("meter", anchors(&[(Edge::Top, "volume", Edge::Top, 0)])),
        ];
        assert!(rejected(graph.iter().map(|(n, a)| (*n, *a))).is_empty());
        let same_axis = [
            ("volume", anchors(&[(Edge::Right, "meter", Edge::Left, 0)])),
            ("meter", anchors(&[(Edge::Left, "volume", Edge::Right, 0)])),
        ];
        assert_eq!(rejected(same_axis.iter().map(|(n, a)| (*n, *a))).len(), 2);
    }

    #[test]
    fn chains_longer_than_the_limit_are_cut_at_their_start() {
        let names: Vec<String> = (0..=MAX_CHAIN + 1).map(|i| format!("n{i}")).collect();
        let graph: Vec<(&str, Anchors)> = names
            .windows(2)
            .filter_map(|pair| match pair {
                [from, to] => Some((from.as_str(), anchors(&[(Edge::Top, to, Edge::Bottom, 0)]))),
                _ => None,
            })
            .collect();
        let rejected = rejected(graph);
        // n0 → … → n9 is nine links: only n0 starts a chain longer than eight.
        assert_eq!(rejected.get("n0"), Some(&Rejected::TooDeep));
        assert!(!rejected.contains_key("n1"));
        assert_eq!(rejected.len(), 1);
    }

    #[test]
    fn oversized_graphs_anchor_nothing() {
        let names: Vec<String> = (0..=MAX_NODES).map(|i| format!("n{i}")).collect();
        let graph = names.iter().map(|n| (n.as_str(), anchors(&[(Edge::Top, "target", Edge::Bottom, 0)])));
        let rejected = rejected(graph);
        assert_eq!(rejected.len(), MAX_NODES + 1);
        assert!(rejected.values().all(|r| *r == Rejected::TooLarge));
    }
}
