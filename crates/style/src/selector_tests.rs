// Copyright (C) 2026 The Suhina contributors
// SPDX-License-Identifier: GPL-3.0-or-later

use super::*;

fn devices() -> Tree {
    let mut cutoff = Element::new("knob", Some(1)).with_id("cutoff");
    cutoff.states.push("hover".into());
    Tree::new(vec![
        Element::new("window", None),
        Element::new("device-panel", Some(0)).with_id("filter-a"),
        cutoff,
        Element::new("knob", Some(1)).with_id("resonance"),
        Element::new("device-panel", Some(0)).with_id("filter-b"),
        Element::new("knob", Some(4)).with_id("cutoff"),
    ])
    .unwrap()
}

#[test]
fn same_control_id_is_local_to_its_device_sheet() {
    let tree = devices();
    let selectors = Selector::parse_list("device-panel > knob#cutoff:hover").unwrap();
    assert!(selectors[0].matches(&tree, 2, Some(1)));
    assert!(!selectors[0].matches(&tree, 5, Some(1)));
    assert!(!Selector::parse_list("window knob").unwrap()[0].matches(&tree, 2, Some(1)));
    assert!(Selector::parse_list("window knob").unwrap()[0].matches(&tree, 2, None));
    assert!(!Selector::parse_list("device-panel + device-panel").unwrap()[0].matches(&tree, 4, Some(4)));
}

#[test]
fn structure_states_and_specificity_follow_the_native_tree() {
    let tree = devices();
    for source in [
        "knob:first-child",
        "knob:nth-child(2n + 1)",
        "device-panel knob:not(:disabled)",
        ":is(button, #cutoff)",
        ":where(window) knob:empty",
    ] {
        assert!(Selector::parse_list(source).unwrap()[0].matches(&tree, 2, None), "{source}");
    }
    for source in ["knob + knob", "knob ~ #resonance", "knob:last-child", "knob:nth-last-child(1)"] {
        assert!(Selector::parse_list(source).unwrap()[0].matches(&tree, 3, None), "{source}");
    }
    assert_eq!(
        Selector::parse_list(":where(#filter-a) > knob:is(.small, #cutoff)").unwrap()[0].specificity,
        Specificity(1, 0, 1)
    );
}

#[test]
fn object_attribute_selects_a_named_project_object() {
    let mut first = Element::new("mixer", Some(0));
    first.object = Some("track-1".into());
    let mut second = Element::new("mixer", Some(0));
    second.object = Some("track-2".into());
    let tree = Tree::new(vec![Element::new("window", None), first, second, Element::new("mixer", Some(0))]).unwrap();
    let named = &Selector::parse_list("mixer[data-object=track-2]").unwrap()[0];
    let quoted = &Selector::parse_list("[data-object='track-1']").unwrap()[0];
    let any = &Selector::parse_list("[data-object]").unwrap()[0];
    assert!(!named.matches(&tree, 1, None) && named.matches(&tree, 2, None));
    assert!(quoted.matches(&tree, 1, None));
    assert!(any.matches(&tree, 1, None) && !any.matches(&tree, 3, None));
    assert_eq!(named.specificity, Specificity(0, 1, 1));
    for bad in ["[data-object=a b]", "[data-object=]", "[data-object==a]", "[-x]", "[data-object=a q]"] {
        assert!(Selector::parse_list(bad).is_err(), "{bad}");
    }
}

#[test]
fn attribute_selectors_test_and_compare_the_host_s_attributes() {
    let tree = Tree::new(vec![
        Element::new("sample-map", None),
        Element::new("sample-point", Some(0)).with_attribute("kind", "kick").with_attribute("selected", ""),
        Element::new("sample-point", Some(0)).with_attribute("kind", "snare-top").with_attribute("tags", "loud short"),
        Element::new("sample-point", Some(0)).with_attribute("kind", "Kick"),
    ])
    .unwrap();
    let matching = |source: &str| -> Vec<usize> {
        let selector = &Selector::parse_list(source).unwrap()[0];
        (0..4).filter(|node| selector.matches(&tree, *node, None)).collect()
    };
    assert_eq!(matching("[selected]"), [1]);
    assert_eq!(matching("sample-point[kind=\"kick\"]"), [1]);
    assert_eq!(matching("[kind=kick i]"), [1, 3]);
    assert_eq!(matching("[kind=kick s]"), [1]);
    assert_eq!(matching("[tags~=short]"), [2]);
    assert_eq!(matching("[kind|=snare]"), [2]);
    assert_eq!(matching("[kind^=sn]"), [2]);
    assert_eq!(matching("[kind$=top]"), [2]);
    assert_eq!(matching("[kind*=are]"), [2]);
    assert_eq!(matching("[kind^='']"), Vec::<usize>::new());
    assert_eq!(matching("[selected='']"), [1]);
    assert_eq!(matching("[KIND=kick]"), [1]);
    assert_eq!(matching(":not([kind])"), [0]);
    assert_eq!(Selector::parse_list("sample-point[kind=kick][selected]").unwrap()[0].specificity, Specificity(0, 2, 1));
}

#[test]
fn hostile_attributes_are_refused_by_the_tree() {
    for (name, value) in [("Color", "red"), ("", "red"), ("data-object", "a"), ("color", "a\u{7}")] {
        let element = Element::new("clip", None).with_attribute(name, value);
        assert!(Tree::new(vec![element]).is_err(), "{name}");
    }
    let long = Element::new("clip", None).with_attribute("color", &"a".repeat(MAX_ATTRIBUTE_BYTES + 1));
    assert!(Tree::new(vec![long]).is_err());
    let twice = Element::new("clip", None).with_attribute("color", "red").with_attribute("color", "blue");
    assert!(Tree::new(vec![twice]).is_err());
    let mut many = Element::new("clip", None);
    for index in 0..=MAX_ATTRIBUTES {
        many = many.with_attribute(&format!("a{index}"), "");
    }
    assert!(Tree::new(vec![many]).is_err());
}

#[test]
fn invalid_tree_and_hostile_selectors_are_rejected() {
    assert!(Tree::new(vec![Element::new("knob", Some(0))]).is_err());
    for source in
        ["", "> knob", "knob >", "knob,,button", "knob:visitted", "[se:cret]", "knob::before", "knob:nth-child(NaN)"]
    {
        assert!(Selector::parse_list(source).is_err(), "{source}");
    }
    assert!(Selector::parse_list(&format!("{}knob{}", ":not(".repeat(10), ")".repeat(10))).is_err());
}

#[test]
fn the_apps_states_are_attributes_not_pseudo_classes() {
    let tree = Tree::new(vec![
        Element::new("window", None),
        Element::new("button", Some(0)).with_attribute("checked", ""),
        Element::new("button", Some(0)),
    ])
    .unwrap();
    let checked = Selector::parse_list("button[checked]").unwrap();
    assert!(checked[0].matches(&tree, 1, None));
    assert!(!checked[0].matches(&tree, 2, None));
    assert_eq!(checked[0].specificity, Specificity(0, 1, 1));
    for state in HOST_ATTRIBUTES {
        let error = Selector::parse_list(&format!("button:{state}")).unwrap_err();
        assert!(error.contains(&format!("[{state}]")), "{error}");
        assert!(Tree::new(vec![Element { states: vec![(*state).into()], ..Element::new("button", None) }]).is_err());
    }
}
