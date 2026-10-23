// Copyright (C) 2026 The Suhina contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! Plug-in editor windows on Windows: a child of the host window, made with the Win32 API.

use std::cell::RefCell;
use std::ffi::c_void;
use std::sync::OnceLock;

use std::time::Duration;

use windows_sys::Win32::Foundation::{HWND, LPARAM, LRESULT, RECT, WPARAM};
use windows_sys::Win32::System::LibraryLoader::GetModuleHandleW;
use windows_sys::Win32::System::Threading::GetCurrentThreadId;
use windows_sys::Win32::UI::Input::KeyboardAndMouse::SetFocus;
use windows_sys::Win32::UI::WindowsAndMessaging::{
    AdjustWindowRectEx, CW_USEDEFAULT, CreateWindowExW, DefWindowProcW, DestroyWindow, DispatchMessageW, GW_CHILD,
    GetWindow, HWND_TOP, IDC_ARROW, IsWindow, LoadCursorW, MSG, MWMO_INPUTAVAILABLE, MsgWaitForMultipleObjectsEx,
    PM_REMOVE, PeekMessageW, PostThreadMessageW, QS_ALLINPUT, RegisterClassW, SW_HIDE, SW_SHOW, SWP_NOACTIVATE,
    SWP_NOMOVE, SWP_NOSIZE, SWP_NOZORDER, SWP_SHOWWINDOW, SetWindowPos, ShowWindow, TranslateMessage, WM_APP, WM_CLOSE,
    WM_SETFOCUS, WM_SIZE, WNDCLASSW, WS_CAPTION, WS_CHILD, WS_CLIPCHILDREN, WS_CLIPSIBLINGS, WS_MAXIMIZEBOX,
    WS_MINIMIZEBOX, WS_OVERLAPPED, WS_SYSMENU, WS_THICKFRAME, WS_VISIBLE,
};

use super::{Placement, Size, WindowId};

/// Wakes the main thread from [`wait`] when a control message has come, from another thread.
#[derive(Clone, Copy)]
pub(crate) struct Waker(u32);

impl Waker {
    /// A waker for the calling thread (the host's main thread).
    pub(crate) fn current() -> Self {
        // SAFETY: no arguments; the id of the calling thread.
        Self(unsafe { GetCurrentThreadId() })
    }

    pub(crate) fn wake(self) {
        // SAFETY: a message without pointers to a thread id; if that thread is gone the call
        // fails, which is fine.
        unsafe { PostThreadMessageW(self.0, WM_APP, 0, 0) };
    }
}

/// Waits until a window message is waiting for this thread (input, painting, a plug-in's
/// timer), the waker wakes it, or `timeout` has passed, so an editor is served at once
/// rather than between ticks.
pub(crate) fn wait(timeout: Duration) {
    let millis = u32::try_from(timeout.as_millis()).unwrap_or(u32::MAX);
    // SAFETY: no handles to wait on, only this thread's message queue.
    unsafe { MsgWaitForMultipleObjectsEx(0, std::ptr::null(), millis, QS_ALLINPUT, MWMO_INPUTAVAILABLE) };
}

/// What happened to one open window.
struct State {
    id: WindowId,
    /// The user asked the window to close.
    close_requested: bool,
    /// The window's inside size as the system last reported it.
    size: Size,
    /// The window's style, for turning an inside size into a window size.
    style: u32,
    /// An embedded editor's clip window, the one inside the engine's window; the window
    /// itself (the view) is inside it.
    clip: Option<HWND>,
}

thread_local! {
    // The editors' windows live on the host's main thread, like the plug-ins' views.
    static OPEN: RefCell<Vec<State>> = const { RefCell::new(Vec::new()) };
}

/// Runs `f` on the state of window `id`, if it is open. The window procedure may be called
/// from inside a system call made here; such a nested call finds the list busy and does
/// nothing rather than borrowing it twice.
fn with_state<R>(id: WindowId, f: impl FnOnce(&mut State) -> R) -> Option<R> {
    OPEN.with(|open| {
        let mut open = open.try_borrow_mut().ok()?;
        open.iter_mut().find(|s| s.id == id).map(f)
    })
}

