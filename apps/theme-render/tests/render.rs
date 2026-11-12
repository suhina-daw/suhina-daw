// Copyright (C) 2026 The Suhina contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! Renders on a GPU, so they run only when asked:
//! `cargo test -p suhina-theme-render --test main -- --ignored`.

use egui_kittest::kittest::Queryable;
use suhina_theme_render::{Gpu, Subject, render_all, rendered, scenes, shoot};

/// The see-through test theme's buttons have round artwork on a translucent toolbar. Its button
/// background used to replace the toolbar's pixels with its own alpha, leaving a square hole
/// around the circle; every background now blends (#1233), so around the Play button's circle,
/// inside its square, as much of the window is left as on the toolbar beside it.
#[test]
#[ignore = "needs a GPU adapter; run with --ignored"]
fn the_see_through_theme_has_no_holes_under_artwork() {
    let theme = super::see_through().unwrap();
    let scene = scenes::find("launcher").expect("the launcher scene");
    let mut h = rendered((scene.app)().unwrap(), &theme, scene.width, 1.0, &Gpu::new().unwrap());
    (scene.drive)(&mut h).unwrap_or_else(|e| panic!("{e}"));
    let play = h.get_by_label("Play").rect();
    let shot = shoot(&mut h).unwrap_or_else(|e| panic!("{e}"));
    assert!(shot.see_through, "the theme makes a see-through window");
    let alpha = |x: f32, y: f32| shot.pixels.get_pixel(x.round() as u32, y.round() as u32)[3];
    // A corner of the button's square, outside its round artwork; and the toolbar just above it.
    let corner = alpha(play.left() + 1.5, play.top() + 1.5);
    let toolbar = alpha(play.center().x, play.top() - 3.0);
    assert!(corner + 4 >= toolbar, "no hole at the corner: alpha {corner} there, {toolbar} on the toolbar");
}

/// Scrolled content never shows through the headers that stay where they are: in the see-through
/// test theme, the launcher's Main header (over the scene column) and the Arrangement view's
/// header corner (over the track headers) are the same, pixel for pixel, scrolled or not.
#[test]
#[ignore = "needs a GPU adapter; run with --ignored"]
fn nothing_scrolled_shows_through_a_frozen_header() {
    let theme = super::see_through().unwrap();
    let gpu = Gpu::new().unwrap();
    for (scene, header) in [("launcher-scrolled", "Select Main"), ("arrangement-scrolled", "Hide returns")] {
        let scene = scenes::find(scene).expect("the scene");
        let mut h = rendered((scene.app)().unwrap(), &theme, scene.width, 1.0, &gpu);
        if scene.name == "arrangement-scrolled" {
            h.get_by_label("Arrangement view").click();
            h.run_steps(4);
        }
        let rect = h.query_all_by_label(header).next().unwrap_or_else(|| panic!("no {header}")).rect();
        let pixels = |shot: &suhina_theme_render::Shot| -> Vec<[u8; 4]> {
            let (x, y) = (rect.left().ceil() as u32, rect.top().ceil() as u32);
            let (w, h) = (rect.width().floor() as u32, rect.height().floor() as u32);
            (y..y + h)
                .flat_map(|y| (x..x + w).map(move |x| (x, y)))
                .map(|(x, y)| shot.pixels.get_pixel(x, y).0)
                .collect()
        };
        let before = pixels(&shoot(&mut h).unwrap_or_else(|e| panic!("{e}")));
        (scene.drive)(&mut h).unwrap_or_else(|e| panic!("{e}"));
        let moved = h.query_all_by_label(header).next().map(|n| n.rect());
        assert_eq!(moved, Some(rect), "{}: {header} stays where it is", scene.name);
        let after = pixels(&shoot(&mut h).unwrap_or_else(|e| panic!("{e}")));
        let changed =
            before.iter().zip(&after).filter(|(a, b)| a.iter().zip(b.iter()).any(|(a, b)| a.abs_diff(*b) > 2)).count();
        assert_eq!(changed, 0, "{}: {changed} pixels of {header} changed when the view scrolled", scene.name);
    }
}

/// Every bundled theme renders in every scene, to `target/theme-shots/<theme>/`.
#[test]
#[ignore = "needs a GPU adapter; run with --ignored"]
fn every_bundled_theme_renders_in_every_scene() {
    let out = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../target/theme-shots");
    let backdrop = suhina_theme_render::composite::default_backdrop();
    let mut themes = Subject::built_ins();
    themes.push(super::see_through().unwrap());
    let scenes = scenes::scenes().len();
    for theme in &themes {
        let done = render_all(theme, &backdrop, &out.join(&theme.id), 1.0, None).unwrap_or_else(|e| panic!("{e}"));
        assert!(done.failed.is_empty(), "{}: {:?}", theme.id, done.failed);
        assert_eq!(done.written.len(), 2 * scenes, "{}: two images a scene", theme.id);
    }
}
