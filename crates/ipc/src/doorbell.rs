// Copyright (C) 2026 The Suhina contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! Waking the other process when a word in shared memory changes, and waiting for that with a
//! deadline. Each side first spins briefly (a fast host answers within microseconds), then
//! sleeps in the operating system:
//!
//! - Linux: a futex on the word itself.
//! - Windows: an auto-reset event per direction, named after the region, waited on together
//!   with a high-resolution waitable timer of the waiting side's own. The timer wakes the wait
//!   about [`FINE_TAIL`] before the deadline and the rest is spent yielding, so a wait ends on
//!   time rather than on the system timer's next tick (15.6 ms by default). Without a
//!   high-resolution timer (Windows 10 before 1803) the event's own timeout is cut a tick short
//!   and the rest is yielded the same way.
//! - macOS: short naps between checks. A thread that waits by yielding alone never sleeps, so
//!   the kernel's timeshare scheduler demotes it for burning a core and hands it to the
//!   efficiency cores, where its round trip misses the block's deadline (measured: an idle
//!   round trip of 138 µs, p50 651 µs and 11 % of blocks late on a machine with three busy
//!   processes). A nap counts as waiting and keeps the thread in the scheduler's good books;
//!   [`NAP`] caps the requested sleep; scheduling can delay the actual wake-up. A semaphore-based wait comes with the macOS
//!   prototype (ADR 0006, not decided yet): Mach semaphores live in one task and cannot be
//!   shared with another process by name.

use std::sync::atomic::{AtomicU32, Ordering};
use std::time::{Duration, Instant};

use crate::IpcError;

/// Checks before going to sleep.
const SPINS: u32 = 256;

/// How long the macOS wait spins before it naps: a ring landing in the spin is noticed at once,
/// so this bounds how often a wake must wait out a nap.
#[cfg_attr(any(target_os = "linux", windows), allow(dead_code))]
const SPIN_BEFORE_NAP: Duration = Duration::from_micros(40);

/// Maximum requested sleep for the macOS wait; the scheduler may wake it later. It stays
/// far inside a block's deadline (a quarter of a millisecond, next to a block's 0.67–2.7 ms;
/// with the plug-in's own work of a few hundred microseconds the round trip keeps its room).
#[cfg_attr(any(target_os = "linux", windows), allow(dead_code))]
const NAP: Duration = Duration::from_micros(250);

/// How long before the deadline a high-resolution timer wakes the wait; the rest is yielded.
/// Such a timer wakes within about half a millisecond, so one millisecond keeps the deadline.
#[cfg_attr(not(windows), allow(dead_code))]
const FINE_TAIL: Duration = Duration::from_millis(1);

/// One tick of the Windows system timer at its default resolution (15.625 ms), rounded up: how
/// late a wait with a millisecond timeout can end.
#[cfg_attr(not(windows), allow(dead_code))]
const TICK: Duration = Duration::from_millis(16);

#[cfg_attr(not(windows), allow(dead_code))]
fn event_name(region: &str, role: &str) -> Vec<u16> {
    format!("Local\\{region}-{role}").encode_utf16().chain(std::iter::once(0)).collect()
}

pub(crate) struct Doorbell {
    #[cfg(windows)]
    event: windows_sys::Win32::Foundation::HANDLE,
    /// A high-resolution waitable timer for this side's waits, unnamed and not shared; `None`
    /// where Windows cannot make one.
    #[cfg(windows)]
    timer: Option<windows_sys::Win32::Foundation::HANDLE>,
}

// SAFETY: Windows event and waitable timer handles can be set, signalled and waited on from any
// thread (the doorbell is not `Sync`, so one thread at a time arms its timer); on other systems
// the doorbell holds nothing.
#[cfg(windows)]
unsafe impl Send for Doorbell {}

impl Doorbell {
    /// Creates the doorbell `role` of the link `region`.
    #[cfg_attr(not(windows), allow(unused_variables))]
    pub(crate) fn create(region: &str, role: &str) -> Result<Self, IpcError> {
        #[cfg(windows)]
        {
            use windows_sys::Win32::System::Threading::CreateEventW;
            let name = event_name(region, role);
            // SAFETY: `name` is a NUL-terminated UTF-16 string that outlives the call; no
            // security attributes; auto-reset, initially not signalled.
            let event = unsafe { CreateEventW(std::ptr::null(), 0, 0, name.as_ptr()) };
            if event.is_null() {
                return Err(std::io::Error::last_os_error().into());
            }
            Ok(Self { event, timer: high_resolution_timer() })
        }
        #[cfg(not(windows))]
        Ok(Self {})
    }

