// Copyright (C) 2026 The Suhina contributors
// SPDX-License-Identifier: GPL-3.0-or-later

use super::*;

#[test]
fn turned_off_only_by_one() {
    assert!(turned_off(Some(std::ffi::OsStr::new("1"))));
    assert!(!turned_off(Some(std::ffi::OsStr::new("0"))));
    assert!(!turned_off(Some(std::ffi::OsStr::new(""))));
    assert!(!turned_off(None));
}

#[test]
fn real_time_priorities() {
    for priority in [Priority::Mmcss, Priority::Fifo, Priority::Rtkit] {
        assert!(priority.is_real_time(), "{priority:?}");
    }
    for priority in [Priority::Off, Priority::Normal, Priority::Highest] {
        assert!(!priority.is_real_time(), "{priority:?}");
    }
}

#[test]
fn notice_only_when_refused() {
    let off = RealTime::with(Priority::Off, None);
    assert_eq!(off.notice(), None);
    let had = RealTime::with(Priority::Rtkit, None);
    assert_eq!(had.notice(), None);
    let refused = RealTime::with(Priority::Normal, Some("rtkit: not there".into()));
    let notice = refused.notice().unwrap_or_default();
    assert!(notice.contains("normal priority"), "{notice}");
    assert!(notice.contains("rtkit: not there"), "{notice}");
}

/// Joining never fails or hangs, whatever the system allows (here: no real-time rights and no
/// rtkit in a container, or both on a desktop), and says why when it is not real time.
#[test]
fn join_always_returns() {
    let joined = std::thread::spawn(|| {
        let realtime = RealTime::join(256, 48_000);
        (realtime.priority(), realtime.refused().map(str::to_owned), realtime.notice())
    })
    .join();
    let Ok((priority, refused, notice)) = joined else {
        panic!("joining real-time priority panicked");
    };
    if std::env::var_os(NO_REALTIME).is_some_and(|v| v == "1") {
        assert_eq!(priority, Priority::Off);
        return;
    }
    #[cfg(any(windows, target_os = "linux"))]
    assert_eq!(priority.is_real_time(), refused.is_none(), "{priority:?} {refused:?}");
    assert_eq!(refused.is_some(), notice.is_some());
}

#[cfg(target_os = "linux")]
mod linux {
    use rustix::process::Rlimit;

    use crate::linux::{RT_PRIORITY, rtkit_priority, rttime_limit};

    #[test]
    fn rtkit_priority_within_its_ceiling() {
        assert_eq!(rtkit_priority(20), Some(u32::from(RT_PRIORITY)));
        assert_eq!(rtkit_priority(5), Some(5));
        assert_eq!(rtkit_priority(1), Some(1));
        assert_eq!(rtkit_priority(0), None);
        assert_eq!(rtkit_priority(-3), None);
    }

    #[test]
    fn rttime_within_rtkits_maximum() {
        let unlimited = Rlimit { current: None, maximum: None };
        assert_eq!(rttime_limit(unlimited, 200_000), Some(Rlimit { current: Some(200_000), maximum: Some(200_000) }));
        // A lower limit is kept, a higher one lowered.
        let low = Rlimit { current: Some(50_000), maximum: Some(100_000) };
        assert_eq!(rttime_limit(low, 200_000), Some(low));
        let high = Rlimit { current: Some(500_000), maximum: None };
        assert_eq!(rttime_limit(high, 200_000), Some(Rlimit { current: Some(200_000), maximum: Some(200_000) }));
        // The soft limit never above the hard one.
        let soft_over = Rlimit { current: None, maximum: Some(100_000) };
        assert_eq!(rttime_limit(soft_over, 200_000), Some(Rlimit { current: Some(100_000), maximum: Some(100_000) }));
        // rtkit allowing no time at all, or a hostile answer.
        assert_eq!(rttime_limit(unlimited, 0), None);
        assert_eq!(rttime_limit(unlimited, -1), None);
    }

    /// rtkit is not in the container, so asking it fails quickly and says so; on a desktop it
    /// may grant the thread instead. Either way it answers, at the latest when its wait is over.
    #[test]
    fn rtkit_answers() {
        let answer = std::thread::spawn(crate::linux::rtkit).join();
        assert!(answer.is_ok());
    }
}

#[test]
fn pacing_keeps_work_within_the_budget() {
    let ms = Duration::from_millis;
    // No limit (MMCSS, SCHED_FIFO, normal): always on.
    assert_eq!(pace(None, ms(10_000), ms(5_000)), Pace::Go);
    // rtkit's 200 ms: on until 25 ms without waiting, then a rest; a 50 ms block steps down.
    let budget = Some(ms(200));
    assert_eq!(pace(budget, ms(24), ms(5)), Pace::Go);
    assert_eq!(pace(budget, ms(25), ms(5)), Pace::Rest);
    assert_eq!(pace(budget, ms(49), ms(49)), Pace::Rest, "under a quarter: a rest");
    assert_eq!(pace(budget, ms(50), ms(50)), Pace::StepDown);
    // A budget of nothing steps down at once.
    assert_eq!(pace(Some(Duration::ZERO), Duration::ZERO, Duration::ZERO), Pace::StepDown);
}

/// A thread that keeps working without waiting never reaches its budget: it rests in time.
#[test]
fn a_thread_that_never_waits_rests_in_time() {
    let mut realtime = RealTime { budget: Some(Duration::from_millis(200)), ..RealTime::with(Priority::Rtkit, None) };
    let mut since_rest = Duration::ZERO;
    for _ in 0..1000 {
        realtime.worked(Duration::from_millis(3));
        since_rest += Duration::from_millis(3);
        if realtime.running.is_zero() {
            since_rest = Duration::ZERO;
        }
        assert!(since_rest < Duration::from_millis(200));
    }
    assert_eq!(realtime.priority(), Priority::Rtkit, "small pieces never step down");
    realtime.waited();
    assert!(realtime.running.is_zero());
}

/// One piece of work that takes a quarter of the budget leaves real-time scheduling for good.
#[test]
fn a_long_block_steps_down() {
    let joined = std::thread::spawn(|| {
        let mut realtime =
            RealTime { budget: Some(Duration::from_millis(200)), ..RealTime::with(Priority::Rtkit, None) };
        realtime.worked(Duration::from_millis(60));
        (realtime.budget, realtime.priority())
    })
    .join();
    let Ok((budget, priority)) = joined else { panic!("stepping down panicked") };
    assert_eq!(budget, None, "no more pacing once stepped down");
    #[cfg(target_os = "linux")]
    assert_eq!(priority, Priority::Normal);
    let _ = priority;
}
