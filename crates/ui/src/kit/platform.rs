// Copyright (C) 2026 The Suhina contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! The system the window looks and behaves like: how shortcuts are written (⌘C or Ctrl+C), what
//! the file manager is called, which plug-in formats Settings names, and whether Suhina draws its
//! own title bar. It is the system Suhina was built for unless something sets another one, as the
//! theme renders do, so that one recording of what each theme paints holds on every system.
//!
//! It is the window's thread's own, as the theme's state is ([`crate::kit::theme`]): a window
//! drawn on a thread of its own starts with the host's system, and setting it there leaves every
//! other window alone.

use std::cell::Cell;

/// A system the window can look like.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Platform {
    MacOs,
    Windows,
    /// Linux, and any other system with no look of its own.
    Linux,
}

impl Platform {
    /// The system Suhina was built for.
    pub const HOST: Platform = if cfg!(target_os = "macos") {
        Platform::MacOs
    } else if cfg!(target_os = "windows") {
        Platform::Windows
    } else {
        Platform::Linux
    };

    /// Whether shortcuts are written with the system's signs (⌘C, ⇧⌘D) rather than as
    /// `Ctrl+C`.
    pub fn writes_key_signs(self) -> bool {
        self == Platform::MacOs
    }

    /// Whether the system leaves the title bar to the window, so Suhina draws its own (the
    /// application bar) with the window's buttons.
    pub fn draws_own_title_bar(self) -> bool {
        self == Platform::Windows
    }

    /// The menu item that shows a file in the system's file manager.
    pub fn show_in_folder_label(self) -> &'static str {
        match self {
            Platform::Windows => "Show in Explorer",
            Platform::MacOs => "Show in Finder",
            Platform::Linux => "Show in folder",
        }
    }
}

thread_local! {
    static CURRENT: Cell<Platform> = const { Cell::new(Platform::HOST) };
}

/// The system this thread's window looks like: [`Platform::HOST`] unless [`set`] said another.
pub fn current() -> Platform {
    CURRENT.with(Cell::get)
}

/// Makes this thread's window look like `platform` from the next frame on. For the theme
/// renders and tests; the app itself leaves it at the host's system.
pub fn set(platform: Platform) {
    CURRENT.with(|current| current.set(platform));
}

/// Runs `f` with this thread's window looking like `platform`, then puts back the system it
/// looked like before, also when `f` panics: a test that checks one system leaves the next test
/// on the same thread as it found it.
pub fn with<T>(platform: Platform, f: impl FnOnce() -> T) -> T {
    struct Restore(Platform);
    impl Drop for Restore {
        fn drop(&mut self) {
            set(self.0);
        }
    }
    let _restore = Restore(current());
    set(platform);
    f()
}

#[cfg(test)]
#[path = "platform_tests.rs"]
mod tests;