    /// Opens the doorbell `role` the other side created.
    #[cfg_attr(not(windows), allow(unused_variables))]
    pub(crate) fn open(region: &str, role: &str) -> Result<Self, IpcError> {
        #[cfg(windows)]
        {
            use windows_sys::Win32::System::Threading::{EVENT_MODIFY_STATE, OpenEventW, SYNCHRONIZATION_SYNCHRONIZE};
            let name = event_name(region, role);
            let access = EVENT_MODIFY_STATE | SYNCHRONIZATION_SYNCHRONIZE;
            // SAFETY: `name` is a NUL-terminated UTF-16 string that outlives the call.
            let event = unsafe { OpenEventW(access, 0, name.as_ptr()) };
            if event.is_null() {
                return Err(std::io::Error::last_os_error().into());
            }
            Ok(Self { event, timer: high_resolution_timer() })
        }
        #[cfg(not(windows))]
        Ok(Self {})
    }

    /// Whether waits end on a high-resolution timer (always, except on Windows that has none).
    #[cfg_attr(not(test), allow(dead_code))]
    pub(crate) fn is_high_resolution(&self) -> bool {
        #[cfg(windows)]
        let fine = self.timer.is_some();
        #[cfg(not(windows))]
        let fine = true;
        fine
    }

    /// Wakes the other side after `word` was changed.
    #[cfg_attr(not(target_os = "linux"), allow(unused_variables))]
    pub(crate) fn ring(&self, word: &AtomicU32) {
        #[cfg(windows)]
        {
            // SAFETY: `self.event` is a valid event handle until `drop`.
            unsafe { windows_sys::Win32::System::Threading::SetEvent(self.event) };
        }
        #[cfg(target_os = "linux")]
        {
            // SAFETY: FUTEX_WAKE only reads the address, which points at a live, aligned
            // `AtomicU32` in the shared mapping; the shared (non-private) futex reaches waiters
            // in the other process.
            unsafe { libc::syscall(libc::SYS_futex, word.as_ptr(), libc::FUTEX_WAKE, i32::MAX) };
        }
    }

    /// Waits until `done(word)` holds or `deadline` passes; returns whether it held.
    pub(crate) fn wait(&self, word: &AtomicU32, done: impl Fn(u32) -> bool, deadline: Instant) -> bool {
        for _ in 0..SPINS {
            if done(word.load(Ordering::Acquire)) {
                return true;
            }
            std::hint::spin_loop();
        }
        loop {
            let value = word.load(Ordering::Acquire);
            if done(value) {
                return true;
            }
            let now = Instant::now();
            if now >= deadline {
                return false;
            }
            self.sleep(word, value, deadline - now);
        }
    }

    /// Sleeps until woken, `word` stops being `value`, or `left` has passed (whichever comes
    /// first; spurious wake-ups are fine, the caller checks again).
    #[cfg_attr(not(target_os = "linux"), allow(unused_variables))]
    fn sleep(&self, word: &AtomicU32, value: u32, left: std::time::Duration) {
        #[cfg(target_os = "linux")]
        {
            let timeout = libc::timespec {
                tv_sec: left.as_secs().min(i64::MAX as u64) as libc::time_t,
                tv_nsec: libc::c_long::from(left.subsec_nanos() as i32),
            };
            // SAFETY: FUTEX_WAIT reads the aligned `AtomicU32` at the address and sleeps only
            // while it still holds `value`; the timeout points at a live `timespec`.
            unsafe { libc::syscall(libc::SYS_futex, word.as_ptr(), libc::FUTEX_WAIT, value, &raw const timeout) };
        }
        #[cfg(windows)]
        self.sleep_windows(left);
        #[cfg(not(any(target_os = "linux", windows)))]
        {
            // A short spin (a ring in it is noticed at once), then a nap, which counts as
            // waiting and keeps the thread out of the scheduler's demotion. The spin watches
            // the word, and so does the moment before the nap: a response seen is not slept
            // over. Cap the requested sleep to the remaining time; scheduling may overshoot.
            // The caller re-checks the word and deadline.
            let spin = left.min(SPIN_BEFORE_NAP);
            let started = Instant::now();
            while started.elapsed() < spin {
                if word.load(Ordering::Acquire) != value {
                    return;
                }
                std::hint::spin_loop();
            }
            let left = left.saturating_sub(started.elapsed());
            if !left.is_zero() && word.load(Ordering::Acquire) == value {
                std::thread::sleep(left.min(NAP));
            }
        }
    }

