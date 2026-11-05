// Copyright (C) 2026 The Suhina contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! A VST3 plug-in's editor: the controller's `IPlugView`, attached to a window of the host's
//! own ([`crate::window`]). The plug-in asks for a new size through the frame the host gives
//! it (`IPlugFrame::resizeView`); when the user resizes a resizable window, the view is told
//! (`onSize`). On Linux the frame is also the plug-in's run loop (`IRunLoop`): the plug-in
//! registers the descriptors it waits on and its timers, and the host calls them from its main
//! loop ([`crate::run_loop`]).

use std::cell::RefCell;
use std::time::{Duration, Instant};
use vst3::Steinberg::Vst::{IEditController, IEditControllerTrait};

use vst3::Steinberg::Linux::{
    IEventHandler, IEventHandlerTrait, IRunLoop, IRunLoopTrait, ITimerHandler, ITimerHandlerTrait,
};
use vst3::Steinberg::*;
use vst3::{Class, ComPtr, ComRef, ComWrapper};

use crate::run_loop::Timers;
use crate::window::{self, Size, Window, WindowId};

/// The frame the view asks to be resized through, and on Linux its run loop.
struct Frame {
    /// The window the view is in.
    window: WindowId,
    handlers: RefCell<Vec<(ComPtr<IEventHandler>, i32)>>,
    timers: RefCell<Timers<ComPtr<ITimerHandler>>>,
}

impl Class for Frame {
    type Interfaces = (IPlugFrame, IRunLoop);
}

/// Most descriptors one plug-in may register.
const MAX_HANDLERS: usize = 64;

impl IRunLoopTrait for Frame {
    unsafe fn registerEventHandler(&self, handler: *mut IEventHandler, fd: i32) -> tresult {
        // SAFETY: the plug-in's handler, or null; a reference is kept while it is registered.
        let Some(handler) = (unsafe { ComRef::from_raw(handler) }).map(|h| h.to_com_ptr()) else {
            return kInvalidArgument;
        };
        let Ok(mut handlers) = self.handlers.try_borrow_mut() else { return kResultFalse };
        if handlers.len() >= MAX_HANDLERS {
            return kResultFalse;
        }
        handlers.push((handler, fd));
        kResultOk
    }

    unsafe fn unregisterEventHandler(&self, handler: *mut IEventHandler) -> tresult {
        let Ok(mut handlers) = self.handlers.try_borrow_mut() else { return kResultFalse };
        handlers.retain(|(kept, _)| kept.as_ptr() != handler);
        kResultOk
    }

    unsafe fn registerTimer(&self, handler: *mut ITimerHandler, milliseconds: u64) -> tresult {
        // SAFETY: as in `registerEventHandler`.
        let Some(handler) = (unsafe { ComRef::from_raw(handler) }).map(|h| h.to_com_ptr()) else {
            return kInvalidArgument;
        };
        let Ok(mut timers) = self.timers.try_borrow_mut() else { return kResultFalse };
        if timers.add(handler, Duration::from_millis(milliseconds)) { kResultOk } else { kResultFalse }
    }

    unsafe fn unregisterTimer(&self, handler: *mut ITimerHandler) -> tresult {
        let Ok(mut timers) = self.timers.try_borrow_mut() else { return kResultFalse };
        timers.remove(|kept| kept.as_ptr() == handler);
        kResultOk
    }
}

impl Frame {
    /// Calls the plug-in back for the descriptors that are ready and the timers that are due.
    /// The lists are copied first: a callback may register or unregister.
    fn run(&self) {
        let handlers: Vec<(ComPtr<IEventHandler>, i32)> =
            self.handlers.try_borrow().map(|h| h.clone()).unwrap_or_default();
        let fds: Vec<i32> = handlers.iter().map(|(_, fd)| *fd).collect();
        for fd in crate::run_loop::ready(&fds) {
            for (handler, _) in handlers.iter().filter(|(_, kept)| *kept == fd) {
                // SAFETY: a registered, live handler, called on the main thread.
                unsafe { handler.onFDIsSet(fd) };
            }
        }
        let due = self.timers.try_borrow_mut().map(|mut t| t.due(Instant::now())).unwrap_or_default();
        for timer in due {
            // SAFETY: as above.
            unsafe { timer.onTimer() };
        }
    }
}

impl IPlugFrameTrait for Frame {
    unsafe fn resizeView(&self, view: *mut IPlugView, size: *mut ViewRect) -> tresult {
        // SAFETY: the view asking, and the size it wants, for the call.
        let (Some(view), Some(rect)) = (unsafe { ComRef::from_raw(view) }, unsafe { size.as_mut() }) else {
            return kInvalidArgument;
        };
        window::resize(self.window, size_of(rect));
        // SAFETY: a live view, told its new size as the SDK asks after a resize.
        unsafe { view.onSize(rect) };
        kResultOk
    }
}

fn size_of(rect: &ViewRect) -> Size {
    let span = |from: i32, to: i32| u32::try_from(to.saturating_sub(from)).unwrap_or(0).clamp(1, 16_384);
    Size { width: span(rect.left, rect.right), height: span(rect.top, rect.bottom) }
}

/// An open editor.
pub(crate) struct Editor {
    view: ComPtr<IPlugView>,
    frame: ComWrapper<Frame>,
    window: Window,
    size: Size,
    /// The user may resize the window (a floating one the plug-in allows it for).
    resizable: bool,
    /// A size the editor got since the engine was last told, by the plug-in or the user.
    resized: Option<Size>,
}