fn wide(text: &str) -> Vec<u16> {
    text.encode_utf16().chain(std::iter::once(0)).collect()
}

const CLASS: &str = "SuhinaPluginEditor";

unsafe extern "system" fn window_proc(hwnd: HWND, message: u32, wparam: WPARAM, lparam: LPARAM) -> LRESULT {
    let id = WindowId(hwnd as usize);
    match message {
        WM_CLOSE => {
            // The host closes it, after the plug-in has let go of it.
            with_state(id, |s| s.close_requested = true);
            0
        }
        WM_SIZE => {
            let (width, height) = ((lparam & 0xFFFF) as u32, ((lparam >> 16) & 0xFFFF) as u32);
            with_state(id, |s| s.size = Size { width, height });
            0
        }
        // The keyboard goes on to the window inside, down to the plug-in's own view, so its
        // keys (Ctrl+Z in a pitch editor) reach it and not the engine's window.
        WM_SETFOCUS => {
            // SAFETY: a window of this thread; its first child, or null.
            let child = unsafe { GetWindow(hwnd, GW_CHILD) };
            if !child.is_null() {
                // SAFETY: a live child window of this one.
                unsafe { SetFocus(child) };
            }
            0
        }
        // SAFETY: the system's default handling of a message for this window.
        _ => unsafe { DefWindowProcW(hwnd, message, wparam, lparam) },
    }
}

/// Registers the window class once; whether it is registered.
fn register() -> bool {
    static REGISTERED: OnceLock<bool> = OnceLock::new();
    *REGISTERED.get_or_init(|| {
        let class = wide(CLASS);
        // SAFETY: a null name asks for this program's own module.
        let instance = unsafe { GetModuleHandleW(std::ptr::null()) };
        // SAFETY: the system's own arrow cursor.
        let cursor = unsafe { LoadCursorW(std::ptr::null_mut(), IDC_ARROW) };
        let description = WNDCLASSW {
            style: 0,
            lpfnWndProc: Some(window_proc),
            cbClsExtra: 0,
            cbWndExtra: 0,
            hInstance: instance,
            hIcon: std::ptr::null_mut(),
            hCursor: cursor,
            hbrBackground: std::ptr::null_mut(),
            lpszMenuName: std::ptr::null(),
            lpszClassName: class.as_ptr(),
        };
        // SAFETY: the description and the class name it points to outlive the call; the
        // system copies them.
        unsafe { RegisterClassW(&description) != 0 }
    })
}

/// The outer size for an inside of `size` with `style`.
fn outer(size: Size, style: u32) -> (i32, i32) {
    let mut rect = RECT { left: 0, top: 0, right: size.width as i32, bottom: size.height as i32 };
    // SAFETY: a rectangle to adjust in place.
    unsafe { AdjustWindowRectEx(&mut rect, style, 0, 0) };
    (rect.right - rect.left, rect.bottom - rect.top)
}

pub(crate) struct Window {
    hwnd: HWND,
}

impl Window {
    /// Opens a window `title` with an inside of `size`; the user may resize it when
    /// `resizable`.
    pub(crate) fn open(title: &str, size: Size, resizable: bool) -> Result<Self, String> {
        if !register() {
            return Err("could not register the editor's window class".into());
        }
        let mut style = WS_OVERLAPPED | WS_CAPTION | WS_SYSMENU | WS_MINIMIZEBOX;
        if resizable {
            style |= WS_THICKFRAME | WS_MAXIMIZEBOX;
        }
        let (width, height) = outer(size, style);
        let (class, title) = (wide(CLASS), wide(title));
        // SAFETY: as in `register`.
        let instance = unsafe { GetModuleHandleW(std::ptr::null()) };
        // The engine's window, if it still is one: the editor's is owned by it (not a child
        // of it), so it floats above Suhina and goes with it.
        let owner = super::owner()
            .and_then(|o| usize::try_from(o).ok())
            .map(|o| o as HWND)
            // SAFETY: only asks whether the handle names a window; any value is allowed.
            .filter(|&o| unsafe { IsWindow(o) } != 0)
            .unwrap_or(std::ptr::null_mut());
        // SAFETY: a registered class and two strings that outlive the call; an owner window
        // just checked (or none); no menu or creation data.
        let hwnd = unsafe {
            CreateWindowExW(
                0,
                class.as_ptr(),
                title.as_ptr(),
                style,
                CW_USEDEFAULT,
                CW_USEDEFAULT,
                width,
                height,
                owner,
                std::ptr::null_mut(),
                instance,
                std::ptr::null(),
            )
        };
        if hwnd.is_null() {
            return Err("could not open the editor's window".into());
        }
        let state = State { id: WindowId(hwnd as usize), close_requested: false, size, style, clip: None };
        let kept = OPEN.with(|open| open.try_borrow_mut().map(|mut open| open.push(state)).is_ok());
        let window = Self { hwnd };
        if !kept {
            return Err("could not keep the editor's window".into());
        }
        // SAFETY: the window just made.
        unsafe { ShowWindow(hwnd, SW_SHOW) };
        Ok(window)
    }

