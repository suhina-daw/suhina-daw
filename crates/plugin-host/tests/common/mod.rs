// Copyright (C) 2026 The Suhina contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! A window standing in for the engine's (the desktop app's main window) in tests: plug-in
//! editors are owned by it or embedded in it (ADR 0006, decision 10). On Windows it lives on
//! a thread that serves its messages, as the app's window does: a host making a window owned
//! by it or a child of it waits for that thread. On X11 it is a plain window on a connection
//! of the test's own.
#![allow(dead_code, unused_imports, clippy::panic, clippy::unwrap_used)] // test helpers outside #[test] functions

/// Whether editors can open here: on Windows, and on Linux with an X server.
pub fn editors_open_here() -> bool {
    cfg!(windows)
        || cfg!(target_os = "macos")
        || (cfg!(target_os = "linux") && std::env::var_os("DISPLAY").is_some_and(|d| !d.is_empty()))
}

/// Whether an editor can open EMBEDDED in this window: on macOS a view of the host's process
/// cannot attach to another process's window, so there it always falls back to a window of
/// its own.
pub fn embeds_editors_here() -> bool {
    cfg!(windows) || (cfg!(target_os = "linux") && std::env::var_os("DISPLAY").is_some_and(|d| !d.is_empty()))
}

/// Asks the editor's window to close, as the user does with its close button (an
/// editor's own window, floating or not, whatever plug-in format opened it).
#[cfg(windows)]
pub fn close_editor_window(process_id: u32) {
    use windows_sys::Win32::UI::WindowsAndMessaging::{PostMessageW, WM_CLOSE};
    let window = editor_window(process_id);
    assert!(!window.is_null(), "no editor window for host {process_id}");
    // SAFETY: a window found just now; WM_CLOSE carries no pointers.
    assert_ne!(unsafe { PostMessageW(window, WM_CLOSE, 0, 0) }, 0);
}

#[cfg(windows)]
pub fn editor_window(process_id: u32) -> windows_sys::Win32::Foundation::HWND {
    use windows_sys::Win32::UI::WindowsAndMessaging::{FindWindowExW, GetWindowThreadProcessId};
    let class: Vec<u16> = "SuhinaPluginEditor".encode_utf16().chain([0]).collect();
    let mut window = std::ptr::null_mut();
    loop {
        // SAFETY: the class name outlives the call; null parent enumerates top-level windows,
        // and window is null or the previous result. No pointers are retained by Windows.
        window = unsafe { FindWindowExW(std::ptr::null_mut(), window, class.as_ptr(), std::ptr::null()) };
        if window.is_null() {
            return window;
        }
        let mut owner = 0;
        // SAFETY: Windows validates the handle; owner is a writable u32 for the result.
        let thread = unsafe { GetWindowThreadProcessId(window, &raw mut owner) };
        if thread != 0 && owner == process_id {
            return window;
        }
    }
}

/// Asks the editor's window to close, as the user does with its close button: the window
/// manager's WM_DELETE_WINDOW message, sent to the newest window whose class is the editor's
/// (the root's children come bottom to top).
#[cfg(target_os = "linux")]
pub fn close_editor_window(_process_id: u32) {
    use x11rb::connection::Connection;
    use x11rb::protocol::xproto::{AtomEnum, ClientMessageEvent, ConnectionExt, EventMask};
    let (connection, screen) = x11rb::connect(None).unwrap();
    let root = connection.setup().roots[screen].root;
    let atom = |name: &[u8]| connection.intern_atom(false, name).unwrap().reply().unwrap().atom;
    let (protocols, delete) = (atom(b"WM_PROTOCOLS"), atom(b"WM_DELETE_WINDOW"));
    let children = connection.query_tree(root).unwrap().reply().unwrap().children;
    let window = children
        .into_iter()
        .rfind(|w| {
            let class = connection.get_property(false, *w, AtomEnum::WM_CLASS, AtomEnum::STRING, 0, 64);
            class.ok().and_then(|c| c.reply().ok()).is_some_and(|c| c.value.starts_with(b"suhina-plugin-editor"))
        })
        .unwrap_or_else(|| panic!("no editor window"));
    let message = ClientMessageEvent::new(32, window, protocols, [delete, 0, 0, 0, 0]);
    connection.send_event(false, window, EventMask::NO_EVENT, message).unwrap();
    // A round trip before the connection closes: closed straight after the request, the
    // server could drop the message unsent, and the test waited for a close that never
    // came (#237). Xlib's XCloseDisplay does the same (XSync).
    connection.get_input_focus().unwrap().reply().unwrap();
}

