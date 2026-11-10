// Copyright (C) 2026 The Suhina contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! The GPU side of `suhina_ui`'s providers, on egui's wgpu renderer. The app installs them in its
//! window and `suhina-theme-render` in its offscreen renders, so both draw a theme the same way.
//! Each reaches `suhina_ui` only through its `Provider` trait:
//!
//! - [`surface`]: `kit::surface`'s clears, drawn unblended so the pixels under a backdrop are
//!   cleared before it is drawn.
//! - [`liquid`]: `kit::liquid`'s backdrops, captured and blurred in renderers of their own.

pub mod liquid;
pub mod surface;