    /// [`Doorbell::sleep`] on Windows. A wait with a millisecond timeout ends on the system
    /// timer's tick, up to 15.6 ms late, so it never sleeps into the last [`FINE_TAIL`] (with the
    /// high-resolution timer) or [`TICK`] (without) before the deadline; there it yields, and
    /// the caller checks the word and the deadline again.
    #[cfg(windows)]
    fn sleep_windows(&self, left: Duration) {
        use windows_sys::Win32::System::Threading::{SetWaitableTimer, WaitForMultipleObjects};
        if let Some(timer) = self.timer
            && let Some(nap) = left.checked_sub(FINE_TAIL)
        {
            // Relative due times are negative, in 100 ns units; at least one.
            let due = -i64::try_from((nap.as_nanos() / 100).max(1)).unwrap_or(i64::MAX);
            // SAFETY: `timer` is a valid waitable timer handle until `drop`; `due` is a live
            // `i64`; no completion routine, so nothing runs on this thread.
            let armed = unsafe { SetWaitableTimer(timer, &raw const due, 0, None, std::ptr::null(), 0) };
            if armed != 0 {
                let handles = [self.event, timer];
                // A backstop in case the timer never fires; it is never shorter than the nap.
                let backstop = millis_rounded_up(left).saturating_add(1);
                // SAFETY: `handles` holds two valid handles and outlives the call; it returns
                // when either is signalled (the event: rung; the timer: the nap is over).
                let answer = unsafe { WaitForMultipleObjects(2, handles.as_ptr(), 0, backstop) };
                // A failed wait returns at once and would turn every wait into a spin (#1256):
                // the event's own timeout below waits instead.
                if answer != windows_sys::Win32::Foundation::WAIT_FAILED {
                    return;
                }
                note_failed_wait();
            }
        }
        wait_coarse(&[self.event], left, Duration::MAX);
    }
}

/// Waits on `handles` (valid event handles) with their own millisecond timeout: at most `cap`,
/// and never into the last [`TICK`] before the end of `left`, where it yields instead, since such
/// a timeout can end up to a tick late. Returns whether Windows took the wait (`false`: it
/// failed and returned at once).
#[cfg(windows)]
fn wait_coarse(handles: &[windows_sys::Win32::Foundation::HANDLE], left: Duration, cap: Duration) -> bool {
    use windows_sys::Win32::System::Threading::WaitForMultipleObjects;
    let Some(nap) = left.checked_sub(TICK) else {
        std::thread::yield_now();
        return true;
    };
    let millis = u32::try_from(nap.min(cap).as_millis()).unwrap_or(u32::MAX - 1).min(u32::MAX - 1);
    let Ok(count) = u32::try_from(handles.len()) else {
        return false;
    };
    // SAFETY: `handles` holds `count` valid event handles and outlives the call; it returns when
    // any is signalled or the timeout passes.
    let answer = unsafe { WaitForMultipleObjects(count, handles.as_ptr(), 0, millis) };
    answer != windows_sys::Win32::Foundation::WAIT_FAILED
}

/// Whether a wait on the operating system has failed since the process started, so it waited
/// on a coarser timeout instead (#1256): for a trace to say so once.
static WAIT_FAILED: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);

#[cfg_attr(not(windows), allow(dead_code))]
fn note_failed_wait() {
    WAIT_FAILED.store(true, Ordering::Relaxed);
}

/// Whether a wait on the operating system has failed in this process (it then waited on the
/// event's own, coarser timeout). Real-time safe.
pub fn a_wait_failed() -> bool {
    WAIT_FAILED.load(Ordering::Relaxed)
}

