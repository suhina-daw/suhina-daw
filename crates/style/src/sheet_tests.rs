// Copyright (C) 2026 The Suhina contributors
// SPDX-License-Identifier: GPL-3.0-or-later

use super::*;

/// A value with `light-dark()` is checked as both schemes see it: each branch must be a value
/// of the property, and a call without two values is named.
#[test]
fn light_dark_is_checked_for_both_schemes() {
    let notices = |css: &str| Sheet::parse(css).unwrap().notices;
    let found = notices("a { color: light-dark(#fff, #000); box-shadow: 0 1px 2px light-dark(red, blue); }");
    assert!(found.is_empty(), "{found:?}");
    let notices = |css: &str| notices(css).len();
    assert_eq!(notices("a { color: light-dark(#fff, 4px); }"), 1);
    assert_eq!(notices("a { color: light-dark(#fff); }"), 1);
    assert_eq!(notices("a { box-shadow: 0 1px 2px light-dark(red, x y); }"), 1);
}

/// Padding in percent is taken only where the CSS layout places an element's children (the
/// window, the toolbar): elsewhere the app paints the content box without the box around
/// the element, so it is refused with a notice rather than read as no padding (#1336).
#[test]
fn padding_in_percent_is_read_only_on_a_layout_container() {
    // `:is()` and `:where()` that hold only layout containers name them too; `:not()` does not.
    let named = Sheet::parse(
        ":is(toolbar, #window) { padding: 1%; } :where(toolbar) { padding-top: 2%; } \
             :is(toolbar, device) { padding: 1%; } :not(device) { padding: 1%; }",
    )
    .unwrap();
    assert_eq!(named.notices.len(), 2, "{:?}", named.notices);
    let kept: Vec<_> = named.rules.iter().map(|r| r.declarations.len()).collect();
    assert_eq!(kept, [4, 1, 0, 0]);
    let sheet = Sheet::parse(
        "window { padding: 2%; } #window, toolbar { padding-left: 5%; } \
             device { padding: 5%; } browser { padding-top: 10%; } toolbar, device { padding: 1% 4px; } \
             device { padding: 4px 1em; }",
    )
    .unwrap();
    let messages: Vec<_> = sheet.notices.iter().map(|n| n.message.as_str()).collect();
    assert_eq!(messages.len(), 3, "{messages:?}");
    assert!(messages.iter().all(|m| m.contains("percentage is read only on a CSS layout container")), "{messages:?}");
    // The refused declarations are dropped; the others stay.
    let kept: Vec<_> = sheet.rules.iter().map(|r| r.declarations.len()).collect();
    assert_eq!(kept, [4, 1, 0, 0, 0, 4]);
}

/// One declaration checked on its own, as a sheet checks it: a shorthand by its parts (#1336).
#[test]
fn a_declaration_is_accepted_as_a_sheet_accepts_it() {
    assert!(accepts("border-width", "1px"));
    assert!(accepts("border-width", "0 0 2px"));
    assert!(accepts("border-top-width", "2px"));
    assert!(accepts("padding", "4px 8px"));
    assert!(accepts("gap", "6px"));
    assert!(accepts("color", "#fff"));
    // A math function is one side, whatever spaces it holds.
    assert!(accepts("border-width", "calc(1px + 1px)"));
    assert!(accepts("border-width", "0 calc(1px + 1px) min(2px, 4px)"));
    assert!(accepts("padding", "calc(2px * 3) 4px"));
    assert_eq!(top_level_words(" a  calc(1px + (2px)) b "), ["a", "calc(1px + (2px))", "b"]);
    assert!(!accepts("border-width", "1px 2px 3px 4px 5px"));
    assert!(!accepts("border-width", "red"));
    assert!(!accepts("tint", "#fff"));
    assert!(accepts("image", "url(a.svg)"));
    assert!(accepts("image", "none"));
    assert!(!accepts("image", "red"));
}

