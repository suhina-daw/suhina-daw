// Copyright (C) 2026 The Suhina contributors
// SPDX-License-Identifier: GPL-3.0-or-later

use super::*;
use crate::settings::settings_view::PluginRow;

#[test]
fn rows_are_picked_alone_with_ctrl_and_as_a_run_with_shift() {
    let order: Vec<String> = ["a", "b", "c", "d"].map(String::from).to_vec();
    let mut state = ManagerState::default();
    state.pick("b", false, false, &order);
    state.pick("d", true, false, &order);
    assert_eq!(state.picked, ["b", "d"]);
    state.pick("b", true, false, &order);
    assert_eq!(state.picked, ["d"], "Ctrl takes a picked row out");
    state.pick("a", false, true, &order);
    assert_eq!(state.picked, ["a", "b"], "Shift takes the run from the row clicked last");
    state.pick("c", false, false, &order);
    assert_eq!(state.picked, ["c"], "a plain click picks that row only");
}

#[test]
fn a_right_click_turns_the_picked_rows_in_view_on_or_off() {
    let strings = |list: &[&str]| list.iter().map(|s| s.to_string()).collect::<Vec<_>>();
    let picked = strings(&["b", "gone", "d", "hidden"]);
    // The search shows a to d; "hidden" is out of it, "gone" went in a rescan.
    let order = strings(&["a", "b", "c", "d"]);
    assert_eq!(batch(&picked, "d", &order), ["b", "d"]);
    assert_eq!(batch(&picked, "c", &order), ["c"], "a row not picked: that one alone");
}

fn plugin(name: &str, vendor: &str, kind: &str, format: &str, enabled: bool) -> PluginRow {
    PluginRow {
        name: name.into(),
        vendor: vendor.into(),
        kind: kind.into(),
        format: format.into(),
        version: "1".into(),
        file: format!("/{format}/{name}.{format}"),
        spec: format!("{format}:/{format}/{name}.{format}#00"),
        enabled,
        owned: false,
    }
}

fn view() -> PluginsView {
    PluginsView {
        found: vec![
            plugin("Verb 10", "Zeta", "effect", "vst3", true),
            plugin("Verb 9", "Alpha", "effect", "vst3", false),
            plugin("Keys", "Alpha", "instrument", "clap", true),
        ],
        failed: vec![("/vst3/Broken.vst3".into(), "the plug-in has no factory".into())],
        incompatible: vec![("/vst/Old.dll".into(), "built for x86_64 only; Suhina runs on arm64".into())],
        ..PluginsView::default()
    }
}

fn names(plugins: &PluginsView, state: &ManagerState) -> Vec<String> {
    lines(plugins, state).iter().map(|l| l.cells[1].clone()).collect()
}

#[test]
fn the_table_lists_plugins_and_files_sorted_and_searched() {
    let plugins = view();
    let mut state = ManagerState { sort: (1, false), ..ManagerState::default() };
    assert_eq!(names(&plugins, &state), ["Broken.vst3", "Keys", "Old.dll", "Verb 9", "Verb 10"], "natural order");
    state.sort = (2, false);
    let vendors: Vec<String> = lines(&plugins, &state).iter().map(|l| l.cells[2].clone()).collect();
    assert_eq!(vendors[2..], ["Alpha", "Alpha", "Zeta"]);
    state.sort = (6, true);
    let first = lines(&plugins, &state).into_iter().next().map(|l| l.cells[6].clone());
    assert_eq!(first.as_deref(), Some("Wrong architecture"), "status, descending");
    state.search = "zeta".into();
    assert_eq!(lines(&plugins, &state).len(), 1);
    state.search = "x86".into();
    let found = lines(&plugins, &state);
    assert_eq!((found.len(), found[0].kind), (1, Kind::Incompatible(0)), "the reason is searched too");
}

#[test]
fn the_sidebar_shows_a_shelf_a_type_some_formats_and_a_vendor() {
    let plugins = view();
    let mut state = ManagerState { sort: (1, false), ..ManagerState::default() };
    state.shelf = Shelf::Enabled;
    assert_eq!(names(&plugins, &state), ["Keys", "Verb 10"]);
    state.shelf = Shelf::Disabled;
    assert_eq!(names(&plugins, &state), ["Verb 9"]);
    state.shelf = Shelf::Attention;
    assert_eq!(names(&plugins, &state), ["Broken.vst3", "Old.dll"], "failed and other processor");
    state.shelf = Shelf::Failed;
    assert_eq!(names(&plugins, &state), ["Broken.vst3"]);
    state.shelf = Shelf::Incompatible;
    assert_eq!(names(&plugins, &state), ["Old.dll"]);
    state.shelf = Shelf::All;
    state.types = TypeFilter::Instruments;
    assert_eq!(names(&plugins, &state), ["Keys"], "a type leaves the files out");
    state.types = TypeFilter::Effects;
    assert_eq!(names(&plugins, &state), ["Verb 9", "Verb 10"]);
    state.types = TypeFilter::All;
    state.formats = vec!["vst2".into()];
    assert_eq!(names(&plugins, &state), ["Old.dll"], "a library file is a VST2 one");
    state.formats.push("clap".into());
    assert_eq!(names(&plugins, &state), ["Keys", "Old.dll"]);
    state.formats.clear();
    state.vendor = Some("Alpha".into());
    assert_eq!(names(&plugins, &state), ["Keys", "Verb 9"]);
    state.search = "verb".into();
    state.reset_filters();
    assert_eq!(names(&plugins, &state).len(), 5, "Reset filters shows every row again");
    assert_eq!(vendors(&plugins), ["Alpha", "Zeta"], "each vendor once");
}