/// What one link waits for: its answer word reaching a value, and the doorbell rung when it
/// changes. Taken from a link ([`crate::Link::watch`]) to wait for several at once
/// ([`wait_any`]).
#[derive(Clone, Copy)]
pub struct Watch<'a> {
    word: &'a AtomicU32,
    value: u32,
    bell: &'a Doorbell,
}

impl<'a> Watch<'a> {
    pub(crate) fn new(word: &'a AtomicU32, value: u32, bell: &'a Doorbell) -> Self {
        Self { word, value, bell }
    }

    fn answered(&self) -> bool {
        self.word.load(Ordering::Acquire) == self.value
    }
}

/// The most links one Windows wait takes, besides the timer (`MAXIMUM_WAIT_OBJECTS` less one).
#[cfg_attr(not(windows), allow(dead_code))]
const MAX_WAITED: usize = 63;

/// How often a Linux wait over several links looks at all of them again: it sleeps on the first
/// link's futex, which an answer on another link does not wake, so that answer is seen at most
/// this late (plus the scheduler's wake-up).
#[cfg_attr(windows, allow(dead_code))]
const RECHECK: Duration = Duration::from_micros(100);

/// How often a Windows wait over more links than one wait takes ([`MAX_WAITED`]) looks at all of
/// them again: the ones left out cannot wake it. A high-resolution timer wakes within about half
/// a millisecond, so a shorter nap would end up yielding.
#[cfg_attr(not(windows), allow(dead_code))]
const RECHECK_OMITTED: Duration = Duration::from_millis(1);

/// Waits until any of `watches` is answered or `until` passes (#1256): the first answered one's
/// index, or `None` at `until`. It spins briefly first, as [`Doorbell::wait`] does, then sleeps
/// in the operating system: on Windows on the links' events together (the first
/// [`MAX_WAITED`]; when there are more, at most [`RECHECK_OMITTED`] at a time, so the others
/// are looked at that often), with the first one's high-resolution timer, and yielding in the
/// last [`FINE_TAIL`] (without that timer, or when Windows refuses the wait, as
/// `sleep_any_windows` says); elsewhere on the first link, as long as [`Doorbell::wait`]
/// would for one link, and on Linux at most [`RECHECK`] at a time for several (macOS looks at
/// all of them after each nap anyway). `None` entries are passed over. Never allocates; never waits past
/// `until`.
pub fn wait_any(watches: &[Option<Watch>], until: Instant) -> Option<usize> {
    let answered = || watches.iter().position(|w| w.is_some_and(|w| w.answered()));
    for _ in 0..SPINS {
        if let Some(i) = answered() {
            return Some(i);
        }
        std::hint::spin_loop();
    }
    loop {
        if let Some(i) = answered() {
            return Some(i);
        }
        let now = Instant::now();
        if now >= until {
            return None;
        }
        let first = watches.iter().flatten().next()?;
        #[cfg(windows)]
        sleep_any_windows(watches, first, until - now);
        #[cfg(not(windows))]
        {
            // The answer may have come since the look above: the futex would sleep on it, and
            // the ring it had is gone.
            let value = first.word.load(Ordering::Acquire);
            if value == first.value {
                continue;
            }
            let left = until - now;
            let several = watches.iter().flatten().nth(1).is_some();
            let left = if cfg!(target_os = "linux") && several { left.min(RECHECK) } else { left };
            first.bell.sleep(first.word, value, left);
        }
    }
}

