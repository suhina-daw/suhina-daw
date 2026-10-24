// Copyright (C) 2026 The Suhina contributors
// SPDX-License-Identifier: GPL-3.0-or-later

use super::*;

#[test]
fn full_width_uses_left_and_right_insets_and_round_trips() {
    let mut p = Placement::default();
    p.set("width", "100%").unwrap();
    p.set("left", "4.5px").unwrap();
    p.set("right", "6px").unwrap();
    let bounds = Rect::from_min_size(egui::pos2(10.0, 20.0), Vec2::new(100.0, 80.0));
    let native = Rect::from_min_size(bounds.min + Vec2::splat(12.0), Vec2::splat(20.0));
    let at = p.apply(native, bounds);
    assert_eq!(at.left(), 14.5);
    assert_eq!(at.right(), 104.0);
    assert_eq!(at.height(), 20.0);
    let mut warnings = Vec::new();
    assert_eq!(Placement::from_json(&p.to_json(), &mut warnings), p);
    assert!(warnings.is_empty());
    p.set("width", "20px").unwrap();
    assert!(!p.full_width);
}

#[test]
fn fractional_offsets_survive_storage_and_reject_nonfinite_input() {
    let mut p = Placement::default();
    p.set("left", "1.25px").unwrap();
    p.set("top", "-2.125px").unwrap();
    assert_eq!(p.offset(), Vec2::new(1.25, -2.125));
    let mut warnings = Vec::new();
    assert_eq!(Placement::from_json(&p.to_json(), &mut warnings), p);
    assert!(warnings.is_empty());
    assert!(p.set("left", "NaNpx").is_err());
    assert!(p.set("top", "infpx").is_err());
}

#[test]
fn full_height_tracks_panel_edges_and_round_trips() {
    let mut p = Placement::default();
    p.set("height", "100%").unwrap();
    p.set("top", "4px").unwrap();
    p.set("bottom", "6px").unwrap();
    for height in [30.0, 200.0] {
        let bounds = Rect::from_min_size(egui::pos2(10.0, -40.0), Vec2::new(100.0, height));
        let native = Rect::from_min_size(bounds.min + Vec2::new(80.0, 8.0), Vec2::new(10.0, 12.0));
        let at = p.apply(native, bounds);
        assert_eq!(at.top(), bounds.top() + 4.0);
        assert_eq!(at.bottom(), bounds.bottom() - 6.0);
        assert_eq!(at.width(), 10.0);
    }
    let mut warnings = Vec::new();
    assert_eq!(Placement::from_json(&p.to_json(), &mut warnings), p);
    assert!(warnings.is_empty());
    p.set("height", "auto").unwrap();
    assert!(!p.full_height);
    assert!(p.set("height", "200%").is_err());
}

fn anchored_skin(entries: &[(&str, &[(&str, &str)])]) -> super::super::skin::Skin {
    let mut skin = super::super::skin::Skin::default();
    for (name, properties) in entries {
        let mut p = Placement::default();
        for (key, value) in *properties {
            p.set(key, value).unwrap_or_else(|e| panic!("{key}: {e}"));
        }
        skin.controls.insert((*name).into(), p);
    }
    skin
}

fn rect(x: f32, y: f32, w: f32, h: f32) -> Rect {
    Rect::from_min_size(egui::pos2(x, y), Vec2::new(w, h))
}

#[test]
fn anchors_round_trip_through_css_properties_and_json() {
    let mut p = Placement::default();
    p.set("height", "100%").unwrap();
    p.set("--suhina-anchor-bottom", "arrangement-volume top 4px").unwrap();
    p.set("--suhina-anchor-left", "arrangement-name right").unwrap();
    let mut warnings = Vec::new();
    assert_eq!(Placement::from_json(&p.to_json(), &mut warnings), p);
    assert!(warnings.is_empty(), "{warnings:?}");
    assert_eq!(p.to_json()["anchors"]["bottom"], "arrangement-volume top 4px");
    assert!(p.set("--suhina-anchor-bottom", "arrangement-volume left").is_err(), "axis must match");
    assert!(p.set("--suhina-anchor-middle", "arrangement-volume top").is_err());
    p.set("--suhina-anchor-bottom", "none").unwrap();
    p.set("--suhina-anchor-left", "none").unwrap();
    assert!(p.anchors.is_empty());
    assert!(Placement::default().to_json().get("anchors").is_none(), "no anchors, no key");
}

#[test]
fn a_full_height_meter_ends_above_the_control_under_it_in_the_same_frame() {
    super::super::skin::configure(&anchored_skin(&[(
        "meter",
        &[("height", "100%"), ("top", "2px"), ("--suhina-anchor-bottom", "volume top 4px")],
    )]));
    let ctx = egui::Context::default();
    let header = rect(0.0, 100.0, 200.0, 60.0);
    let capture = Capture::begin(&ctx);
    let volume = place("volume", rect(150.0, 140.0, 40.0, 16.0), header);
    let meter = place("meter", rect(190.0, 100.0, 6.0, 60.0), header);
    drop(capture);
    assert_eq!(meter.top(), 102.0);
    assert_eq!(meter.bottom(), volume.top() - 4.0);
    assert_eq!(meter.width(), 6.0);
}