/// The platform type a view is attached as on this system: Win32 on Windows, an `NSView` on
/// macOS (there is no X11 on a Mac), an X11 window id elsewhere.
const PLATFORM: FIDString = if cfg!(windows) {
    kPlatformTypeHWND
} else if cfg!(target_os = "macos") {
    kPlatformTypeNSView
} else {
    kPlatformTypeX11EmbedWindowID
};

impl Editor {
    /// Opens `controller`'s editor in a window of `mode`, titled `title` when it has a title.
    pub(crate) fn open(controller: &ComPtr<IEditController>, mode: window::Mode, title: &str) -> Result<Self, String> {
        // SAFETY: a live controller; the name is a static C string.
        let raw = unsafe { controller.createView(c"editor".as_ptr()) };
        // SAFETY: an owned reference to the view, or null when there is no editor.
        let view = unsafe { ComPtr::from_raw(raw) }.ok_or("the plug-in has no editor")?;
        // SAFETY: a live view; the platform name is a static C string.
        if unsafe { view.isPlatformTypeSupported(PLATFORM) } != kResultOk {
            return Err("the plug-in's editor does not run on this system".into());
        }
        let mut rect = ViewRect { left: 0, top: 0, right: 800, bottom: 600 };
        // SAFETY: a live view filling a rectangle.
        let _ = unsafe { view.getSize(&mut rect) };
        let size = size_of(&rect);
        // SAFETY: a live view.
        let resizable = unsafe { view.canResize() } == kResultTrue && mode == window::Mode::Floating;
        let window = Window::open_as(mode, title, size, resizable)?;
        let frame = ComWrapper::new(Frame {
            window: window.id(),
            handlers: RefCell::new(Vec::new()),
            timers: RefCell::new(Timers::new()),
        });
        let frame_pointer = frame.to_com_ptr::<IPlugFrame>().ok_or("frame")?;
        // SAFETY: a live view and a frame that outlives it (it is dropped after `removed`).
        let _ = unsafe { view.setFrame(frame_pointer.as_ptr()) };
        // SAFETY: a live view and the native window it goes in, which stays open until `close`.
        if unsafe { view.attached(window.handle(), PLATFORM) } != kResultOk {
            // SAFETY: as above, letting go of the frame.
            let _ = unsafe { view.setFrame(std::ptr::null_mut()) };
            return Err("the plug-in's editor did not open".into());
        }
        Ok(Self { view, frame, window, size, resizable, resized: None })
    }

    pub(crate) fn size(&self) -> Size {
        self.size
    }

    /// Asks the view to take `size` (the room the engine has for it), when it can be resized:
    /// it may change the size to one it takes (`checkSizeConstraint`). The window follows, and
    /// the engine is told the size it got. Nothing for a view of a fixed size.
    pub(crate) fn fit(&mut self, size: Size) {
        // SAFETY: a live view.
        if unsafe { self.view.canResize() } != kResultTrue || size.width == 0 || size.height == 0 {
            return;
        }
        let mut rect = ViewRect {
            left: 0,
            top: 0,
            right: i32::try_from(size.width).unwrap_or(i32::MAX),
            bottom: i32::try_from(size.height).unwrap_or(i32::MAX),
        };
        // SAFETY: a live view, asked whether it takes the size and then given it.
        let _ = unsafe { self.view.checkSizeConstraint(&mut rect) };
        let got = size_of(&rect);
        if got == self.size {
            return;
        }
        window::resize(self.window.id(), got);
        // SAFETY: as above.
        let _ = unsafe { self.view.onSize(&mut rect) };
        self.size = got;
        self.resized = Some(got);
    }

    /// Puts an embedded editor where the engine says.
    pub(crate) fn set_title(&self, title: &str) {
        window::set_title(self.window.id(), title);
    }

    pub(crate) fn place(&self, placement: window::Placement) {
        window::place(self.window.id(), placement);
    }

    /// Keeps the editor in step with its window: tells the view about a size the user gave
    /// the window, and notes a size the plug-in gave the view (through the frame). Returns
    /// whether the user asked the window to close.
    pub(crate) fn tick(&mut self) -> bool {
        self.frame.run();
        let now = window::client_size(self.window.id());
        if now != self.size && now.width > 0 && now.height > 0 {
            if self.resizable {
                let mut rect = ViewRect { left: 0, top: 0, right: now.width as i32, bottom: now.height as i32 };
                // SAFETY: a live view, asked whether it takes the size and then given it.
                let _ = unsafe { self.view.checkSizeConstraint(&mut rect) };
                // SAFETY: as above.
                let _ = unsafe { self.view.onSize(&mut rect) };
            }
            self.size = now;
            self.resized = Some(now);
        }
        window::take_close_request(self.window.id())
    }

    /// A size the editor got since this was last asked, to tell the engine.
    pub(crate) fn take_resized(&mut self) -> Option<Size> {
        self.resized.take()
    }

    /// Closes the editor: the view lets go of the window, then the window closes.
    pub(crate) fn close(self) {
        // SAFETY: a live, attached view.
        let _ = unsafe { self.view.removed() };
        // SAFETY: as above.
        let _ = unsafe { self.view.setFrame(std::ptr::null_mut()) };
        let Self { view, frame, window, .. } = self;
        drop(view);
        drop(frame);
        drop(window);
    }
}
