// Copyright (C) 2026 The Suhina contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! The time the plug-in deadlines are read from. In a build it is `Instant::now()`, inlined; the
//! engine's own tests can instead run it by hand (`Manual`), so a test of what happens when a
//! plug-in answers late or never needs no real waiting and gives the same result on a slow
//! machine as on a fast one. The manual clock exists only under `cfg(test)`: no setting, command
//! or message can reach it, and a shipped build cannot contain it.

use std::time::Instant;

/// The current time for a deadline or a plug-in round trip.
#[cfg(not(test))]
#[inline(always)]
pub(crate) fn now() -> Instant {
    Instant::now()
}

#[cfg(test)]
pub(crate) fn now() -> Instant {
    Manual::read().unwrap_or_else(Instant::now)
}

/// A clock that moves only when told to, for the current thread (each test has its own), and for
/// the render workers while they run that thread's chains ([`enter`]).
#[cfg(test)]
pub(crate) struct Manual;

/// A manual clock's time: where it started and how far it has moved since, shared by the test's
/// thread and any worker running its chains.
#[cfg(test)]
pub(crate) struct ManualTime {
    base: Instant,
    moved_ns: std::sync::atomic::AtomicU64,
}

#[cfg(test)]
thread_local! {
    static MANUAL: std::cell::RefCell<Option<std::sync::Arc<ManualTime>>> = const { std::cell::RefCell::new(None) };
}

/// The clock a thread reads, to hand to another thread that runs its work ([`enter`]): the
/// manual one under test, nothing in a build.
#[cfg(test)]
pub(crate) type Handle = Option<std::sync::Arc<ManualTime>>;
#[cfg(not(test))]
pub(crate) type Handle = ();

/// This thread's clock ([`Handle`]). Never allocates.
#[cfg(test)]
pub(crate) fn handle() -> Handle {
    MANUAL.with(|t| t.borrow().clone())
}
#[cfg(not(test))]
#[inline(always)]
pub(crate) fn handle() -> Handle {}

/// Makes this thread read `handle`'s clock: a render worker does, before it runs a chain sent
/// from another thread, so a test's manual clock holds there too. Nothing in a build.
#[cfg(test)]
pub(crate) fn enter(handle: &Handle) {
    MANUAL.with(|t| {
        let mut current = t.borrow_mut();
        let same = match (current.as_ref(), handle.as_ref()) {
            (Some(a), Some(b)) => std::sync::Arc::ptr_eq(a, b),
            (None, None) => true,
            _ => false,
        };
        if !same {
            *current = handle.clone();
        }
    });
}
#[cfg(not(test))]
#[inline(always)]
pub(crate) fn enter(_handle: &Handle) {}

#[cfg(test)]
impl Manual {
    /// Starts the manual clock on this thread, at the real time now; it stands still from here.
    pub(crate) fn start() {
        let time = ManualTime { base: Instant::now(), moved_ns: std::sync::atomic::AtomicU64::new(0) };
        MANUAL.with(|t| *t.borrow_mut() = Some(std::sync::Arc::new(time)));
    }

    fn read() -> Option<Instant> {
        MANUAL.with(|t| {
            t.borrow().as_ref().map(|time| {
                let moved = time.moved_ns.load(std::sync::atomic::Ordering::Acquire);
                time.base + std::time::Duration::from_nanos(moved)
            })
        })
    }

    /// Moves the clock forward to `at`; a time already past leaves it where it is.
    pub(crate) fn advance_to(at: Instant) {
        MANUAL.with(|t| {
            if let Some(time) = t.borrow().as_ref() {
                let moved = u64::try_from(at.saturating_duration_since(time.base).as_nanos()).unwrap_or(u64::MAX);
                time.moved_ns.fetch_max(moved, std::sync::atomic::Ordering::AcqRel);
            }
        });
    }
}
