// Copyright (C) 2026 The Suhina contributors
// SPDX-License-Identifier: GPL-3.0-or-later

use super::*;
use crate::kit::skin;
use std::sync::Arc;

/// Lays `items` out in a `bar` `width` wide and 30 tall under `css`.
fn laid(css: &str, bar: &'static str, width: f32, items: &[Item]) -> Row {
    let ctx = egui::Context::default();
    skin::configure(&skin::Skin { css: Some(Arc::from(css)), ..Default::default() });
    ctx.run_ui(Default::default(), |ui| crate::kit::native_css::begin_frame(ui)).textures_delta.clear();
    let row = row(&Bar::new(bar, Vec2::new(width, 30.0)), items);
    skin::configure(&skin::Skin::default());
    row.map(|row| (*row).clone()).unwrap_or_default()
}

fn three() -> Vec<Item> {
    ["a", "b", "c"].into_iter().map(|id| Item::new("bar-item").id(id).width(40.0)).collect()
}

#[test]
fn gap_and_padding_move_the_items() {
    let plain = laid("", "status-bar", 400.0, &three());
    let moved = laid("status-bar { padding: 0 10px; gap: 20px; }", "status-bar", 400.0, &three());
    let lefts = |row: &Row| row.items.iter().map(|r| r.map(|r| r.left())).collect::<Vec<_>>();
    // The user-agent sheet's padding and gap (6 points).
    assert_eq!(lefts(&plain), [Some(6.0), Some(52.0), Some(98.0)]);
    assert_eq!(lefts(&moved), [Some(10.0), Some(70.0), Some(130.0)]);
    assert!(moved.fits);
}

#[test]
fn display_none_takes_an_item_away() {
    let row = laid("#b { display: none; }", "status-bar", 400.0, &three());
    assert!(row.items[1].is_none());
    assert_eq!(row.items[2].map(|r| r.left()), Some(52.0), "the next item closes up");
}

#[test]
fn a_flexible_space_takes_the_room_left() {
    let mut items = three();
    items.insert(1, Item::new("flexible-space"));
    let row = laid("", "status-bar", 400.0, &items);
    // 400 less the padding (12), three items (120) and three gaps (18).
    assert_eq!(row.items[1].map(|r| r.width()), Some(250.0));
    assert_eq!(row.items[3].map(|r| r.right()), Some(394.0));
}

#[test]
fn items_in_a_group_lie_in_the_group() {
    let items = vec![
        Item::named("group", "toolbar-end"),
        Item::new("bar-item").id("a").width(40.0).in_group(0),
        Item::new("bar-item").id("b").width(40.0).in_group(0),
    ];
    let row = laid("", "toolbar", 400.0, &items);
    // The end group takes the room and puts its items at its end.
    assert_eq!(row.items[2].map(|r| r.right()), Some(394.0));
    // A group after its item is malformed.
    assert_eq!(super::row(&Bar::new("toolbar", Vec2::new(400.0, 30.0)), &[Item::new("group").in_group(0)]), None);
}

#[test]
fn a_bar_gives_way_until_its_items_fit() {
    let wide: Vec<_> = (0..5).map(|n| Item::new("bar-item").id(&format!("i{n}")).width(100.0)).collect();
    let narrow = three();
    let candidates = [wide, narrow];
    let at = |width: f32, css: &str| {
        let ctx = egui::Context::default();
        skin::configure(&skin::Skin { css: Some(Arc::from(css)), ..Default::default() });
        ctx.run_ui(Default::default(), |ui| crate::kit::native_css::begin_frame(ui)).textures_delta.clear();
        let chosen = fitting(&Bar::new("status-bar", Vec2::new(width, 30.0)), &candidates).map(|(i, _)| i);
        skin::configure(&skin::Skin::default());
        chosen
    };
    assert_eq!(at(800.0, ""), Some(0));
    assert_eq!(at(400.0, ""), Some(1));
    // A theme that lets the items shrink still gives way: they no longer have their width.
    assert_eq!(at(400.0, "bar-item { min-width: 0; flex-shrink: 1; }"), Some(1));
    // The items are measured by their content: padding shrunk into the same width is not room.
    // Five 100-point items with 10 points of padding either side and no gap need 600 points; in
    // 500 each box stays 100 wide but its content box shrinks to 80, so the sparser set is chosen.
    let padded = "bar-item { padding: 0 10px; min-width: 0; flex-shrink: 1; } status-bar { gap: 0; padding: 0; }";
    assert_eq!(at(500.0, padded), Some(1));
    assert_eq!(at(600.0, padded), Some(0));
    // `width: auto` and `max-width: none` give no size: the items still shrink below theirs.
    assert_eq!(at(400.0, "bar-item { width: auto; max-width: none; min-width: 0; flex-shrink: 1; }"), Some(1));
    // Nothing fits: the sparsest, clipped.
    assert_eq!(at(50.0, ""), Some(1));
}