#[test]
fn a_target_drawn_later_comes_from_the_last_frame_moved_with_the_container() {
    super::super::skin::configure(&anchored_skin(&[(
        "meter",
        &[("height", "100%"), ("--suhina-anchor-bottom", "volume top 4px")],
    )]));
    let ctx = egui::Context::default();
    let frame = |y: f32| {
        let header = rect(0.0, y, 200.0, 60.0);
        let _capture = Capture::begin(&ctx);
        let meter = place("meter", rect(190.0, y, 6.0, 60.0), header);
        let volume = place("volume", rect(150.0, y + 40.0, 40.0, 16.0), header);
        (meter, volume)
    };
    // First frame: nothing known yet, the meter keeps its own placement.
    let (meter, _) = frame(100.0);
    assert_eq!(meter.bottom(), 160.0);
    let (meter, volume) = frame(100.0);
    assert_eq!(meter.bottom(), volume.top() - 4.0);
    // Scrolled by 25 points: the last frame's target moves with the header.
    let (meter, volume) = frame(75.0);
    assert_eq!(meter.bottom(), volume.top() - 4.0);
}

#[test]
fn hidden_missing_and_foreign_targets_fall_back_to_the_own_placement() {
    let ctx = egui::Context::default();
    let header = rect(0.0, 0.0, 200.0, 60.0);
    let native = rect(190.0, 0.0, 6.0, 60.0);
    for (volume_hidden, volume_bounds) in [(true, header), (false, rect(0.0, 300.0, 200.0, 60.0))] {
        let mut entries: Vec<(&str, &[(&str, &str)])> =
            vec![("meter", &[("height", "100%"), ("--suhina-anchor-bottom", "volume top 4px")])];
        if volume_hidden {
            entries.push(("volume", &[("display", "none")]));
        }
        super::super::skin::configure(&anchored_skin(&entries));
        // An empty frame: nothing from the previous case is remembered.
        drop(Capture::begin(&ctx));
        for _ in 0..2 {
            let _capture = Capture::begin(&ctx);
            place("volume", rect(150.0, volume_bounds.top() + 40.0, 40.0, 16.0), volume_bounds);
            assert_eq!(place("meter", native, header), native);
        }
    }
    // A target that is never drawn at all.
    super::super::skin::configure(&anchored_skin(&[("meter", &[("--suhina-anchor-bottom", "nowhere top")])]));
    let _capture = Capture::begin(&ctx);
    assert_eq!(place("meter", native, header), native);
}

#[test]
fn anchors_outside_a_frame_capture_are_inert() {
    super::super::skin::configure(&anchored_skin(&[("meter", &[("--suhina-anchor-bottom", "volume top")])]));
    let header = rect(0.0, 0.0, 200.0, 60.0);
    let native = rect(190.0, 0.0, 6.0, 20.0);
    place("volume", rect(150.0, 40.0, 40.0, 16.0), header);
    assert_eq!(place("meter", native, header), native);
}

#[test]
fn cyclic_anchors_are_dropped_with_a_warning() {
    let mut controls = anchored_skin(&[
        ("a", &[("--suhina-anchor-left", "b right")]),
        ("b", &[("--suhina-anchor-left", "a right")]),
        ("c", &[("--suhina-anchor-top", "a bottom 2px")]),
    ])
    .controls;
    let mut warnings = Vec::new();
    validate_anchors(&mut controls, &mut warnings);
    assert!(controls["a"].anchors.is_empty() && controls["b"].anchors.is_empty());
    assert!(!controls["c"].anchors.is_empty());
    assert_eq!(warnings.len(), 2, "{warnings:?}");
}

#[test]
fn semantic_names_are_stable_bounded_and_keep_distinct_non_ascii_ids() {
    assert_eq!(semantic_name("device", "enabled"), "device-enabled");
    for id in ["Ääni 🌲", "volume.1", &"large".repeat(100)] {
        let name = semantic_name("device", id);
        assert!(valid_name(&name));
        assert_eq!(name, semantic_name("device", id));
    }
    assert_ne!(semantic_name("device", "volume.1"), semantic_name("device", "volume-1"));
}

#[test]
fn placement_is_bounded_and_round_trips_without_changing_the_native_default() {
    let native = Rect::from_min_size(egui::pos2(30.0, 40.0), Vec2::new(20.0, 60.0));
    let bounds = Rect::from_min_size(egui::Pos2::ZERO, Vec2::splat(100.0));
    let mut p = Placement::default();
    assert_eq!(p.apply(native, bounds), native);
    assert!(p.set("width", "NaNpx").is_err());
    assert!(p.set("left", "2147483648px").is_err());
    assert!(p.set("--suhina-direction", "diagonal").is_err());
    assert!(p.set("width", "4096px").is_ok());
    assert!(p.set("left", "-16384px").is_ok());
    let placed = p.apply(native, bounds);
    assert!(bounds.contains_rect(placed));
    assert_eq!(placed.width(), 100.0);
    let mut warnings = Vec::new();
    assert_eq!(Placement::from_json(&p.to_json(), &mut warnings), p);
    assert!(warnings.is_empty());
}