#[cfg(not(any(windows, target_os = "linux")))]
pub fn close_editor_window(_process_id: u32) {}

/// Whether the test can close the host's window from here, as the user would: another
/// process's window cannot be reached on macOS.
pub fn user_closes_windows() -> bool {
    cfg!(windows) || (cfg!(target_os = "linux") && std::env::var_os("DISPLAY").is_some_and(|d| !d.is_empty()))
}
#[cfg(windows)]
mod imp {
    use windows_sys::Win32::UI::WindowsAndMessaging::{
        CreateWindowExW, DestroyWindow, DispatchMessageW, FindWindowExW, GW_OWNER, GWL_STYLE, GetMessageW, GetWindow,
        GetWindowLongW, MSG, PostThreadMessageW, WM_QUIT, WS_OVERLAPPEDWINDOW,
    };

    pub struct ParentWindow {
        handle: usize,
        thread: u32,
        served: Option<std::thread::JoinHandle<()>>,
    }

    impl ParentWindow {
        pub fn open() -> Self {
            let (sender, receiver) = std::sync::mpsc::channel();
            let served = std::thread::spawn(move || {
                let class: Vec<u16> = "STATIC".encode_utf16().chain([0]).collect();
                // SAFETY: a system window class and no title, parent, menu or creation data.
                let window = unsafe {
                    CreateWindowExW(
                        0,
                        class.as_ptr(),
                        std::ptr::null(),
                        WS_OVERLAPPEDWINDOW,
                        0,
                        0,
                        400,
                        300,
                        std::ptr::null_mut(),
                        std::ptr::null_mut(),
                        std::ptr::null_mut(),
                        std::ptr::null(),
                    )
                };
                // SAFETY: no arguments.
                let thread = unsafe { windows_sys::Win32::System::Threading::GetCurrentThreadId() };
                sender.send((window as usize, thread)).unwrap();
                // SAFETY: a zeroed message is a valid value to be filled.
                let mut message: MSG = unsafe { std::mem::zeroed() };
                // SAFETY: any message of this thread, until WM_QUIT.
                while unsafe { GetMessageW(&mut message, std::ptr::null_mut(), 0, 0) } > 0 {
                    // SAFETY: a message just taken from the queue.
                    unsafe { DispatchMessageW(&message) };
                }
                // SAFETY: the window made above, on this thread.
                unsafe { DestroyWindow(window as _) };
            });
            let (handle, thread) = receiver.recv().unwrap();
            assert_ne!(handle, 0, "no parent window");
            Self { handle, thread, served: Some(served) }
        }

        /// The native handle, as the engine names its window.
        pub fn id(&self) -> u64 {
            self.handle as u64
        }

