// Copyright (C) 2026 The Suhina contributors
// SPDX-License-Identifier: GPL-3.0-or-later

use super::*;

/// A doorbell nobody rings: each wait must time out, never before its deadline.
#[test]
fn an_unanswered_wait_ends_at_its_deadline() {
    // How late it ends is for an idle host: shared runners can deschedule this thread for
    // arbitrarily longer than the audio deadline (`unanswered_wait_precision`).
    unanswered_wait_lateness("deadline");
}

/// Measures timer precision on an idle host, outside shared-runner CI.
#[test]
#[ignore = "timing measurement; run on an idle host with --ignored --test-threads=1"]
fn unanswered_wait_precision() {
    let mut late = unanswered_wait_lateness("precision");
    late.sort();
    let worst = late.last().copied().unwrap_or_default();
    let median = late.get(late.len() / 2).copied().unwrap_or_default();
    if cfg!(any(target_os = "linux", windows)) {
        assert!(median < Duration::from_millis(1), "median {median:?}, all {late:?}");
        assert!(worst < Duration::from_millis(40), "worst {worst:?}, all {late:?}");
    } else {
        assert!(worst < Duration::from_millis(500), "worst {worst:?}, all {late:?}");
    }
}

/// Samples the real wait and checks timeout semantics at block-sized deadlines.
fn unanswered_wait_lateness(role: &str) -> Vec<Duration> {
    let name = format!("suhina-ipc-doorbell-test-{}", std::process::id());
    let bell = Doorbell::create(&name, role).unwrap_or_else(|e| panic!("{e}"));
    let word = AtomicU32::new(0);
    let mut late = Vec::new();
    for n in 0..40u64 {
        let timeout = Duration::from_micros(1_000 + n * 300);
        let deadline = Instant::now() + timeout;
        assert!(!bell.wait(&word, |v| v != 0, deadline));
        let ended = Instant::now();
        assert!(ended >= deadline, "ended {:?} early", deadline - ended);
        late.push(ended - deadline);
    }
    late
}

/// A ring wakes a sleeping wait. Its deadline is an hour away, so only the ring can end it
/// within the minute it is given here: a wait that sleeps on reads the changed word at its
/// deadline and says yes all the same, so the answer alone would not show a ring that failed.
#[test]
fn a_ring_wakes_the_wait() {
    let name = format!("suhina-ipc-doorbell-ring-{}", std::process::id());
    let bell = Doorbell::create(&name, "test").unwrap_or_else(|e| panic!("{e}"));
    let other = Doorbell::open(&name, "test").unwrap_or_else(|e| panic!("{e}"));
    let word = std::sync::Arc::new(AtomicU32::new(0));
    let (sleeping, going_to_sleep) = std::sync::mpsc::channel();
    let (answer, answered) = std::sync::mpsc::channel();
    let waiter = {
        let word = word.clone();
        std::thread::spawn(move || {
            let looks = AtomicU32::new(0);
            let done = |value: u32| {
                // The look after the spins is the last one before the wait sleeps.
                if looks.fetch_add(1, Ordering::Relaxed) == SPINS {
                    let _ = sleeping.send(());
                }
                value == 1
            };
            let _ = answer.send(bell.wait(&word, done, Instant::now() + Duration::from_secs(3600)));
        })
    };
    assert!(going_to_sleep.recv().is_ok());
    word.store(1, Ordering::Release);
    other.ring(&word);
    assert_eq!(answered.recv_timeout(Duration::from_secs(60)), Ok(true), "the ring woke nothing");
    assert!(waiter.join().is_ok());
}

/// The high-resolution timer exists on every Windows the project supports (10 1803 and
/// later), so CI checks the fine path rather than the fallback.
#[test]
fn waits_are_high_resolution() {
    let name = format!("suhina-ipc-doorbell-hr-{}", std::process::id());
    let bell = Doorbell::create(&name, "test").unwrap_or_else(|e| panic!("{e}"));
    assert!(bell.is_high_resolution());
}

