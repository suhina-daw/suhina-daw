// Copyright (C) 2026 The Suhina contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! The building blocks every view draws with: the theme, widgets, meters, motion, the logo,
//! the scroll bar of a self-drawn list and waveforms. Nothing here knows about a view.

pub mod artwork;
pub mod artwork_png;
pub mod control_layout;
pub mod frame_trace;
pub mod gradient;
pub mod liquid;
pub mod logo;
pub mod meter;
pub mod mixer_controls;
pub mod motion;
pub mod native_css;
pub mod scroll;
pub mod session_geometry;
pub mod skin;
pub mod style_class;
pub mod surface;
pub mod theme;
pub mod theme_css;
pub mod theme_preview;
pub mod theme_vars;
pub mod waveform;
pub mod widgets;
