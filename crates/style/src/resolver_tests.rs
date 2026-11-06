// Copyright (C) 2026 The Suhina contributors
// SPDX-License-Identifier: GPL-3.0-or-later

use super::*;
use crate::selector::Element;
fn scope(source: &str) -> SheetScope {
    SheetScope { sheet: Sheet::parse(source).unwrap(), layer: Layer::Theme, root: None }
}
#[test]
fn cached_snapshot_invalidates_on_dimensions_state_and_source() {
    let tree = Tree::new(vec![Element::new("window", None), Element::new("knob", Some(0)).with_id("cutoff")]).unwrap();
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

fn note(colour: &str, selected: bool) -> Element {
    let mut note = Element::new("note", None).with_attribute("color", colour);
    if selected {
        note.states.push("selected".into());
    }
    note
}

#[test]
fn notes_of_one_identity_share_a_style_until_the_snapshot_changes() {
    use crate::{
        property::Property,
        value::{Color, Value},
    };
    let css = "piano-roll { --lane: #202020; } \
                   note { background-color: color-mix(in srgb, attr(color type(<color>)), var(--lane)); } \
                   note:selected { border-color: contrast-color(attr(color type(<color>))); } \
                   piano-roll:hover note { opacity: 0.5; }";
    let tree = Tree::new(vec![Element::new("window", None), Element::new("piano-roll", Some(0))]).unwrap();
    let viewport = Viewport { width: 800.0, height: 600.0, dark: true, reduced_motion: false };
    let mut resolver = Resolver::new(vec![scope(css)]).unwrap();
    resolver.resolve(&tree, viewport).unwrap();
    let colours = ["#ff0000", "#00ff00", "#0000ff", "#ffff00"];
    let mut first = None;
    for index in 0..10_000 {
        let style = resolver.leaf_style(1, &note(colours[index % 4], index % 3 == 0)).unwrap();
        first.get_or_insert(style);
    }
    assert_eq!(resolver.leaf_stats(), LeafStats { hits: 9_992, misses: 8, entries: 8 });
    // The cached style is the one the element would have in the tree.
    let mut in_tree = tree.elements().to_vec();
    in_tree.push(Element { parent: Some(1), ..note("#ff0000", true) });
    let expected = cascade::compute(
        &Tree::new(in_tree).unwrap(),
        viewport,
        &[AppliedSheet { sheet: &resolver.sheets[0].sheet, layer: Layer::Theme, root: None }],
    )
    .unwrap();
    let cached = first.unwrap();
    for property in [Property::BackgroundColor, Property::BorderColor, Property::Opacity] {
        assert_eq!(cached.get(property), expected[2].get(property), "{property:?}");
    }
    assert_eq!(cached.get(Property::BackgroundColor), Value::Color(Color::Rgba([144, 16, 16, 255])));
    // The same tree again keeps the cache; a hovered parent is a new snapshot.
    resolver.resolve(&tree, viewport).unwrap();
    assert_eq!(resolver.leaf_stats().entries, 8);
    let mut hovered = tree.elements().to_vec();
    hovered[1].states.push("hover".into());
    resolver.resolve(&Tree::new(hovered).unwrap(), viewport).unwrap();
    assert_eq!(resolver.leaf_stats().entries, 0);
    let dimmed = resolver.leaf_style(1, &note("#ff0000", false)).unwrap();
    assert_eq!(dimmed.get(Property::Opacity), Value::Number(0.5));
    resolver.replace(vec![scope("note { opacity: 0.25; }")], &tree, viewport).unwrap();
    assert_eq!(resolver.leaf_style(1, &note("#ff0000", false)).unwrap().get(Property::Opacity), Value::Number(0.25));
    assert_eq!(resolver.leaf_stats(), LeafStats { hits: 9_992, misses: 10, entries: 1 });
}

#[test]
fn a_leaf_is_its_parent_s_only_child_and_is_checked_like_a_tree_element() {
    use crate::{property::Property, value::Value};
    let tree =
        Tree::new(vec![Element::new("window", None), Element::new("lane", Some(0)), Element::new("marker", Some(1))])
            .unwrap();
    let viewport = Viewport { width: 800.0, height: 600.0, dark: true, reduced_motion: false };
    let css =
        "note:only-child { opacity: 0.5; } marker + note, marker ~ note { opacity: 0.1; } lane:empty { opacity: 0.2; }";
    let mut resolver = Resolver::new(vec![scope(css)]).unwrap();
    assert!(resolver.leaf_style(1, &note("#ff0000", false)).is_err());
    let snapshot = resolver.resolve(&tree, viewport).unwrap();
    assert_eq!(resolver.leaf_style(1, &note("#ff0000", false)).unwrap().get(Property::Opacity), Value::Number(0.5));
    assert_eq!(snapshot.styles[1].get(Property::Opacity), Property::Opacity.initial());
    assert!(resolver.leaf_style(9, &note("#ff0000", false)).is_err());
    assert!(resolver.leaf_style(1, &Element::new("note", None).with_attribute("Color", "red")).is_err());
    // A refused leaf leaves the tree as it was.
    assert!(Arc::ptr_eq(&snapshot, &resolver.resolve(&tree, viewport).unwrap()));
    let mut deep = vec![Element::new("window", None)];
    for index in 0..32 {
        deep.push(Element::new("group", Some(index)));
    }
    let deep = Tree::new(deep).unwrap();
    resolver.resolve(&deep, viewport).unwrap();
    assert!(resolver.leaf_style(32, &note("#ff0000", false)).is_err());
    assert!(resolver.leaf_style(31, &note("#ff0000", false)).is_ok());
}

#[test]
fn a_changed_element_drops_only_the_leaves_it_can_reach() {
    use crate::{property::Property, value::Value};
    let lanes = |hovered: Option<usize>| {
        let mut elements = vec![Element::new("arrangement", None)];
        for lane in 0..3 {
            let mut element = Element::new("lane", Some(0));
            if hovered == Some(lane) {
                element.states.push("hover".into());
            }
            elements.push(element);
        }
        Tree::new(elements).unwrap()
    };
    let viewport = Viewport { width: 800.0, height: 600.0, dark: true, reduced_motion: false };
    let fill = |resolver: &mut Resolver| {
        for lane in 1..4 {
            resolver.leaf_style(lane, &note("#ff0000", false)).unwrap();
        }
    };
    let opacity = |resolver: &mut Resolver, lane| {
        resolver.leaf_style(lane, &note("#ff0000", false)).unwrap().get(Property::Opacity)
    };
    let mut plain = Resolver::new(vec![scope("lane:hover note { opacity: 0.5; }")]).unwrap();
    plain.resolve(&lanes(None), viewport).unwrap();
    fill(&mut plain);
    plain.resolve(&lanes(Some(1)), viewport).unwrap();
    assert_eq!(plain.leaf_stats().entries, 2);
    assert_eq!(opacity(&mut plain, 2), Value::Number(0.5));
    assert_eq!(opacity(&mut plain, 3), Property::Opacity.initial());
    // With siblings related, a hovered lane also reaches the lanes after it.
    let mut related = Resolver::new(vec![scope("lane:hover + lane note { opacity: 0.25; }")]).unwrap();
    related.resolve(&lanes(None), viewport).unwrap();
    fill(&mut related);
    related.resolve(&lanes(Some(1)), viewport).unwrap();
    assert_eq!(related.leaf_stats().entries, 1);
    assert_eq!(opacity(&mut related, 3), Value::Number(0.25));
    assert_eq!(opacity(&mut related, 1), Property::Opacity.initial());
    // Another structure or viewport starts again.
    related
        .resolve(&Tree::new(vec![Element::new("arrangement", None), Element::new("lane", Some(0))]).unwrap(), viewport)
        .unwrap();
    assert_eq!(related.leaf_stats().entries, 0);
    fill(&mut plain);
    plain.resolve(&lanes(Some(1)), Viewport { width: 900.0, ..viewport }).unwrap();
    assert_eq!(plain.leaf_stats().entries, 0);
}