/// A wait over `count` links, the first never answered while it waits and the last answered
/// (and rung) once the wait is under way: the last one's answer ends it, however long the
/// first is out (#1256). `prepare` changes the bells before the wait: on Windows, to take
/// the way the wait sleeps away from the usual one. Its `until` is an hour away, and a
/// watchdog answers the first link after a minute, so a wait that only the first link could
/// wake returns the first link instead.
fn a_wait_wakes_for_the_last_bell_of(count: usize, tag: &str, prepare: impl FnOnce(&mut [Doorbell])) {
    let name = format!("suhina-ipc-doorbell-any-{tag}-{}", std::process::id());
    let open = |i: usize| Doorbell::open(&name, &i.to_string()).unwrap_or_else(|e| panic!("{e}"));
    let mut bells: Vec<Doorbell> =
        (0..count).map(|i| Doorbell::create(&name, &i.to_string()).unwrap_or_else(|e| panic!("{e}"))).collect();
    let (first_ring, last_ring) = (open(0), open(count - 1));
    prepare(&mut bells);
    let words: std::sync::Arc<Vec<AtomicU32>> = std::sync::Arc::new((0..count).map(|_| AtomicU32::new(0)).collect());
    let answer = |ring: Doorbell, at: usize, hold: Box<dyn FnOnce() + Send>| {
        let words = words.clone();
        std::thread::spawn(move || {
            hold();
            if let Some(word) = words.get(at) {
                word.store(1, Ordering::Release);
                ring.ring(word);
            }
        })
    };
    let (waited, watchdog) = std::sync::mpsc::channel::<()>();
    let watchdog = answer(
        first_ring,
        0,
        Box::new(move || {
            let _ = watchdog.recv_timeout(Duration::from_secs(60));
        }),
    );
    let (go, hold) = std::sync::mpsc::channel::<()>();
    let last = answer(
        last_ring,
        count - 1,
        Box::new(move || {
            let _ = hold.recv();
        }),
    );
    let watches: Vec<Option<Watch>> = words.iter().zip(&bells).map(|(w, b)| Some(Watch::new(w, 1, b))).collect();
    assert!(go.send(()).is_ok());
    let answered = wait_any(&watches, Instant::now() + Duration::from_secs(3600));
    let _ = waited.send(());
    assert_eq!(answered, Some(count - 1), "only the first link's answer woke the wait");
    assert!(last.join().is_ok() && watchdog.join().is_ok());
}

/// Takes away each bell's high-resolution timer, as Windows before 10 1803 has none.
fn without_timers(bells: &mut [Doorbell]) {
    #[cfg(windows)]
    for bell in bells {
        if let Some(timer) = bell.timer.take() {
            // SAFETY: the handle came from CreateWaitableTimerExW and, taken out of the
            // doorbell, is closed only here.
            unsafe { windows_sys::Win32::Foundation::CloseHandle(timer) };
        }
    }
    #[cfg(not(windows))]
    let _ = bells;
}

/// Without a high-resolution timer the wait sleeps on the events' own timeout: on all of
/// them, not the first alone.
#[test]
fn a_wait_without_a_timer_wakes_for_the_second_link() {
    a_wait_wakes_for_the_last_bell_of(2, "untimed-2", without_timers);
}

/// Without a high-resolution timer, a link past the first [`MAX_WAITED`] is looked at all
/// the same.
#[test]
fn a_wait_without_a_timer_looks_at_a_link_left_out() {
    a_wait_wakes_for_the_last_bell_of(MAX_WAITED + 1, "untimed-64", without_timers);
}

/// A wait Windows refuses (here for a bell whose event is no handle at all) falls back to
/// the first link's event a recheck at a time, so the others are still looked at.
#[cfg(windows)]
#[test]
fn a_refused_wait_over_several_links_looks_at_them_all() {
    for (tag, timed) in [("refused-timed", true), ("refused-untimed", false)] {
        a_wait_wakes_for_the_last_bell_of(3, tag, |bells| {
            if !timed {
                without_timers(bells);
            }
            if let Some(bell) = bells.get_mut(1) {
                let event = std::mem::replace(&mut bell.event, std::ptr::null_mut());
                // SAFETY: the handle came from CreateEventW and, replaced in the doorbell
                // by a null one that its drop closes harmlessly, is closed only here.
                unsafe { windows_sys::Win32::Foundation::CloseHandle(event) };
            }
        });
    }
    assert!(a_wait_failed());
}

#[cfg(windows)]
#[test]
fn milliseconds_round_up_and_stay_short_of_infinite() {
    assert_eq!(millis_rounded_up(Duration::ZERO), 0);
    assert_eq!(millis_rounded_up(Duration::from_micros(1)), 1);
    assert_eq!(millis_rounded_up(Duration::from_millis(10)), 10);
    assert_eq!(millis_rounded_up(Duration::MAX), u32::MAX - 1);
}
