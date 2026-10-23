// Copyright (C) 2026 The Suhina contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! The windows plug-ins' editors live in: plain top-level windows on the host's main thread,
//! which the plug-ins' own views are attached to. Several may be open at once, one per
//! instance whose editor is open; what happened to each (asked to close, its size) is kept by
//! its id in a list on the main thread.
//!
//! Windows (Win32) and Linux (X11, through `x11rb`); macOS (AppKit, through `objc2-app-kit`):
//! one `NSWindow` per open editor, its content view the plug-in's view attaches into, the
//! application an accessory (no Dock icon of its own) whose events the main loop pumps.
//! Cross-process embedding does not exist there (a view of the host's process cannot attach
//! to another process's window), so an embedded editor opens in a window of its own instead.
//! On any other system opening one is an error for now (ADR 0006).
//!
//! An editor's window is owned by the engine's window when the engine names it
//! ([`set_owner`]): it floats above Suhina, has no taskbar entry of its own and is minimised
//! with it (Win32 owner window; X11 `WM_TRANSIENT_FOR`). macOS has no cross-process window
//! ownership either, so there the editor is a plain window of the host's process.
//!
//! An embedded editor ([`Window::open_embedded`]) is instead a child inside the engine's
//! window, where its device panel shows it: two windows, a *clip* the size of the panel's
//! visible area (what falls outside it is cut off, so a scrolled editor does not paint over the
//! rest of Suhina) and inside it the *view* the plug-in draws in, at the editor's full size.
//! The engine says where both go ([`place`]); until it does, the editor is hidden.

use std::cell::Cell;

thread_local! {
    /// Space presses caught in the editor windows since the host last sent them on.
    static TRANSPORT_KEYS: Cell<u32> = const { Cell::new(0) };
}

/// How many times Space was pressed in an editor window since the last call. The key is kept
/// from the plug-in: Space plays and stops the engine in every DAW, from its plug-in windows too.
pub(crate) fn take_transport_keys() -> u32 {
    TRANSPORT_KEYS.with(|k| k.replace(0))
}

#[cfg(any(windows, target_os = "macos"))]
fn caught_space(repeat: bool) {
    if !repeat {
        TRANSPORT_KEYS.with(|k| k.set(k.get().saturating_add(1)));
    }
}

pub(crate) use suhina_plugins::Placement;

thread_local! {
    static OWNER: Cell<Option<u64>> = const { Cell::new(None) };
}

/// The engine's window that the next editor window opened on this thread belongs to (a Win32
/// `HWND`, an X11 window id), or `None` for a window of its own.
pub(crate) fn set_owner(owner: Option<u64>) {
    OWNER.with(|o| o.set(owner));
}

/// The engine's window that the next editor window opened on this thread belongs to, as
/// [`set_owner`] set it: read only where a window can be opened (Windows, X11).
#[cfg(any(windows, target_os = "linux"))]
fn owner() -> Option<u64> {
    OWNER.with(Cell::get)
}

/// The size of a window's inside, in pixels.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct Size {
    pub width: u32,
    pub height: u32,
}

/// Names one open window, for the code that has no hold of the [`Window`] itself (a plug-in's
/// frame asking for a new size).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct WindowId(usize);

#[cfg(windows)]
#[path = "window/windows.rs"]
mod imp;

#[cfg(target_os = "linux")]
#[path = "window/x11.rs"]
mod imp;

#[cfg(not(any(windows, target_os = "linux")))]
#[path = "window/macos.rs"]
mod imp;

#[cfg(not(windows))]
pub(crate) use imp::any_open;
#[cfg(windows)]
pub(crate) use imp::{Waker, wait};
pub(crate) use imp::{Window, client_size, place, pump, resize, take_close_request};

/// Where an editor opens: a window of its own (owned by the engine's, when it named one with
/// [`set_owner`]), or embedded inside the engine's window `parent`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Mode {
    Floating,
    Embedded { parent: u64 },
}

impl Window {
    /// Opens a window for an editor of inside `size` as `mode` says: floating, titled `title`
    /// and resizable by the user when `resizable`; or embedded and hidden until placed.
    pub(crate) fn open_as(mode: Mode, title: &str, size: Size, resizable: bool) -> Result<Self, String> {
        #[cfg(windows)]
        imp::interactive();
        match mode {
            Mode::Floating => Self::open(title, size, resizable),
            Mode::Embedded { parent } => Self::open_embedded(parent, size),
        }
    }
}
