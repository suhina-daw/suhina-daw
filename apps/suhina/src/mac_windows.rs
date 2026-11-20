// Copyright (C) 2026 The Suhina contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! The app's windows of their own on macOS (Settings, the track color window, the sample map)
//! while the system resizes them. The system resizes a window as soon as the app asks, and the
//! frame drawn at the new size comes a frame later. Meanwhile the Metal layer the window is drawn
//! in shows its last frame, stretched to its new size by default (`kCAGravityResize`), which
//! flickered as the color window opened Fine adjustment. Each such layer instead keeps its last
//! frame at the window's top left, which is the layer's `bottomLeft` gravity: with `topLeft`, a
//! recording showed the last frame held at the bottom of a window that had grown. The main window keeps the default, which hides the system's lag while
//! it is dragged to a new size. Only safe AppKit calls are used.

use std::ffi::c_void;

use objc2::rc::Retained;
use objc2::runtime::NSObjectProtocol;
use objc2::{MainThreadMarker, Message};
use objc2_app_kit::NSApplication;
use objc2_foundation::{NSArray, NSObjectNSKeyValueCoding, NSString};
use objc2_quartz_core::CALayer;

/// The contents gravity the windows of their own take: their top left on screen.
const PINNED: &str = "bottomLeft";

/// Pins the Metal layers of every window but the main one, whose content view is `main`, to the
/// window's top left. Called every frame: a window opened since gets it before it is first resized.
pub fn pin_frames_top_left(main: *const c_void) {
    let Some(thread) = MainThreadMarker::new() else { return };
    for window in NSApplication::sharedApplication(thread).windows().iter() {
        let Some(view) = window.contentView() else { continue };
        if Retained::as_ptr(&view).cast::<c_void>() == main {
            continue;
        }
        let Some(root) = view.layer() else { continue };
        for layer in metal_layers(&root) {
            if pins(layer.contentsGravity().to_string().as_str()) {
                layer.setContentsGravity(&NSString::from_str(PINNED));
            }
        }
    }
}

/// The Metal layers in `root`: the layer itself or one of its sublayers, where wgpu draws.
fn metal_layers(root: &CALayer) -> Vec<Retained<CALayer>> {
    let mut found = Vec::new();
    if is_metal(root) {
        found.push(root.retain());
    }
    let sublayers =
        root.valueForKeyPath(&NSString::from_str("sublayers")).and_then(|value| value.downcast::<NSArray>().ok());
    for layer in sublayers.iter().flat_map(|array| array.iter()) {
        if let Ok(layer) = layer.downcast::<CALayer>()
            && is_metal(&layer)
        {
            found.push(layer);
        }
    }
    found
}

/// Whether `layer` is a Metal layer: wgpu's is a subclass of `CAMetalLayer` (`RawWindowMetalLayer`).
fn is_metal(layer: &CALayer) -> bool {
    objc2::runtime::AnyClass::get(c"CAMetalLayer").is_some_and(|metal| layer.isKindOfClass(metal))
}

/// Whether a layer whose contents gravity is `gravity` is pinned: not twice.
fn pins(gravity: &str) -> bool {
    gravity != PINNED
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A layer is pinned once: one already pinned is left alone.
    #[test]
    fn only_a_layer_not_yet_pinned_is_pinned() {
        assert!(pins("resize"));
        assert!(pins("topLeft"));
        assert!(!pins(PINNED));
    }
}