#[test]
fn the_notice_counts_the_files_that_need_attention() {
    let counts = Counts::of(&view());
    assert_eq!((counts.plugins, counts.enabled, counts.disabled, counts.attention()), (3, 2, 1, 2));
    assert_eq!(counts.notice().as_deref(), Some("1 file failed to scan · 1 uses an incompatible architecture"));
    let many = Counts { failed: 6, incompatible: 6, ..Counts::default() };
    assert_eq!(many.notice().as_deref(), Some("6 files failed to scan · 6 use an incompatible architecture"));
    let other = Counts { incompatible: 2, ..Counts::default() };
    assert_eq!(other.notice().as_deref(), Some("2 files use an incompatible architecture"));
    assert_eq!(Counts::default().notice(), None, "nothing to review, no notice");
}

#[test]
fn the_file_fills_what_the_other_columns_leave() {
    let area = Rect::from_min_size(Pos2::ZERO, Vec2::new(1000.0, ROW));
    let spans = spans(area, &WIDTHS, true, true);
    assert_eq!(spans[0].0, 0.0);
    assert!((spans[ACTION].1 - 1000.0).abs() < 0.01, "the columns fill the width");
    assert!(spans.windows(2).all(|w| (w[0].1 - w[1].0).abs() < 0.01), "no gaps");
    let without = spans_of_file(area, false);
    assert!(without > spans[FILE].1 - spans[FILE].0, "no file rows: the action column gives its room to the file");
}

fn spans_of_file(area: Rect, actions: bool) -> f32 {
    let (left, right) = spans(area, &WIDTHS, actions, true)[FILE];
    right - left
}

#[test]
fn a_dragged_width_is_kept_no_narrower_than_the_column_allows() {
    let mut state = ManagerState::default();
    assert_eq!(state.column_widths(), WIDTHS, "the widths at first");
    state.set_width(2, 260.4);
    assert_eq!(state.widths.len(), COLUMN_COUNT, "every width is kept once one is dragged");
    assert_eq!(state.widths[2], 260);
    assert_eq!(state.column_widths()[1], WIDTHS[1], "the others keep theirs");
    state.set_width(1, 3.0);
    assert_eq!(state.column_widths()[1], MIN_WIDTHS[1]);
    // Widths read back from the settings, shorter than the columns, fill the rest from the start.
    let read = ManagerState { widths: vec![28, 300], ..ManagerState::default() };
    assert_eq!(read.column_widths()[1], 300.0);
    assert_eq!(read.column_widths()[3], WIDTHS[3]);
}

#[test]
fn files_of_the_same_name_in_two_folders_get_a_scan_again_each() {
    let (one, two) = (r"C:\Windows\System32\d3dcompiler_47.dll", r"C:\Plugins\VST\d3dcompiler_47.dll");
    assert_ne!(scan_again_id(one), scan_again_id(two), "the id is made from the whole path (#1461)");
    assert_ne!(scan_again_label(one), scan_again_label(two), "the label names the folder too");
    assert!(scan_again_label(one).starts_with("Scan again d3dcompiler_47.dll"), "and still the file");
}

#[test]
fn the_second_of_two_same_named_files_is_scanned_again_by_its_own_button() {
    use egui_kittest::{Harness, kittest::Queryable};
    let one = "/one/d3dcompiler_47.dll".to_string();
    let two = "/two/d3dcompiler_47.dll".to_string();
    let plugins = PluginsView {
        failed: vec![(one.clone(), "could not load".into())],
        incompatible: vec![(two.clone(), "built for x86 only".into())],
        ..PluginsView::default()
    };
    let mut harness = Harness::builder().allow_missing_glyphs().with_size(egui::vec2(1180.0, 600.0)).build_ui_state(
        |ui, state: &mut (PluginsView, Option<PluginRequest>)| {
            crate::kit::theme::install(ui.ctx());
            let area = ui.max_rect();
            if let Some(request) = show(ui, area, &mut state.0).request {
                state.1 = Some(request);
            }
        },
        (plugins, None),
    );
    harness.run();
    let first = harness.get_by_label(&scan_again_label(&one)).rect();
    let second = harness.get_by_label(&scan_again_label(&two)).rect();
    assert_ne!(first, second, "two buttons, one in each row");
    harness.get_by_label(&scan_again_label(&two)).click();
    harness.run();
    assert_eq!(harness.state().1, Some(PluginRequest::ScanFiles(vec![two])), "the click reaches its own row");
}