    /// Opens an editor's window of inside `size` as a child inside the engine's window
    /// `parent` (a Win32 `HWND`, which may belong to another process), hidden until
    /// [`place`] puts it somewhere in it.
    pub(crate) fn open_embedded(parent: u64, size: Size) -> Result<Self, String> {
        if !register() {
            return Err("could not register the editor's window class".into());
        }
        let parent = usize::try_from(parent).ok().map(|p| p as HWND).ok_or("no window to embed the editor in")?;
        // SAFETY: only asks whether the handle names a window; any value is allowed.
        if unsafe { IsWindow(parent) } == 0 {
            return Err("the window to embed the editor in is gone".into());
        }
        let (class, title) = (wide(CLASS), wide(""));
        // SAFETY: as in `register`.
        let instance = unsafe { GetModuleHandleW(std::ptr::null()) };
        // The clip: a hidden, empty child of the engine's window, which `place` sizes to
        // the panel's visible area and shows.
        let clip_style = WS_CHILD | WS_CLIPCHILDREN | WS_CLIPSIBLINGS;
        // SAFETY: a registered class and two strings that outlive the call; a parent
        // window just checked; no menu or creation data.
        let clip = unsafe {
            CreateWindowExW(
                0,
                class.as_ptr(),
                title.as_ptr(),
                clip_style,
                0,
                0,
                0,
                0,
                parent,
                std::ptr::null_mut(),
                instance,
                std::ptr::null(),
            )
        };
        if clip.is_null() {
            return Err("could not open the editor's window in the engine's".into());
        }
        // The view: the editor's full size, inside the clip.
        let style = WS_CHILD | WS_VISIBLE | WS_CLIPSIBLINGS;
        let (width, height) = outer(size, style);
        // SAFETY: as above, inside the clip just made.
        let hwnd = unsafe {
            CreateWindowExW(
                0,
                class.as_ptr(),
                title.as_ptr(),
                style,
                0,
                0,
                width,
                height,
                clip,
                std::ptr::null_mut(),
                instance,
                std::ptr::null(),
            )
        };
        if hwnd.is_null() {
            // SAFETY: the clip this made, on this thread.
            unsafe { DestroyWindow(clip) };
            return Err("could not open the editor's window in the engine's".into());
        }
        let state = State { id: WindowId(hwnd as usize), close_requested: false, size, style, clip: Some(clip) };
        let kept = OPEN.with(|open| open.try_borrow_mut().map(|mut open| open.push(state)).is_ok());
        if !kept {
            // SAFETY: both windows this made, on this thread; the view first, as a child of
            // the clip.
            unsafe { DestroyWindow(hwnd) };
            // SAFETY: as above.
            unsafe { DestroyWindow(clip) };
            return Err("could not keep the editor's window".into());
        }
        Ok(Self { hwnd })
    }

    /// The native handle, for the plug-in to attach its view to.
    pub(crate) fn handle(&self) -> *mut c_void {
        self.hwnd
    }

    pub(crate) fn id(&self) -> WindowId {
        WindowId(self.hwnd as usize)
    }
}

