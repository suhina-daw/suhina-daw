// Copyright (C) 2026 The Suhina contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! The GPU side of `suhina_ui`'s providers, on egui's wgpu renderer. The app installs them in its
//! window and `suhina-theme-render` in its offscreen renders, so both draw a theme the same way.
//! Each reaches `suhina_ui` only through its `Provider` trait:
//!
//! - [`surface`]: `kit::surface`'s backgrounds, drawn unblended so a see-through theme's alpha
//!   reaches the window's pixels.
//! - [`liquid`]: `kit::liquid`'s backdrops, captured and blurred in renderers of their own.

pub mod liquid;
pub mod surface;
