// Copyright (C) 2026 The Suhina contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! The app's windows of their own on macOS (Settings, the track color window, the sample map)
//! while the system resizes them. The system resizes a window as soon as the app asks, and the
//! frame drawn at the new size comes a frame later. Meanwhile the Metal layer the window is drawn
//! in shows its last frame, stretched to its new size by default (`kCAGravityResize`), which
//! flickered as the color window grew to a taller view. Each such layer instead keeps its last
//! frame at the window's top left, which is the layer's `bottomLeft` gravity: with `topLeft`, a
//! recording showed the last frame held at the bottom of a window that had grown. The main window keeps the default, which hides the system's lag while
//! it is dragged to a new size. Only safe AppKit calls are used.

use std::ffi::c_void;
use std::sync::OnceLock;

use objc2::MainThreadMarker;
use objc2::rc::Retained;
use objc2::runtime::{AnyClass, NSObjectProtocol};
use objc2_app_kit::NSApplication;
use objc2_foundation::{NSArray, NSObjectNSKeyValueCoding, NSString, ns_string};
use objc2_quartz_core::CALayer;

/// The contents gravity the windows of their own take: their top left on screen.
fn pinned() -> &'static NSString {
    ns_string!("bottomLeft")
}

/// Pins the Metal layers of every window but the main one, whose content view is `main`, to the
/// window's top left. Called every frame: a window opened since gets it before it is first resized.
/// It allocates nothing on the way: the strings are static and the class is looked up once.
pub fn pin_frames_top_left(main: *const c_void) {
    let Some(thread) = MainThreadMarker::new() else { return };
    let Some(metal) = metal_class() else { return };
    for window in NSApplication::sharedApplication(thread).windows().iter() {
        let Some(view) = window.contentView() else { continue };
        if Retained::as_ptr(&view).cast::<c_void>() == main {
            continue;
        }
        let Some(root) = view.layer() else { continue };
        each_metal_layer(&root, metal, |layer| {
            if pins(&layer.contentsGravity()) {
                layer.setContentsGravity(pinned());
            }
        });
    }
}

/// `CAMetalLayer`, looked up once.
fn metal_class() -> Option<&'static AnyClass> {
    static METAL: OnceLock<Option<&'static AnyClass>> = OnceLock::new();
    *METAL.get_or_init(|| AnyClass::get(c"CAMetalLayer"))
}

/// Runs `pin` on each Metal layer in `root`, a subclass of `metal`: the layer itself or one of its
/// sublayers, where wgpu draws (`RawWindowMetalLayer`). `sublayers` is read by key-value coding,
/// as `CALayer::sublayers` is not a safe call.
fn each_metal_layer(root: &CALayer, metal: &AnyClass, mut pin: impl FnMut(&CALayer)) {
    if root.isKindOfClass(metal) {
        pin(root);
    }
    let Some(sublayers) = root.valueForKey(ns_string!("sublayers")) else { return };
    let Ok(sublayers) = sublayers.downcast::<NSArray>() else { return };
    for layer in sublayers.iter() {
        if let Ok(layer) = layer.downcast::<CALayer>()
            && layer.isKindOfClass(metal)
        {
            pin(&layer);
        }
    }
}

/// Whether a layer whose contents gravity is `gravity` is pinned: not twice.
fn pins(gravity: &NSString) -> bool {
    !gravity.isEqualToString(pinned())
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A layer is pinned once: one already pinned is left alone.
    #[test]
    fn only_a_layer_not_yet_pinned_is_pinned() {
        assert!(pins(ns_string!("resize")));
        assert!(pins(ns_string!("topLeft")));
        assert!(!pins(pinned()));
        // Equal by its characters, not by being the same object.
        assert!(!pins(&NSString::from_str("bottomLeft")));
    }
}