impl Drop for Window {
    fn drop(&mut self) {
        let id = self.id();
        let clip = OPEN.with(|open| {
            let mut open = open.try_borrow_mut().ok()?;
            let at = open.iter().position(|s| s.id == id)?;
            open.swap_remove(at).clip
        });
        // SAFETY: the window this made, destroyed once, on the thread that made it.
        unsafe { DestroyWindow(self.hwnd) };
        if let Some(clip) = clip {
            // SAFETY: the clip this made for an embedded window, destroyed once, on the
            // thread that made it, after the view inside it.
            unsafe { DestroyWindow(clip) };
        }
    }
}

/// Puts embedded window `id` at `placement` in the engine's window: the clip at the
/// visible area, the view inside it so that the editor's origin lands at `(x, y)` of the
/// engine's window. An empty visible area hides it. Nothing happens to a window that is
/// not embedded.
pub(crate) fn place(id: WindowId, placement: Placement) {
    let Some(Some(clip)) = with_state(id, |s| s.clip) else { return };
    let Placement { x, y, clip_x, clip_y, clip_width, clip_height } = placement;
    if clip_width == 0 || clip_height == 0 {
        // SAFETY: an open window (it is in the list).
        unsafe { ShowWindow(clip, SW_HIDE) };
        return;
    }
    let span = |n: u32| i32::try_from(n).unwrap_or(i32::MAX);
    // SAFETY: the view, an open window; only its place inside the clip changes.
    unsafe {
        SetWindowPos(
            id.0 as HWND,
            std::ptr::null_mut(),
            x.saturating_sub(clip_x),
            y.saturating_sub(clip_y),
            0,
            0,
            SWP_NOSIZE | SWP_NOZORDER | SWP_NOACTIVATE,
        )
    };
    // SAFETY: the clip, an open window, put over the engine's other children where the
    // panel shows the editor.
    unsafe {
        SetWindowPos(
            clip,
            HWND_TOP,
            clip_x,
            clip_y,
            span(clip_width),
            span(clip_height),
            SWP_NOACTIVATE | SWP_SHOWWINDOW,
        )
    };
}

/// Resizes window `id` so its inside is `size` (a plug-in asked for it).
pub(crate) fn resize(id: WindowId, size: Size) {
    let Some(style) = with_state(id, |s| {
        s.size = size;
        s.style
    }) else {
        return;
    };
    let (width, height) = outer(size, style);
    // SAFETY: an open window (it is in the list); only its size changes.
    unsafe {
        SetWindowPos(
            id.0 as HWND,
            std::ptr::null_mut(),
            0,
            0,
            width,
            height,
            SWP_NOMOVE | SWP_NOZORDER | SWP_NOACTIVATE,
        )
    };
}

/// Whether the user asked window `id` to close since the last call.
pub(crate) fn take_close_request(id: WindowId) -> bool {
    with_state(id, |s| std::mem::take(&mut s.close_requested)).unwrap_or(false)
}

/// Window `id`'s inside size as the system last reported it.
pub(crate) fn client_size(id: WindowId) -> Size {
    with_state(id, |s| s.size).unwrap_or(Size { width: 0, height: 0 })
}

/// The plug-in is taking text: the key's window is a system text field (an Edit or RichEdit
/// control, which VSTGUI and others open to type in) or this thread shows a text caret. A
/// space typed there is the plug-in's. A field a plug-in draws itself without a caret is not
/// seen here.
fn typing(hwnd: HWND) -> bool {
    use windows_sys::Win32::UI::WindowsAndMessaging::{GUITHREADINFO, GetClassNameW, GetGUIThreadInfo};
    let mut class = [0u16; 32];
    // SAFETY: the buffer and its length are this array's.
    let length = unsafe { GetClassNameW(hwnd, class.as_mut_ptr(), class.len() as i32) };
    let class = String::from_utf16_lossy(&class[..usize::try_from(length).unwrap_or(0)]).to_lowercase();
    if class == "edit" || class.starts_with("richedit") {
        return true;
    }
    // SAFETY: a zeroed info with its size set is what the call fills; thread 0 is this one's.
    let mut info: GUITHREADINFO = unsafe { std::mem::zeroed() };
    info.cbSize = std::mem::size_of::<GUITHREADINFO>() as u32;
    // SAFETY: no arguments; this thread's id.
    let thread = unsafe { GetCurrentThreadId() };
    // SAFETY: as above.
    let found = unsafe { GetGUIThreadInfo(thread, &mut info) };
    found != 0 && !info.hwndCaret.is_null()
}

