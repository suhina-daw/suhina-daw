// Copyright (C) 2026 The Suhina contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! Plug-in editor windows on Linux: an X11 window the editor is embedded in.

use std::cell::RefCell;
use std::ffi::c_void;

use x11rb::connection::Connection;
use x11rb::protocol::Event;
use x11rb::protocol::xproto::{
    AtomEnum, ConfigureWindowAux, ConnectionExt as _, CreateWindowAux, EventMask, PropMode, WindowClass,
};
use x11rb::rust_connection::RustConnection;
use x11rb::wrapper::ConnectionExt as _;

use super::{Placement, Size, WindowId};

x11rb::atom_manager! {
    Atoms: AtomsCookie {
        WM_PROTOCOLS,
        WM_DELETE_WINDOW,
        _NET_WM_NAME,
        UTF8_STRING,
    }
}

/// An open window: its own connection to the X server, the window and what happened to it.
struct Open {
    connection: RustConnection,
    window: u32,
    atoms: Atoms,
    close_requested: bool,
    size: Size,
    /// An embedded editor's clip window, the one inside the engine's window; the window
    /// itself (the view) is inside it.
    clip: Option<u32>,
}

thread_local! {
    // The editors' windows live on the host's main thread, like the plug-ins' views.
    static OPEN: RefCell<Vec<Open>> = const { RefCell::new(Vec::new()) };
}

/// Runs `f` on open window `id`, if it is open (and the list is not busy).
fn with_open<R>(id: WindowId, f: impl FnOnce(&mut Open) -> R) -> Option<R> {
    OPEN.with(|open| {
        let mut open = open.try_borrow_mut().ok()?;
        open.iter_mut().find(|o| o.window as usize == id.0).map(f)
    })
}

fn failed(what: &str) -> impl Fn(String) -> String + '_ {
    move |e| format!("could not {what} the editor's window: {e}")
}

pub(crate) struct Window {
    window: u32,
}

impl Window {
    /// Opens a window `title` with an inside of `size` on the X server `DISPLAY` names; the
    /// user may resize it when `resizable` (otherwise its smallest and largest size are
    /// that size).
    pub(crate) fn open(title: &str, size: Size, resizable: bool) -> Result<Self, String> {
        let connect = failed("open");
        let (connection, screen) = x11rb::connect(None).map_err(|e| connect(e.to_string()))?;
        let root = connection.setup().roots.get(screen).ok_or("no X screen")?;
        let (root_window, depth, background) = (root.root, root.root_depth, root.black_pixel);
        let atoms = Atoms::new(&connection).map_err(|e| connect(e.to_string()))?;
        let atoms = atoms.reply().map_err(|e| connect(e.to_string()))?;
        let window = connection.generate_id().map_err(|e| connect(e.to_string()))?;
        let width = u16::try_from(size.width).unwrap_or(u16::MAX);
        let height = u16::try_from(size.height).unwrap_or(u16::MAX);
        let aux = CreateWindowAux::new().event_mask(EventMask::STRUCTURE_NOTIFY).background_pixel(background);
        let make = || -> Result<(), String> {
            let error = |e: x11rb::errors::ConnectionError| connect(e.to_string());
            connection
                .create_window(depth, window, root_window, 0, 0, width, height, 0, WindowClass::INPUT_OUTPUT, 0, &aux)
                .map_err(error)?;
            connection
                .change_property8(PropMode::REPLACE, window, AtomEnum::WM_NAME, AtomEnum::STRING, title.as_bytes())
                .map_err(error)?;
            connection
                .change_property8(PropMode::REPLACE, window, atoms._NET_WM_NAME, atoms.UTF8_STRING, title.as_bytes())
                .map_err(error)?;
            connection
                .change_property32(
                    PropMode::REPLACE,
                    window,
                    atoms.WM_PROTOCOLS,
                    AtomEnum::ATOM,
                    &[atoms.WM_DELETE_WINDOW],
                )
                .map_err(error)?;
            connection
                .change_property8(
                    PropMode::REPLACE,
                    window,
                    AtomEnum::WM_CLASS,
                    AtomEnum::STRING,
                    b"suhina-plugin-editor\0Suhina\0",
                )
                .map_err(error)?;
            if let Some(owner) = super::owner().and_then(|o| u32::try_from(o).ok()) {
                // Floats above the engine's window and goes with it.
                connection
                    .change_property32(
                        PropMode::REPLACE,
                        window,
                        AtomEnum::WM_TRANSIENT_FOR,
                        AtomEnum::WINDOW,
                        &[owner],
                    )
                    .map_err(error)?;
            }
            if !resizable {
                // WM_NORMAL_HINTS: program-specified minimum and maximum size, both this one.
                let (w, h) = (u32::from(width), u32::from(height));
                let hints = [0x10 | 0x20, 0, 0, 0, 0, w, h, w, h, 0, 0, 0, 0, 0, 0, 0, 0, 0];
                connection
                    .change_property32(
                        PropMode::REPLACE,
                        window,
                        AtomEnum::WM_NORMAL_HINTS,
                        AtomEnum::WM_SIZE_HINTS,
                        &hints,
                    )
                    .map_err(error)?;
            }
            connection.map_window(window).map_err(error)?;
            connection.flush().map_err(error)?;
            Ok(())
        };
        make()?;
        let fresh = Open { connection, window, atoms, close_requested: false, size, clip: None };
        Self::keep(fresh)
    }

