// Copyright (C) 2026 The Suhina contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! Real-time priority for the threads that make sound: the desktop app's render thread and the
//! plug-in host's audio threads (ADR 0006, decision 4).
//!
//! [`RealTime::join`] raises the calling thread as far as the system lets an ordinary user's
//! process go, and keeps it there while the value lives:
//!
//! - **Windows:** the thread joins the MMCSS "Pro Audio" task (`audio_thread_priority`), which
//!   schedules it time-critically, gives it a fine timer and bounds the share of a core it may
//!   take. Where MMCSS refuses, the highest thread priority.
//! - **Linux:** `SCHED_FIFO` set directly, which works when the user has real-time rights
//!   (`rtprio` in `limits.conf`, the `audio` or `realtime` group); otherwise rtkit, the system
//!   service that grants real-time scheduling (`SCHED_RR`) to unprivileged processes, asked over
//!   the system D-Bus (rtkit requires `RLIMIT_RTTIME`, which is set to its maximum first);
//!   otherwise the highest normal priority, and otherwise the normal one. Both at priority 10.
//! - **Elsewhere:** the highest priority the system gives.
//!
//! rtkit's grant comes with a limit: a thread that runs longer than `RLIMIT_RTTIME` (200 ms by
//! default) without waiting ends the process. A thread that renders in a loop says when it waits
//! and how long each piece of work took ([`RealTime::waited`], [`RealTime::worked`]): it then
//! rests a moment before reaching the limit, and steps down to normal priority when one piece
//! alone takes a quarter of it, so an overloaded project drops out rather than ends Suhina.
//!
//! Nothing here fails: a thread that cannot be raised plays at the priority it has, and
//! [`RealTime::notice`] says why, for the caller to tell the user once. The work is done when
//! the thread starts, before it renders anything; none of it runs inside an audio callback.
//! `SUHINA_NO_REALTIME=1` leaves the thread at its normal priority, to compare.
//!
//! In detail: Real-time priority for the threads that make sound, the render thread and the plug-in
//! hosts' audio threads (`RealTime::join`): MMCSS "Pro Audio" on Windows; on Linux `SCHED_FIFO`
//! where the user has real-time rights, otherwise rtkit over the system D-Bus (`zbus`, pure Rust,
//! so no libdbus), otherwise the highest normal priority; the highest priority elsewhere. Says what
//! it got, and why real-time scheduling was refused; never fails. Under rtkit, whose grant ends the
//! process when a thread runs `RLIMIT_RTTIME` without waiting, it paces a rendering thread
//! (`waited`, `worked`): a short rest before the limit, and normal priority after a block that
//! takes a quarter of it. No `unsafe`.

use std::time::Duration;

#[cfg(target_os = "linux")]
mod linux;

/// How long a thread rests when it has run long enough without waiting: any sleep counts as
/// waiting for `RLIMIT_RTTIME`, and this one is far below a block's time.
const REST: Duration = Duration::from_micros(50);

/// The environment variable that turns real-time priority off when it is `1`.
pub const NO_REALTIME: &str = "SUHINA_NO_REALTIME";

/// The priority a thread got.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Priority {
    /// Left at the normal priority on purpose (`SUHINA_NO_REALTIME=1`).
    Off,
    /// The normal priority: nothing higher was allowed.
    Normal,
    /// The highest priority among ordinary threads, not real-time scheduling.
    Highest,
    /// The MMCSS "Pro Audio" task (Windows).
    Mmcss,
    /// `SCHED_FIFO`, set by the process itself with the user's real-time rights (Linux).
    Fifo,
    /// `SCHED_RR`, granted by rtkit (Linux).
    Rtkit,
}

impl Priority {
    /// Whether the thread is scheduled in real time: ahead of every ordinary thread.
    pub fn is_real_time(self) -> bool {
        matches!(self, Self::Mmcss | Self::Fifo | Self::Rtkit)
    }

    /// The priority in a few words, for a status line or a log.
    pub fn describe(self) -> &'static str {
        match self {
            Self::Off => "normal priority (SUHINA_NO_REALTIME)",
            Self::Normal => "normal priority",
            Self::Highest => "highest normal priority",
            Self::Mmcss => "real-time priority (MMCSS Pro Audio)",
            Self::Fifo => "real-time priority (SCHED_FIFO)",
            Self::Rtkit => "real-time priority (SCHED_RR through rtkit)",
        }
    }
}

/// Keeps the calling thread at the priority [`RealTime::join`] got for it while it lives.
pub struct RealTime {
    priority: Priority,
    /// Why real-time scheduling was not had, when it was tried and refused.
    refused: Option<String>,
    /// The CPU time the thread may take without waiting before the kernel ends the process
    /// (`RLIMIT_RTTIME`, with rtkit's grant); `None` when there is no such limit.
    budget: Option<Duration>,
    /// Work since the thread last waited, counted against `budget`.
    running: Duration,
    /// The MMCSS registration, which lasts until the thread ends; the handle only undoes it early.
    #[cfg(windows)]
    _mmcss: Option<audio_thread_priority::RtPriorityHandle>,
}