/// [`wait_any`]'s sleep on Windows: the links' events and `first`'s timer together, as
/// [`Doorbell::sleep_windows`] waits on one. Over several links every way it sleeps wakes for
/// any of the first [`MAX_WAITED`] or ends within [`RECHECK_OMITTED`] (without a
/// high-resolution timer, on the system timer's next tick) so the caller looks at all of them:
/// without a timer, when it cannot be armed and in the last stretch it waits on the events with
/// their own timeout, and when Windows refuses a wait on them it waits on the first event alone,
/// a recheck at a time.
#[cfg(windows)]
fn sleep_any_windows(watches: &[Option<Watch>], first: &Watch, left: Duration) {
    use windows_sys::Win32::Foundation::{HANDLE, WAIT_FAILED};
    use windows_sys::Win32::System::Threading::{SetWaitableTimer, WaitForMultipleObjects};
    if watches.iter().flatten().nth(1).is_none() {
        // One link: as its own wait sleeps.
        first.bell.sleep_windows(left);
        return;
    }
    // Links past the first `MAX_WAITED` cannot wake the wait: it ends soon to look at them.
    let omitted = watches.iter().flatten().nth(MAX_WAITED).is_some();
    let mut handles: [HANDLE; MAX_WAITED + 1] = [std::ptr::null_mut(); MAX_WAITED + 1];
    let mut events = 0;
    for watch in watches.iter().flatten().take(MAX_WAITED) {
        if let Some(slot) = handles.get_mut(events) {
            *slot = watch.bell.event;
            events += 1;
        }
    }
    let failed = || {
        // The same handles would be refused again: the first link's event, a recheck at a time.
        note_failed_wait();
        wait_coarse(&[first.bell.event], left, RECHECK_OMITTED);
    };
    if let (Some(timer), Some(nap)) = (first.bell.timer, left.checked_sub(FINE_TAIL)) {
        let nap = if omitted { nap.min(RECHECK_OMITTED) } else { nap };
        let due = -i64::try_from((nap.as_nanos() / 100).max(1)).unwrap_or(i64::MAX);
        // SAFETY: `timer` is a valid waitable timer handle while its doorbell lives, which the
        // watch borrows; `due` is a live `i64`; no completion routine.
        let armed = unsafe { SetWaitableTimer(timer, &raw const due, 0, None, std::ptr::null(), 0) };
        if armed != 0
            && let Some(slot) = handles.get_mut(events)
        {
            *slot = timer;
            let backstop = millis_rounded_up(left).saturating_add(1);
            // SAFETY: the first `events + 1` handles are valid (the watched links' events and the
            // timer) and outlive the call; it returns when any is signalled.
            let answer = unsafe { WaitForMultipleObjects(events as u32 + 1, handles.as_ptr(), 0, backstop) };
            if answer == WAIT_FAILED {
                failed();
            }
            return;
        }
    }
    // No timer, one that cannot be armed, or the last stretch: the events' own timeout.
    let cap = if omitted { RECHECK_OMITTED } else { Duration::MAX };
    if !wait_coarse(handles.get(..events).unwrap_or(&[]), left, cap) {
        failed();
    }
}

/// Milliseconds, rounded up, short of `INFINITE`.
#[cfg(windows)]
fn millis_rounded_up(d: Duration) -> u32 {
    u32::try_from(d.as_micros().div_ceil(1_000)).unwrap_or(u32::MAX - 1).min(u32::MAX - 1)
}

/// A high-resolution waitable timer (Windows 10 1803 and later), auto-reset; `None` where
/// Windows cannot make one.
#[cfg(windows)]
fn high_resolution_timer() -> Option<windows_sys::Win32::Foundation::HANDLE> {
    use windows_sys::Win32::System::Threading::{
        CREATE_WAITABLE_TIMER_HIGH_RESOLUTION, CreateWaitableTimerExW, TIMER_ALL_ACCESS,
    };
    // SAFETY: no security attributes and no name (both may be null); the flag asks for a
    // high-resolution timer, which older Windows refuses with a null handle.
    let timer = unsafe {
        CreateWaitableTimerExW(
            std::ptr::null(),
            std::ptr::null(),
            CREATE_WAITABLE_TIMER_HIGH_RESOLUTION,
            TIMER_ALL_ACCESS,
        )
    };
    (!timer.is_null()).then_some(timer)
}

#[cfg(windows)]
impl Drop for Doorbell {
    fn drop(&mut self) {
        // SAFETY: the handle came from CreateEventW or OpenEventW and is closed only here.
        unsafe { windows_sys::Win32::Foundation::CloseHandle(self.event) };
        if let Some(timer) = self.timer {
            // SAFETY: the handle came from CreateWaitableTimerExW and is closed only here.
            unsafe { windows_sys::Win32::Foundation::CloseHandle(timer) };
        }
    }
}

#[cfg(test)]
#[path = "doorbell_tests.rs"]
mod tests;
