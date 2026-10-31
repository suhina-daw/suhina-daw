// Copyright (C) 2026 The Suhina contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! The time the plug-in deadlines are read from. In a build it is `Instant::now()`, inlined; the
//! engine's own tests can instead run it by hand ([`Manual`]), so a test of what happens when a
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

/// A clock that moves only when told to, for the current thread (each test has its own).
#[cfg(test)]
pub(crate) struct Manual;

#[cfg(test)]
thread_local! {
    static MANUAL: std::cell::Cell<Option<Instant>> = const { std::cell::Cell::new(None) };
}

#[cfg(test)]
impl Manual {
    /// Starts the manual clock on this thread, at the real time now; it stands still from here.
    pub(crate) fn start() {
        MANUAL.with(|t| t.set(Some(Instant::now())));
    }

    fn read() -> Option<Instant> {
        MANUAL.with(std::cell::Cell::get)
    }

    /// Moves the clock forward to `at`; a time already past leaves it where it is.
    pub(crate) fn advance_to(at: Instant) {
        MANUAL.with(|t| {
            if t.get().is_some_and(|now| at > now) {
                t.set(Some(at));
            }
        });
    }
}
