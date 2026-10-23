// Copyright (C) 2026 The Suhina contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! Plug-in editor windows on macOS: an NSView in an NSWindow, through AppKit.

use std::cell::{Cell, RefCell};
use std::ffi::c_void;

use objc2::rc::Retained;
use objc2::runtime::NSObjectProtocol;
use objc2::{ClassType, MainThreadOnly};
use objc2_app_kit::{
    NSApplication, NSApplicationActivationPolicy, NSBackingStoreType, NSEventMask, NSEventModifierFlags, NSEventType,
    NSFloatingWindowLevel, NSNormalWindowLevel, NSView, NSWindow, NSWindowStyleMask, NSWorkspace,
};
use objc2_foundation::{MainThreadMarker, NSDate, NSDefaultRunLoopMode, NSPoint, NSRect, NSSize, NSString};

use super::{Placement, Size, WindowId};

/// One open window: its AppKit objects, and what happened to it.
struct State {
    id: WindowId,
    window: Retained<NSWindow>,
    /// The content view the plug-in's view attaches into.
    view: Retained<NSView>,
    /// The user asked the window to close.
    close_requested: bool,
    /// The window's inside size as the system last reported it.
    size: Size,
}

/// A window of this host's own, with a content view for the plug-in's editor to attach to.
pub(crate) struct Window {
    id: WindowId,
}

thread_local! {
    // The editors' windows live on the host's main thread, like the plug-ins' views.
    static OPEN: RefCell<Vec<State>> = const { RefCell::new(Vec::new()) };
    /// `pump` runs AppKit calls that can reach back into this code (an event's handler
    /// opening or closing a window); a nested call from inside one does nothing rather than
    /// running the loop twice.
    static PUMPING: Cell<bool> = const { Cell::new(false) };
}

/// The shared application, set once to run as an accessory: the editors' windows belong to
/// this process, which has no Dock icon of its own and never becomes the active app on its
/// own: a window takes the focus when it opens. `None` off the main thread, where windows
/// cannot live; the host's message loop runs on the main thread.
fn app() -> Option<Retained<NSApplication>> {
    let marker = MainThreadMarker::new()?;
    let app = NSApplication::sharedApplication(marker);
    app.setActivationPolicy(NSApplicationActivationPolicy::Accessory);
    Some(app)
}

impl Window {
    /// Opens a window `title` with an inside of `size`; the user may resize it when
    /// `resizable`.
    pub(crate) fn open(title: &str, size: Size, resizable: bool) -> Result<Self, String> {
        let Some(app) = app() else { return Err("the editor's window wants the main thread".into()) };
        let mut style = NSWindowStyleMask::Titled | NSWindowStyleMask::Closable | NSWindowStyleMask::Miniaturizable;
        if resizable {
            style |= NSWindowStyleMask::Resizable;
        }
        let content = NSSize::new(f64::from(size.width), f64::from(size.height));
        // SAFETY: this is the host's main thread, where AppKit wants its calls.
        let allocated = unsafe { NSWindow::alloc(MainThreadMarker::new_unchecked()) };
        // SAFETY: on the main thread; the window joins the OPEN list before this returns.
        let window: Retained<NSWindow> = unsafe {
            NSWindow::initWithContentRect_styleMask_backing_defer(
                allocated,
                NSRect::new(NSPoint::ZERO, content),
                style,
                NSBackingStoreType::Buffered,
                false,
            )
        };
        // SAFETY: on the main thread; the window just made.
        unsafe { window.setReleasedWhenClosed(false) };
        window.setTitle(&NSString::from_str(title));
        let Some(view) = window.contentView() else {
            return Err("could not open the editor's window".into());
        };
        let id = WindowId(Retained::as_ptr(&view) as usize);
        let state = State { id, view: view.clone(), close_requested: false, size, window: window.clone() };
        if !OPEN.with(|open| open.try_borrow_mut().map(|mut open| open.push(state)).is_ok()) {
            return Err("could not keep the editor's window".into());
        }
        // In the middle of the screen, where AppKit puts a new window, rather than at the
        // corner its frame starts at.
        window.center();
        // The window takes the keyboard: this process is never the active app on its own,
        // so it asks to come forward when the user opens an editor.
        window.makeKeyAndOrderFront(None);
        app.activate();
        Ok(Self { id })
    }

    /// An editor cannot open inside another process's window here: a view of this process
    /// cannot attach to one of another's, so the engine opens a window of its own instead.
    pub(crate) fn open_embedded(_parent: u64, _size: Size) -> Result<Self, String> {
        Err("a plug-in's editor opens in a window of its own on this system".into())
    }

    /// The content view's pointer, for the plug-in to attach its view to.
    pub(crate) fn handle(&self) -> *mut c_void {
        self.id.0 as *mut c_void
    }

    pub(crate) fn id(&self) -> WindowId {
        self.id
    }
}