impl RealTime {
    /// Raises the calling thread, which renders blocks of `buffer_frames` at `sample_rate`
    /// (MMCSS sizes its share of the core from them), as far as it is allowed. Never fails.
    pub fn join(buffer_frames: usize, sample_rate: u32) -> Self {
        if turned_off(std::env::var_os(NO_REALTIME).as_deref()) {
            return Self::with(Priority::Off, None);
        }
        Self::raise(buffer_frames, sample_rate)
    }

    #[cfg(windows)]
    fn raise(buffer_frames: usize, sample_rate: u32) -> Self {
        let frames = u32::try_from(buffer_frames).unwrap_or(u32::MAX);
        match audio_thread_priority::promote_current_thread_to_real_time(frames, sample_rate) {
            Ok(handle) => Self { _mmcss: Some(handle), ..Self::with(Priority::Mmcss, None) },
            Err(e) => Self::highest(Some(format!("MMCSS refused the thread: {e}"))),
        }
    }

    #[cfg(target_os = "linux")]
    fn raise(buffer_frames: usize, sample_rate: u32) -> Self {
        let _ = (buffer_frames, sample_rate);
        let fifo = match linux::fifo() {
            Ok(()) => return Self::with(Priority::Fifo, None),
            Err(e) => e,
        };
        match linux::rtkit() {
            Ok(budget) => Self { budget: Some(budget), ..Self::with(Priority::Rtkit, None) },
            Err(rtkit) => Self::highest(Some(format!("SCHED_FIFO: {fifo}; rtkit: {rtkit}"))),
        }
    }

    #[cfg(not(any(windows, target_os = "linux")))]
    fn raise(buffer_frames: usize, sample_rate: u32) -> Self {
        let _ = (buffer_frames, sample_rate);
        Self::highest(None)
    }

    /// The highest ordinary priority, or the normal one where even that is not allowed.
    fn highest(refused: Option<String>) -> Self {
        let priority = match thread_priority::set_current_thread_priority(thread_priority::ThreadPriority::Max) {
            Ok(()) => Priority::Highest,
            Err(_) => Priority::Normal,
        };
        Self::with(priority, refused)
    }

    fn with(priority: Priority, refused: Option<String>) -> Self {
        Self {
            priority,
            refused,
            budget: None,
            running: Duration::ZERO,
            #[cfg(windows)]
            _mmcss: None,
        }
    }

    /// Tells it the thread has just waited (parked, slept, waited for an answer): the time it
    /// may run without waiting starts again.
    pub fn waited(&mut self) {
        self.running = Duration::ZERO;
    }

    /// Tells it the thread has just done a piece of work that took `took` without waiting. Where
    /// the kernel limits that time (rtkit), it rests a moment when the thread nears the limit,
    /// and steps down to normal priority for good when this one piece took a quarter of it, as
    /// the next could take all of it. Elsewhere it does nothing. No allocation.
    pub fn worked(&mut self, took: Duration) {
        self.running = self.running.saturating_add(took);
        match pace(self.budget, self.running, took) {
            Pace::Go => {}
            Pace::Rest => {
                std::thread::sleep(REST);
                self.running = Duration::ZERO;
            }
            Pace::StepDown => self.step_down(),
        }
    }

    #[cfg(target_os = "linux")]
    fn step_down(&mut self) {
        // Leaving real-time scheduling is always allowed; if it failed, resting keeps it alive.
        if linux::normal().is_ok() {
            self.priority = Priority::Normal;
            self.budget = None;
        }
    }

    #[cfg(not(target_os = "linux"))]
    fn step_down(&mut self) {
        self.budget = None;
    }

    /// The priority the thread got.
    pub fn priority(&self) -> Priority {
        self.priority
    }

    /// Why real-time scheduling was not had, when it was tried and refused.
    pub fn refused(&self) -> Option<&str> {
        self.refused.as_deref()
    }

    /// What a user should know when real-time scheduling was refused: the sound may drop out
    /// while the computer is busy. `None` when it was had, or was not asked for.
    pub fn notice(&self) -> Option<String> {
        let why = self.refused.as_deref()?;
        Some(format!(
            "the audio runs at {} without real-time scheduling ({why}), so it may drop out while the computer is busy",
            self.priority.describe()
        ))
    }
}

/// What a thread does after a piece of work, by [`pace`].
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Pace {
    Go,
    Rest,
    StepDown,
}

/// After a piece of work that took `took`, with `running` the work since the thread last waited:
/// within `budget`, the time it may run without waiting, it goes on until an eighth of it, then
/// rests; a single piece over a quarter of it steps down. Work never reaches the budget: an
/// eighth plus less than a quarter.
fn pace(budget: Option<Duration>, running: Duration, took: Duration) -> Pace {
    let Some(budget) = budget else { return Pace::Go };
    if took >= budget / 4 {
        Pace::StepDown
    } else if running >= budget / 8 {
        Pace::Rest
    } else {
        Pace::Go
    }
}

/// Whether `SUHINA_NO_REALTIME` has the value that turns real-time priority off.
fn turned_off(value: Option<&std::ffi::OsStr>) -> bool {
    value.is_some_and(|v| v == "1")
}

#[cfg(test)]
mod tests;
