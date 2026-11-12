// Copyright (C) 2026 The Suhina contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! What the views share: dragging and dropping, picking a value from a list, the context menus,
//! groups inside groups, the grid of tracks and slots, the track colour picker, the timelines'
//! grid and how far they scroll, the grid menu, the toolbars' icon buttons and a MIDI key's name.

pub mod colour_picker;
pub mod commands;
pub mod device_at;
pub mod drop;
pub mod empty_space;
pub mod grid;
pub mod grid_menu;
pub mod key_name;
pub mod menus;
pub mod nesting;
pub mod pick;
pub mod scroll;
pub mod status;
pub mod timeline_grid;
pub mod toolbar;
