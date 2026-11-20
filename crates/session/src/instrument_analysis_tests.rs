// Copyright (C) 2026 The Suhina contributors
// SPDX-License-Identifier: GPL-3.0-or-later

#![allow(clippy::unwrap_used)]
use super::*;

#[test]
fn admission_and_late_completion_have_no_timing_dependency() {
    let first = Permit::acquire().unwrap();
    let second = Permit::acquire().unwrap();
    assert!(Permit::acquire().is_err());
    drop(first);
    let replacement = Permit::acquire().unwrap();
    assert!(Permit::acquire().is_err());
    drop(second);
    drop(replacement);

    let (tx, rx) = mpsc::sync_channel(1);
    let flag = Arc::new(AtomicBool::new(false));
    let mut job = Job {
        kind: AnalysisKind::Root,
        id: 1,
        source: Source {
            channel: ChannelId(1),
            device: DeviceId(2),
            file: "owned.wav".into(),
            pcm: Arc::new(AudioData { sample_rate: 48000, channels: 1, samples: vec![] }),
            start: 0.0,
            end: 1.0,
            root: 60.0,
            profile: None,
        },
        cancel: flag.clone(),
        progress: Arc::new([AtomicUsize::new(0), AtomicUsize::new(0)]),
        receiver: Some(rx),
        phase: AnalysisPhase::Running,
        report: None,
        modal: None,
        message: None,
        pending_root: None,
        pending_fit: false,
    };
    job.receive(false);
    assert_eq!(job.phase, AnalysisPhase::Running);
    job.stop(AnalysisPhase::Cancelled, "cancelled");
    assert!(flag.load(Ordering::Acquire));
    // A finished report arrives after cancellation; it must not restore a proposal.
    tx.send(Ok(Prepared::Root(Report {
        version: 1,
        range: 0..4096,
        outcome: Outcome::Silent,
        proposal: None,
        active_windows: 0,
        voiced_windows: 0,
    })))
    .unwrap();
    job.receive(true);
    assert_eq!(job.phase, AnalysisPhase::Cancelled);
    assert!(job.report.is_none() && job.receiver.is_none());
    flag.store(false, Ordering::Release);
    drop(job);
    assert!(flag.load(Ordering::Acquire), "dropping a session job cancels its detached worker");
}
