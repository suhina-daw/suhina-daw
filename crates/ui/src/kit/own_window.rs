// Copyright (C) 2026 The Suhina contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! A window of its own: Settings, the track color window or the sample map in a system window of
//! their own. Each is made like the main window. It is see-through where the main window is, so
//! the desktop shows through a see-through theme there too. Where the main window's content runs
//! under the system's title bar (macOS), so does its content. Its base is `window`'s background,
//! as under the main window. The element that fills it (`settings`, `dialog[name="color-picker"]`)
//! is its root: [`super::element::Entered::paint_root`].

use egui::{CornerRadius, Rect, Ui, ViewportBuilder};

/// What the system draws of the app's windows, which a window of its own takes after the main
/// window.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Frame {
    /// The main window was asked to be see-through (the GPU renderer), so its windows of their
    /// own are too: without that they open opaque, as the main window does.
    pub see_through: bool,
    /// How tall the system's title bar over the top of the content is: 0 where the content
    /// starts under it.
    pub title_bar: f32,
    /// The radius of the window's corners, where the window does not clip to them.
    pub corner: u8,
}

/// `builder` for a window of its own, made as the main window is: see-through, its content under
/// the system's title bar where [`Frame::title_bar`] says so.
pub fn builder(builder: ViewportBuilder, frame: Frame) -> ViewportBuilder {
    // Every window clears to see-through under a see-through theme (`App::clear_color` is the
    // app's, not the window's): one that is not see-through shows the system's own window color
    // there. Where the main window is not asked to be see-through (the OpenGL renderer), nor are
    // these.
    let builder = builder.with_transparent(frame.see_through);
    if frame.title_bar > 0.0 { builder.with_fullsize_content_view(true).with_titlebar_shown(false) } else { builder }
}

/// What [`base`] painted.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Base {
    /// The whole window.
    pub whole: Rect,
    /// The window's corners, as its root paints them.
    pub corners: CornerRadius,
    /// How tall the system's title bar over the top of it is.
    pub title_bar: f32,
}

impl Base {
    /// The window under the system's title bar.
    pub fn under_title_bar(&self) -> Rect {
        Rect::from_min_max(egui::pos2(self.whole.left(), self.whole.top() + self.title_bar), self.whole.max)
    }
}

/// Paints the base of a window of its own over all of `ui`: `window`'s background color and
/// gradients, in the window's corners. Full screen, it has neither corners nor a title bar.
pub fn base(ui: &Ui, frame: Frame) -> Base {
    let whole = ui.max_rect();
    let (title_bar, corners) = shown(ui, frame);
    let window = super::element::WINDOW.probe();
    ui.painter().rect_filled(whole, corners, window.background());
    if let Some(style) = window.style() {
        let mut style = style.clone();
        style.authored.radius = false;
        super::native_css::gradients_rounded(ui.painter(), whole, corners.average(), style);
    }
    Base { whole, corners, title_bar }
}

/// How tall the title bar over the window `ui` draws in is, and the window's corners, as the
/// system draws them now: full screen, it has neither.
pub fn shown(ui: &Ui, frame: Frame) -> (f32, CornerRadius) {
    if ui.input(|i| i.viewport().fullscreen.unwrap_or(false)) {
        (0.0, CornerRadius::ZERO)
    } else {
        (frame.title_bar, CornerRadius::same(frame.corner))
    }
}

#[cfg(test)]
#[path = "own_window_tests.rs"]
mod tests;