impl Drop for Window {
    fn drop(&mut self) {
        // The window goes away; a user-closed one is already out of sight. The plug-in has
        // let go of its view before this (the editors close their views first).
        // It is taken out of the list first and hidden after, so the list is not borrowed
        // while AppKit runs.
        let closed = OPEN.with(|open| {
            let mut open = open.try_borrow_mut().ok()?;
            let at = open.iter().position(|s| s.id == self.id)?;
            Some(open.swap_remove(at))
        });
        if let Some(s) = closed
            && s.window.isVisible()
        {
            s.window.orderOut(None);
        }
    }
}

/// Runs `f` on the state of open window `id`, if it is still open.
fn with_state<R>(id: WindowId, f: impl FnOnce(&mut State) -> R) -> Option<R> {
    OPEN.with(|open| {
        let mut open = open.try_borrow_mut().ok()?;
        open.iter_mut().find(|s| s.id == id).map(f)
    })
}

/// Puts an embedded window at `placement`: there are no embedded windows here
/// ([`Window::open_embedded`]), so nothing to do.
pub(crate) fn place(_id: WindowId, _placement: Placement) {}

/// Resizes window `id` so its inside is `size` (a plug-in asked for it).
pub(crate) fn resize(id: WindowId, size: Size) {
    with_state(id, |s| {
        s.window.setContentSize(NSSize::new(f64::from(size.width), f64::from(size.height)));
        s.size = size;
    });
}

/// Whether the user asked window `id` to close since the last call: its close button made
/// it leave the screen.
pub(crate) fn take_close_request(id: WindowId) -> bool {
    with_state(id, |s| {
        s.close_requested |= !s.window.isVisible();
        std::mem::take(&mut s.close_requested)
    })
    .unwrap_or(true)
}

/// Window `id`'s inside size as the system last reported it.
pub(crate) fn client_size(id: WindowId) -> Size {
    with_state(id, |s| {
        let frame = s.view.frame();
        Size { width: frame.size.width.round() as u32, height: frame.size.height.round() as u32 }
    })
    .unwrap_or(Size { width: 0, height: 0 })
}

/// Handles the events waiting for this application's windows (the editors' and the
/// plug-ins' own): input, painting, moving, closing. Called from the host's main loop.
pub(crate) fn pump() {
    // A nested pump (an event's handler asked here again) would run the loop twice; the
    // outer one is already draining everything there is.
    if PUMPING.with(|p| p.replace(true)) {
        return;
    }
    if let Some(app) = app() {
        // SAFETY: on the main thread; the application and its windows.
        unsafe {
            let past = NSDate::distantPast();
            while let Some(event) = app.nextEventMatchingMask_untilDate_inMode_dequeue(
                NSEventMask::Any,
                Some(&past),
                NSDefaultRunLoopMode,
                true,
            ) {
                // Space (key code 49) without ⌘, ⌃ or ⌥ goes to the engine, not the plug-in,
                // unless the plug-in is typing in a system text field (its window's first
                // responder is a text view, the field editor AppKit text fields use).
                let kind = event.r#type();
                let typing = event
                    .window(app.mtm())
                    .and_then(|w| w.firstResponder())
                    .is_some_and(|r| r.isKindOfClass(objc2_app_kit::NSText::class()));
                if (kind == NSEventType::KeyDown || kind == NSEventType::KeyUp)
                    && event.keyCode() == 49
                    && !typing
                    && !event.modifierFlags().intersects(
                        NSEventModifierFlags::Command | NSEventModifierFlags::Control | NSEventModifierFlags::Option,
                    )
                {
                    if kind == NSEventType::KeyDown {
                        super::caught_space(event.isARepeat());
                    }
                    continue;
                }
                app.sendEvent(&event);
            }
        }
    }
    PUMPING.with(|p| p.set(false));
    float_while_ours();
}

thread_local! {
    static LEVEL_CHECKED: Cell<Option<std::time::Instant>> = const { Cell::new(None) };
}

/// The editors float above other windows while Suhina or this host is the active app, so a
/// click in Suhina does not hide them behind it, and sink back to normal windows while
/// another app is: macOS has no window ownership across processes, which is how Windows and
/// X11 keep them over Suhina. Checked ten times a second.
fn float_while_ours() {
    let due = LEVEL_CHECKED.with(|c| c.get().is_none_or(|at| at.elapsed() >= std::time::Duration::from_millis(100)));
    if !due || !any_open() {
        return;
    }
    LEVEL_CHECKED.with(|c| c.set(Some(std::time::Instant::now())));
    let front = NSWorkspace::sharedWorkspace().frontmostApplication().map(|a| a.processIdentifier());
    let ours = |pid: i32| {
        u32::try_from(pid).is_ok_and(|pid| pid == std::process::id() || pid == std::os::unix::process::parent_id())
    };
    let level = if front.is_some_and(ours) { NSFloatingWindowLevel } else { NSNormalWindowLevel };
    OPEN.with(|open| {
        if let Ok(open) = open.try_borrow() {
            for s in open.iter() {
                if s.window.level() != level {
                    s.window.setLevel(level);
                }
            }
        }
    });
}

pub(crate) fn any_open() -> bool {
    OPEN.with(|open| open.try_borrow().map(|open| !open.is_empty()).unwrap_or(false))
}
