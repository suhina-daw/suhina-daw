// Copyright (C) 2026 The Suhina contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! Suhina's user interface (ADR 0009): the main window and its views, drawn with egui. A thin
//! client of the session: it reads the project and the engine's monitor, and every change it
//! makes is a command (`Session::execute`), the same ones the CLI and MCP run.
//!
//! # What is on screen, in code
//!
//! Where each part of the window is drawn, and where what it does is handled (#768 › 4). A view
//! hands back [`Action`](kit::widgets::Action)s; [`App::do_action`] runs them. CI builds this
//! documentation with warnings as errors, so a link here fails when what it names is renamed.
//!
//! | On screen | Drawn by | Handled by |
//! |---|---|---|
//! | The menu bar | [`App::menu_bar`] | [`App::do_action`] |
//! | The control bar (transport, grid, CPU, view switch) | [`App::control_bar`] | [`App::do_action`] |
//! | The status bar | [`App::status_bar`] | — |
//! | Project tabs and Ctrl+Tab | [`App::tab_row`], [`App::tab_switcher_ui`] | [`App::do_action`] |
//! | The Session view's slots and scenes | [`session::session_view::show`] | [`App::do_action`] |
//! | A slot's right-click menu | [`interaction::menus::slot_menu`] | [`App::do_action`] |
//! | A scene's right-click menu | [`interaction::menus::scene_menu`] | [`App::do_action`] |
//! | A channel strip (the Session view's mixer) | [`session::mixer_strip::show`] | [`App::do_action`] |
//! | The Arrangement's lanes, ruler, markers and loop brace | [`arrangement::arrangement_view::show`] | [`App::do_action`] |
//! | An arrangement clip's right-click menu | [`interaction::menus::clip_menu`] | [`App::do_action`] |
//! | A track header and its right-click menu | [`interaction::menus::track_menu`] | [`App::do_action`] |
//! | Automation lanes | [`arrangement::automation_lanes::show`] | [`App::do_action`] |
//! | The detail area (devices, clip properties) | [`App::detail_area`], [`detail::detail_view::show_row`] | [`App::do_action`] |
//! | A device's right-click menu | [`interaction::menus::device_menu`] | [`App::do_action`] |
//! | The piano roll | [`detail::piano_roll::show`] | its edits as `note.*` actions, [`App::do_action`] |
//! | The MIDI tools beside the piano roll | [`detail::midi_tools::show`] | [`App::do_action`] |
//! | The browser | [`browser::browser_view::show`], [`App::browser_panel`] | [`App::do_action`] |
//! | The sample map | [`browser::sample_map::show`], [`App::sample_map_window`] | [`App::do_action`] |
//! | Settings | [`settings::settings_view::show`], [`App::settings_dialog`] | [`App::settings_page`] |
//! | Keys and shortcuts | — | [`App::shortcuts`], launch keys in [`session::launch_keys::LaunchKeys::handle`] |
//! | Files and items dropped on the window | — | [`App::dropped_files`], [`App::drop_carried`] |

pub mod app;
pub mod arrangement;
pub mod browser;
pub mod demo;
pub mod detail;
pub mod interaction;
pub mod kit;
pub mod session;
pub mod settings;

pub use app::{App, NewProject, OpenAs, View};
pub use interaction::status::Status;
