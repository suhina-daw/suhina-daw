// Copyright (C) 2026 The Suhina contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! The keyboard between the app's window and the plug-in editors embedded in it from the host
//! process (ADR 0006). A click in an embedded editor gives it the keyboard, as it should (the
//! plug-in's own keys), but Windows leaves it there when the window's own controls are clicked
//! afterwards: Delete, Ctrl+X and the rest then went to the plug-in.

/// Takes the keyboard back to `window` (the app's own `HWND`) when another window, such as an
/// editor embedded in it, has it. For a press on the window's own controls; does nothing
/// elsewhere than on Windows.
pub fn take_keyboard_back(window: u64) {
    #[cfg(windows)]
    if let Ok(window) = usize::try_from(window) {
        use windows_sys::Win32::UI::Input::KeyboardAndMouse::{GetFocus, SetFocus};
        let own = window as windows_sys::Win32::Foundation::HWND;
        // SAFETY: a plain Win32 call from the app's window thread; it takes no pointers.
        let focus = unsafe { GetFocus() };
        if focus != own {
            // SAFETY: the app's own window handle, from its window thread; a handle that is no
            // window makes the call fail, nothing else.
            unsafe { SetFocus(own) };
        }
    }
    #[cfg(not(windows))]
    let _ = window;
}

/// Whether the app's computer keyboard plays notes (#944): set by the app; a host learns it
/// from the blocks it gets, and while it is on hands the note keys pressed in its editors back
/// to the app instead of to the plug-in (unless a text field there has the keyboard).
static KEYS_TO_ENGINE: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);

/// The app: says whether its computer keyboard plays notes.
pub fn set_keys_to_engine(on: bool) {
    KEYS_TO_ENGINE.store(on, std::sync::atomic::Ordering::Relaxed);
}

/// Whether the app's computer keyboard plays notes, as this process last heard. Real-time safe.
pub fn keys_to_engine() -> bool {
    KEYS_TO_ENGINE.load(std::sync::atomic::Ordering::Relaxed)
}

pub(crate) fn heard_keys_to_engine(on: bool) {
    KEYS_TO_ENGINE.store(on, std::sync::atomic::Ordering::Relaxed);
}

/// Where the mouse pointer is on the screen, in physical pixels: for a drag of files from the
/// system, during which Windows sends the window no pointer moves. `None` elsewhere than on
/// Windows, or when it cannot be read.
pub fn pointer_on_screen() -> Option<(i32, i32)> {
    #[cfg(windows)]
    {
        let mut point = windows_sys::Win32::Foundation::POINT { x: 0, y: 0 };
        // SAFETY: the call fills the point it is given, a local of the right type.
        let read = unsafe { windows_sys::Win32::UI::WindowsAndMessaging::GetCursorPos(&mut point) };
        (read != 0).then_some((point.x, point.y))
    }
    #[cfg(not(windows))]
    None
}
