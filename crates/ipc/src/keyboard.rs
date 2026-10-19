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