/// Once an editor is open this process is interactive, not background work: Windows 11
/// otherwise may class a host with no console of its own as background, throttle it
/// (EcoQoS) and coarsen its timers, and a plug-in that paces its drawing by timers (Pro-Q 4)
/// then redraws a few times a second (#638). The timers get 1 ms resolution too, as DAWs
/// set it for their editors.
pub(crate) fn interactive() {
    static ONCE: std::sync::Once = std::sync::Once::new();
    ONCE.call_once(|| {
        use windows_sys::Win32::System::Threading::{
            GetCurrentProcess, PROCESS_POWER_THROTTLING_CURRENT_VERSION, PROCESS_POWER_THROTTLING_EXECUTION_SPEED,
            PROCESS_POWER_THROTTLING_IGNORE_TIMER_RESOLUTION, PROCESS_POWER_THROTTLING_STATE, ProcessPowerThrottling,
            SetProcessInformation,
        };
        let state = PROCESS_POWER_THROTTLING_STATE {
            Version: PROCESS_POWER_THROTTLING_CURRENT_VERSION,
            ControlMask: PROCESS_POWER_THROTTLING_EXECUTION_SPEED | PROCESS_POWER_THROTTLING_IGNORE_TIMER_RESOLUTION,
            StateMask: 0,
        };
        // SAFETY: this process's pseudo handle; no arguments.
        let process = unsafe { GetCurrentProcess() };
        // SAFETY: the state lives through the call and its size is given.
        let _ = unsafe {
            SetProcessInformation(
                process,
                ProcessPowerThrottling,
                std::ptr::from_ref(&state).cast(),
                std::mem::size_of::<PROCESS_POWER_THROTTLING_STATE>() as u32,
            )
        };
        // SAFETY: no pointers; kept for the life of the process, as DAWs keep it.
        let _ = unsafe { windows_sys::Win32::Media::timeBeginPeriod(1) };
    });
}

thread_local! {
    /// The last Space press went to the engine, so its release does too.
    static SPACE_OURS: std::cell::Cell<bool> = const { std::cell::Cell::new(false) };
}

/// Handles the window messages waiting for this thread's windows (the editors' and the
/// plug-ins' own). Called from the host's main loop.
pub(crate) fn pump() {
    // SAFETY: a zeroed message is a valid value to be filled.
    let mut message: MSG = unsafe { std::mem::zeroed() };
    // SAFETY: the message is filled by the call; null asks for any window of this thread.
    while unsafe { PeekMessageW(&mut message, std::ptr::null_mut(), 0, 0, PM_REMOVE) } != 0 {
        // Space goes to the engine, not the plug-in: its press (bit 30 of lParam is set on
        // an auto-repeat) and its release are kept from the window. With Ctrl or the Windows
        // key held it is the plug-in's (Alt+Space is a system key, never seen here); its
        // release goes where its press went.
        const WM_KEYDOWN: u32 = 0x0100;
        const WM_KEYUP: u32 = 0x0101;
        const VK_SPACE: usize = 0x20;
        if message.wParam == VK_SPACE && message.message == WM_KEYDOWN {
            let held = |key: u16| {
                // SAFETY: reads this thread's keyboard state; no pointers.
                let state = unsafe { windows_sys::Win32::UI::Input::KeyboardAndMouse::GetKeyState(i32::from(key)) };
                state < 0
            };
            use windows_sys::Win32::UI::Input::KeyboardAndMouse::{VK_CONTROL, VK_LWIN, VK_RWIN};
            let ours = !(held(VK_CONTROL) || held(VK_LWIN) || held(VK_RWIN) || typing(message.hwnd));
            SPACE_OURS.with(|s| s.set(ours));
            if ours {
                super::caught_space(message.lParam & (1 << 30) != 0);
                continue;
            }
        } else if message.wParam == VK_SPACE && message.message == WM_KEYUP && SPACE_OURS.with(std::cell::Cell::get) {
            continue;
        }
        // SAFETY: a message just taken from the queue.
        unsafe { TranslateMessage(&message) };
        // SAFETY: as above.
        unsafe { DispatchMessageW(&message) };
    }
}