#[test]
fn group_classes_are_declared_with_a_label_and_checked_for_a_rule() {
    let sheet = Sheet::parse(
        "@group-class lcd { label: \"LCD display\"; } \
             @group-class pill { /* no label */ } \
             @group-class typo { label: 'Typo'; } \
             @group-class lcd { label: \"LCD\"; } \
             @group-class 9lives { label: \"Bad\"; } \
             @group-class bad { tint: red; } \
             toolbar group.lcd { background-color: #1a2a1a; } \
             :is(group.pill, button) { radius: 9px; } \
             group.helper { radius: 2px; }",
    )
    .unwrap();
    assert_eq!(
        sheet.group_classes,
        [
            GroupClass { name: "pill".into(), label: None },
            GroupClass { name: "typo".into(), label: Some("Typo".into()) },
            GroupClass { name: "lcd".into(), label: Some("LCD".into()) },
        ]
    );
    assert_eq!(sheet.notices.len(), 2, "{:?}", sheet.notices);
    assert_eq!(sheet.unstyled_group_classes(), ["typo"]);
}

#[test]
fn backdrop_filter_is_read_on_floating_elements_and_named_elsewhere() {
    let sheet = Sheet::parse(
        "popup, menu { backdrop-filter: blur(20px); } dialog[kind=\"x\"] { backdrop-filter: none; } \
             tooltip { backdrop-filter: blur(0); } window dialog { backdrop-filter: blur(4.5px); } \
             settings { backdrop-filter: blur(12px); }",
    )
    .unwrap();
    assert!(sheet.notices.is_empty(), "{:?}", sheet.notices);
    let sheet = Sheet::parse(
        "workspace-panel { backdrop-filter: blur(8px); } popup, browser { backdrop-filter: none; } \
             popup button { backdrop-filter: none; } :is(popup, tooltip) { backdrop-filter: none; } \
             menu-bar menu { backdrop-filter: none; } popup#metronome-level { backdrop-filter: none; }",
    )
    .unwrap();
    assert_eq!(sheet.notices.len(), 6, "{:?}", sheet.notices);
    assert!(sheet.notices.iter().all(|n| n.message.contains("backdrop-filter is read only")), "{:?}", sheet.notices);
    for bad in ["blur()", "blur(2em)", "sepia(1)", "blur(-1px)", "blur(NaNpx)"] {
        let sheet = Sheet::parse(&format!("popup {{ backdrop-filter: {bad}; }}")).unwrap();
        assert_eq!(sheet.notices.len(), 1, "{bad}: {:?}", sheet.notices);
    }
    use crate::property::blurs;
    assert!(blurs("blur(20px)") && blurs(" blur( 0.5px ) "));
    assert!(!blurs(" none ") && !blurs("blur(0)") && !blurs("blur(0px)") && !blurs("sepia(1)"));
}

#[test]
fn a_removed_placement_names_what_took_its_place() {
    // Control placements went with #1453: a `.control-*` rule is a rule like any other, and
    // its offsets, direction and anchors are properties no element has. Each notice names the
    // CSS that replaced it (#1478).
    let sheet = Sheet::parse(
        ".control-play { direction: vertical; anchor-left: 4px; left: 2px; } \
             button { top: 1px; RIGHT: 0; bottom: 0; anchor-bottom: 0; } arrangement { compact-below: 40px; } \
             mixer-pan { --suhina-direction: horizontal; } knob { tint: red; }",
    )
    .unwrap();
    let messages: Vec<_> = sheet.notices.iter().map(|notice| notice.message.as_str()).collect();
    assert_eq!(messages.len(), 10, "{messages:?}");
    let expect = [
        ("direction ", "flex-direction"),
        ("anchor-left ", "align-self"),
        ("left ", "margin-left or order"),
        ("top ", "margin-top or order"),
        ("right ", "margin-right or order"),
        ("bottom ", "margin-bottom or order"),
        ("anchor-bottom ", "justify-content"),
        ("compact-below ", "track-header[layout=\"short\"]"),
        ("--suhina-direction ", "flex-direction"),
    ];
    for ((start, names), message) in expect.iter().zip(&messages) {
        assert!(message.starts_with(start) && message.contains(names), "{message}");
    }
    assert!(messages[2].contains("#mixer-volume"), "{}", messages[2]);
    assert_eq!(messages[9], "Unknown native property tint");
    assert!(sheet.rules.iter().all(|rule| rule.declarations.is_empty()), "the engine keeps none of them");
}