    /// Opens an editor's window of inside `size` as a child inside the engine's window
    /// `parent` (an X11 window id, which may belong to another client), hidden until
    /// [`place`] puts it somewhere in it.
    pub(crate) fn open_embedded(parent: u64, size: Size) -> Result<Self, String> {
        let connect = failed("open");
        let parent = u32::try_from(parent).map_err(|_| "no window to embed the editor in")?;
        let (connection, screen) = x11rb::connect(None).map_err(|e| connect(e.to_string()))?;
        let root = connection.setup().roots.get(screen).ok_or("no X screen")?;
        let (depth, background) = (root.root_depth, root.black_pixel);
        let atoms = Atoms::new(&connection).map_err(|e| connect(e.to_string()))?;
        let atoms = atoms.reply().map_err(|e| connect(e.to_string()))?;
        let clip = connection.generate_id().map_err(|e| connect(e.to_string()))?;
        let window = connection.generate_id().map_err(|e| connect(e.to_string()))?;
        let width = u16::try_from(size.width).unwrap_or(u16::MAX);
        let height = u16::try_from(size.height).unwrap_or(u16::MAX);
        let make = || -> Result<(), String> {
            let error = |e: x11rb::errors::ConnectionError| connect(e.to_string());
            // The clip: unmapped (hidden) until `place` sizes it to the panel's visible
            // area; 1 × 1 for now, as X refuses an empty window. The server is asked
            // whether it took it (`check`): a parent that is no window is an error here
            // rather than an event later.
            let plain = CreateWindowAux::new();
            connection
                .create_window(depth, clip, parent, 0, 0, 1, 1, 0, WindowClass::INPUT_OUTPUT, 0, &plain)
                .map_err(error)?
                .check()
                .map_err(|e| format!("no window to embed the editor in: {e}"))?;
            // The view: the editor's full size, inside the clip.
            let aux = CreateWindowAux::new().event_mask(EventMask::STRUCTURE_NOTIFY).background_pixel(background);
            connection
                .create_window(depth, window, clip, 0, 0, width, height, 0, WindowClass::INPUT_OUTPUT, 0, &aux)
                .map_err(error)?
                .check()
                .map_err(|e| connect(e.to_string()))?;
            connection
                .change_property8(
                    PropMode::REPLACE,
                    window,
                    AtomEnum::WM_CLASS,
                    AtomEnum::STRING,
                    b"suhina-plugin-editor\0Suhina\0",
                )
                .map_err(error)?;
            connection.map_window(window).map_err(error)?;
            connection.flush().map_err(error)?;
            Ok(())
        };
        if let Err(e) = make() {
            let _ = connection.destroy_window(window);
            let _ = connection.destroy_window(clip);
            let _ = connection.flush();
            return Err(e);
        }
        let fresh = Open { connection, window, atoms, close_requested: false, size, clip: Some(clip) };
        Self::keep(fresh)
    }