#[test]
fn columns_are_hidden_and_shown_again_and_a_wide_table_scrolls_sideways() {
    let mut state = ManagerState::default();
    assert!((0..COLUMN_COUNT).all(|c| state.shows(c)), "every column at first");
    state.set_shown(FILE, false);
    state.set_shown(2, false);
    assert_eq!(state.hidden, ["File", "Vendor"]);
    assert!(!state.shows(FILE) && !state.shows(2));
    state.set_shown(1, false);
    assert!(state.shows(1), "the name is never hidden");
    let widths = state.shown_widths();
    assert_eq!((widths[2], widths[FILE]), (0.0, 0.0), "a hidden column takes no width");
    let area = Rect::from_min_size(Pos2::ZERO, Vec2::new(1000.0, ROW));
    let spans = spans(area, &widths, true, false);
    assert_eq!(spans[FILE].0, spans[FILE].1, "the file hidden: no width for it");
    state.set_shown(FILE, true);
    assert_eq!(state.hidden, ["Vendor"]);
    // Narrower than the columns and a path's room: the table is laid out wider and scrolls.
    let all = ManagerState::default();
    let others: f32 = WIDTHS.iter().enumerate().filter(|(i, _)| *i != FILE).map(|(_, w)| w).sum();
    assert_eq!(all.content_width(400.0, true), others + FILE_WANT);
    assert_eq!(all.content_width(5000.0, true), 5000.0, "wide enough: the table fills the room");
}

#[test]
fn the_menu_offers_only_the_switch_that_changes_something() {
    let plugins = view();
    let spec = |name: &str| plugins.found.iter().find(|p| p.name == name).map(|p| p.spec.clone()).unwrap_or_default();
    let keys = vec![spec("Verb 10"), spec("Verb 9"), spec("Keys"), file_key("/vst3/Broken.vst3")];
    let (on, off) = split_by_state(&keys, &plugins);
    assert_eq!(on, [spec("Verb 10"), spec("Keys")], "Turn off: the ones on");
    assert_eq!(off, [spec("Verb 9")], "Turn on: the ones off; the file is neither");
    let (on, off) = split_by_state(&[spec("Verb 9")], &plugins);
    assert!(on.is_empty() && off.len() == 1, "every one picked is off: no Turn off");
}

#[test]
fn one_switch_turns_every_picked_plug_in_on_or_off() {
    let plugins = view();
    let spec = |name: &str| plugins.found.iter().find(|p| p.name == name).map(|p| p.spec.clone()).unwrap_or_default();
    let mixed = vec![spec("Verb 10"), spec("Verb 9")];
    assert_eq!(
        switch_for_many(&mixed, &plugins),
        Some((false, vec![spec("Verb 9")], true)),
        "some off: shows off, turns them on"
    );
    let on = vec![spec("Verb 10"), spec("Keys")];
    assert_eq!(switch_for_many(&on, &plugins), Some((true, on.clone(), false)), "all on: shows on, turns them all off");
    assert_eq!(switch_for_many(&[file_key("/vst3/Broken.vst3")], &plugins), None, "files only: no switch");
}

#[test]
fn the_rest_of_a_vendor_is_what_is_on_and_not_marked_as_owned() {
    let mut plugins = view();
    let spec = |plugins: &PluginsView, name: &str| {
        plugins.found.iter().find(|p| p.name == name).map(|p| p.spec.clone()).unwrap_or_default()
    };
    plugins.found.push(plugin("Comp", "Alpha", "effect", "vst3", true));
    assert!(rest_of_vendor("Alpha", &plugins).is_empty(), "nothing marked: nothing is turned off");
    if let Some(keys) = plugins.found.iter_mut().find(|p| p.name == "Keys") {
        keys.owned = true;
    }
    assert_eq!(rest_of_vendor("Alpha", &plugins), [spec(&plugins, "Comp")], "on and not owned; Verb 9 is off already");
    assert!(rest_of_vendor("Zeta", &plugins).is_empty(), "another vendor is left alone");
    let state = ManagerState { shelf: Shelf::Owned, ..ManagerState::default() };
    assert_eq!(names(&plugins, &state), ["Keys"], "the Owned shelf");
    let keys = vec![spec(&plugins, "Keys"), spec(&plugins, "Comp")];
    assert_eq!(split_by_owned(&keys, &plugins), (vec![spec(&plugins, "Keys")], vec![spec(&plugins, "Comp")]));
    assert_eq!(Counts::of(&plugins).owned, 1);
}
