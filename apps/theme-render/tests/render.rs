// Copyright (C) 2026 The Suhina contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! Renders on a GPU, so they run only when asked:
//! `cargo test -p suhina-theme-render --test main -- --ignored`.

use egui_kittest::kittest::Queryable;
use suhina_theme_render::{Gpu, Subject, render_all, rendered, scenes, shoot};

/// The see-through test theme's buttons have round artwork on a translucent toolbar. The real
/// window shows a square hole around each: the button's background replaces the toolbar's pixels
/// with its own alpha, and the artwork's corners leave them so. The render shows it as the window
/// does: around the Play button's circle, inside its square, less of the window is left than on
/// the toolbar beside it.
#[test]
#[ignore = "needs a GPU adapter; run with --ignored"]
fn the_see_through_themes_square_holes_under_artwork_show_in_its_render() {
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
    assert!(corner + 16 < toolbar, "a hole at the corner: alpha {corner} there, {toolbar} on the toolbar");
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