    /// Keeps `fresh` in the list of open windows, or closes it when the list is busy.
    fn keep(fresh: Open) -> Result<Self, String> {
        let window = fresh.window;
        let kept = OPEN.with(|open| match open.try_borrow_mut() {
            Ok(mut open) => {
                open.push(fresh);
                None
            }
            Err(_) => Some(fresh),
        });
        if let Some(lost) = kept {
            lost.close();
            return Err("could not keep the editor's window".into());
        }
        Ok(Self { window })
    }

    /// The window's id, as the pointer-sized handle plug-ins take on X11.
    pub(crate) fn handle(&self) -> *mut c_void {
        self.window as usize as *mut c_void
    }

    pub(crate) fn id(&self) -> WindowId {
        WindowId(self.window as usize)
    }
}

impl Open {
    /// Destroys the window (and an embedded one's clip).
    fn close(self) {
        let _ = self.connection.destroy_window(self.window);
        if let Some(clip) = self.clip {
            let _ = self.connection.destroy_window(clip);
        }
        let _ = self.connection.flush();
    }
}

impl Drop for Window {
    fn drop(&mut self) {
        let closed = OPEN.with(|open| {
            let mut open = open.try_borrow_mut().ok()?;
            let at = open.iter().position(|o| o.window == self.window)?;
            Some(open.swap_remove(at))
        });
        if let Some(open) = closed {
            open.close();
        }
    }
}

/// Resizes window `id` so its inside is `size` (a plug-in asked for it).
pub(crate) fn resize(id: WindowId, size: Size) {
    with_open(id, |open| {
        let aux = ConfigureWindowAux::new().width(size.width).height(size.height);
        let _ = open.connection.configure_window(open.window, &aux);
        let _ = open.connection.flush();
        open.size = size;
    });
}

/// Puts embedded window `id` at `placement` in the engine's window: the clip at the
/// visible area, the view inside it so that the editor's origin lands at `(x, y)` of the
/// engine's window. An empty visible area hides it. Nothing happens to a window that is
/// not embedded.
pub(crate) fn place(id: WindowId, placement: Placement) {
    with_open(id, |open| {
        let Some(clip) = open.clip else { return };
        let Placement { x, y, clip_x, clip_y, clip_width, clip_height } = placement;
        if clip_width == 0 || clip_height == 0 {
            let _ = open.connection.unmap_window(clip);
            let _ = open.connection.flush();
            return;
        }
        let view = ConfigureWindowAux::new().x(x.saturating_sub(clip_x)).y(y.saturating_sub(clip_y));
        let _ = open.connection.configure_window(open.window, &view);
        let clip_aux = ConfigureWindowAux::new()
            .x(clip_x)
            .y(clip_y)
            .width(clip_width)
            .height(clip_height)
            .stack_mode(x11rb::protocol::xproto::StackMode::ABOVE);
        let _ = open.connection.configure_window(clip, &clip_aux);
        let _ = open.connection.map_window(clip);
        let _ = open.connection.flush();
    });
}

/// Whether the user asked window `id` to close since the last call.
pub(crate) fn take_close_request(id: WindowId) -> bool {
    with_open(id, |open| std::mem::take(&mut open.close_requested)).unwrap_or(false)
}

/// Window `id`'s inside size as the X server last reported it.
pub(crate) fn client_size(id: WindowId) -> Size {
    with_open(id, |open| open.size).unwrap_or(Size { width: 0, height: 0 })
}

/// Whether an editor's window is open, so the main loop serves it often.
pub(crate) fn any_open() -> bool {
    OPEN.with(|open| open.try_borrow().map_or(true, |open| !open.is_empty()))
}

/// Takes in the events waiting for every open window: a close request, a new size. (The
/// plug-ins' own windows are on their own connections and served through their run loops.)
pub(crate) fn pump() {
    OPEN.with(|open| {
        let Ok(mut open) = open.try_borrow_mut() else { return };
        for open in open.iter_mut() {
            while let Ok(Some(event)) = open.connection.poll_for_event() {
                match event {
                    Event::ClientMessage(message) => {
                        let [first, ..] = message.data.as_data32();
                        if message.window == open.window && first == open.atoms.WM_DELETE_WINDOW {
                            open.close_requested = true;
                        }
                    }
                    Event::ConfigureNotify(configured) if configured.window == open.window => {
                        open.size = Size { width: u32::from(configured.width), height: u32::from(configured.height) };
                    }
                    _ => {}
                }
            }
        }
    });
}