#[test]
fn hostile_input_lays_out_nothing_or_finite_rectangles() {
    // No room lays nothing out; hostile sizes are clamped.
    assert_eq!(super::row(&Bar::new("toolbar", Vec2::ZERO), &three()), None);
    assert_eq!(super::row(&Bar::new("toolbar", Vec2::new(f32::NAN, 30.0)), &three()), None);
    let huge = vec![Item::new("bar-item").width(f32::INFINITY), Item::new("bar-item").width(-5.0)];
    let row = laid("", "status-bar", 400.0, &huge);
    assert!(row.items.iter().flatten().all(|r| r.is_finite()));
    let many: Vec<_> = (0..=MAX_ITEMS).map(|_| Item::new("bar-item")).collect();
    assert_eq!(super::row(&Bar::new("status-bar", Vec2::new(400.0, 30.0)), &many), None);
}

#[test]
fn a_group_s_height_is_its_children_s_unless_the_sheet_sets_one() {
    let items = vec![Item::new("group").id("g"), Item::new("bar-item").id("a").width(40.0).height(20.0).in_group(0)];
    let height = |css: &str| laid(css, "piano-roll-toolbar", 400.0, &items).items[0].map(|r| r.height());
    assert!(height("").is_some_and(|h| (20.0..30.0).contains(&h)), "the child's height, with its room");
    assert_eq!(height("group { height: 8px; }"), Some(8.0));
}

#[test]
fn css_order_does_not_reorder_a_bar_s_items() {
    let plain = laid("", "status-bar", 400.0, &three());
    let ordered = laid("#a { order: 1; } #c { order: -1; }", "status-bar", 400.0, &three());
    assert_eq!(ordered.items, plain.items, "the app's order is the bar's");
}

#[test]
fn a_bar_gives_the_room_its_padding_takes_and_its_attributes_choose_the_rules() {
    // Where no theme's sheet runs, the user-agent sheet's padding (#1023): the room a bar's items
    // are measured for is the room the layout gives them.
    skin::configure(&skin::Skin::default());
    let bar = Bar::new("piano-roll-toolbar", Vec2::new(400.0, 28.0));
    assert_eq!(insets(&bar), Some([3.0, 4.0, 3.0, 8.0]));
    // A bar given its Toolbar size lays out with that size's gap.
    let items = three();
    let normal = row(&bar, &items).unwrap_or_default();
    let large = row(&bar.clone().attribute("size", "large"), &items).unwrap_or_default();
    let step = |row: &Row| row.items[1].zip(row.items[0]).map(|(b, a)| b.left() - a.left());
    assert_eq!(step(&normal), Some(43.0));
    assert_eq!(step(&large), Some(43.75));
    assert_eq!(insets(&Bar::new("piano-roll-toolbar", Vec2::ZERO)), None, "no room, no layout");
}

/// A frame that lays out more rows than the cache keeps lays none of them out again the next
/// frame: a full cache drops only the rows no frame used lately.
#[test]
fn a_settled_frame_lays_out_nothing_again_even_past_the_cache_bound() {
    let ctx = egui::Context::default();
    skin::configure(&skin::Skin { css: Some(Arc::from("status-bar { gap: 3px; }")), ..Default::default() });
    let frame = |ctx: &egui::Context| {
        ctx.run_ui(Default::default(), |ui| {
            let _frame = crate::kit::native_css::frame(ui);
            for width in 0..MAX_ROWS + 44 {
                let bar = Bar::new("status-bar", Vec2::new(100.0 + width as f32, 30.0));
                assert!(row(&bar, &three()).is_some());
            }
        })
        .textures_delta
        .clear();
    };
    frame(&ctx);
    let before = computed();
    frame(&ctx);
    let again = computed() - before;
    skin::configure(&skin::Skin::default());
    assert_eq!(again, 0, "{} rows laid out again", again);
}