        /// How many editor windows (the host's class) are children of this window.
        /// The embedded editors shown over this window: on Windows a frameless window of the
        /// editor's class that this window owns (not a child, which the engine's
        /// DirectComposition drawing would cover).
        pub fn editor_children(&self) -> usize {
            let class: Vec<u16> = "SuhinaPluginEditor".encode_utf16().chain([0]).collect();
            let mut count = 0;
            let mut after = std::ptr::null_mut();
            loop {
                // SAFETY: a class name that outlives the call; the previous top-level window
                // found, or none.
                let window = unsafe { FindWindowExW(std::ptr::null_mut(), after, class.as_ptr(), std::ptr::null()) };
                if window.is_null() {
                    return count;
                }
                // SAFETY: a window just found; the calls only read its owner and its style. An
                // editor in a window of its own has a caption; an embedded one has none.
                let owned = unsafe { GetWindow(window, GW_OWNER) } == self.handle as _;
                // SAFETY: as above.
                let framed = unsafe { GetWindowLongW(window, GWL_STYLE) } as u32
                    & windows_sys::Win32::UI::WindowsAndMessaging::WS_CAPTION
                    != 0;
                if owned && !framed {
                    count += 1;
                }
                after = window;
            }
        }
    }

    impl Drop for ParentWindow {
        fn drop(&mut self) {
            // SAFETY: a message without pointers to the window's thread.
            unsafe { PostThreadMessageW(self.thread, WM_QUIT, 0, 0) };
            if let Some(served) = self.served.take() {
                let _ = served.join();
            }
        }
    }
}

#[cfg(target_os = "linux")]
mod imp {
    use x11rb::connection::Connection;
    use x11rb::protocol::xproto::{AtomEnum, ConnectionExt, CreateWindowAux, WindowClass};
    use x11rb::rust_connection::RustConnection;

    pub struct ParentWindow {
        connection: RustConnection,
        window: u32,
    }

    impl ParentWindow {
        pub fn open() -> Self {
            let (connection, screen) = x11rb::connect(None).unwrap();
            let root = &connection.setup().roots[screen];
            let window = connection.generate_id().unwrap();
            let aux = CreateWindowAux::new().background_pixel(root.black_pixel);
            connection
                .create_window(
                    root.root_depth,
                    window,
                    root.root,
                    0,
                    0,
                    400,
                    300,
                    0,
                    WindowClass::INPUT_OUTPUT,
                    0,
                    &aux,
                )
                .unwrap();
            connection.map_window(window).unwrap();
            connection.flush().unwrap();
            Self { connection, window }
        }

        /// The window id, as the engine names its window.
        pub fn id(&self) -> u64 {
            u64::from(self.window)
        }

        /// How many of this window's children hold an editor (the host's class is on the view
        /// inside each child).
        pub fn editor_children(&self) -> usize {
            let children = self.connection.query_tree(self.window).unwrap().reply().unwrap().children;
            children
                .into_iter()
                .filter(|clip| {
                    let inside = self.connection.query_tree(*clip).unwrap().reply().unwrap().children;
                    inside.into_iter().any(|view| {
                        let class =
                            self.connection.get_property(false, view, AtomEnum::WM_CLASS, AtomEnum::STRING, 0, 64);
                        class
                            .ok()
                            .and_then(|c| c.reply().ok())
                            .is_some_and(|c| c.value.starts_with(b"suhina-plugin-editor"))
                    })
                })
                .count()
        }
    }

    impl Drop for ParentWindow {
        fn drop(&mut self) {
            let _ = self.connection.destroy_window(self.window);
            let _ = self.connection.flush();
        }
    }
}

#[cfg(not(any(windows, target_os = "linux")))]
mod imp {
    pub struct ParentWindow;

    impl ParentWindow {
        pub fn open() -> Self {
            Self
        }

        pub fn id(&self) -> u64 {
            0
        }

        pub fn editor_children(&self) -> usize {
            0
        }
    }
}

pub use imp::ParentWindow;

/// Waits up to `timeout` until `done` holds, looking every 10 ms; whether it did.
pub fn soon(timeout: std::time::Duration, mut done: impl FnMut() -> bool) -> bool {
    let deadline = std::time::Instant::now() + timeout;
    loop {
        if done() {
            return true;
        }
        if std::time::Instant::now() >= deadline {
            return false;
        }
        std::thread::sleep(std::time::Duration::from_millis(10));
    }
}
