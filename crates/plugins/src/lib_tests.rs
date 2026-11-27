// Copyright (C) 2026 The Suhina contributors
// SPDX-License-Identifier: GPL-3.0-or-later

#![allow(clippy::panic, clippy::unwrap_used)] // test helpers outside #[test] functions
#[test]
fn repeated_misses_skip_recovery_wait_but_a_late_answer_can_recover() {
    let link = Link::create(2, 16).unwrap();
    let mut host = suhina_ipc::Served::open(&link.address()).unwrap();
    let mut audio = PluginAudio {
        link,
        unanswered: Arc::new(AtomicU32::new(0)),
        group: std::sync::Weak::new(),
        sample_rate: Arc::new(AtomicU64::new(44100.0f64.to_bits())),
        instance: 1,
        process: 1,
    };
    let input = [0.5; 32];
    let mut output = [0.0; 32];
    assert!(audio.send(&input, &[], output.len(), Duration::ZERO));
    assert_eq!(audio.finish(&mut output, Duration::ZERO), Outcome::Missed);
    assert!(!audio.ready(Instant::now()), "one late block may still catch up");
    assert!(!audio.send(&input, &[], output.len(), Duration::ZERO));
    assert!(audio.ready(Instant::now()), "let the chain advance to immediate refusal");
    assert!(audio.watch_ready().is_none(), "do not enlist the hung host in the scheduler wait");
    assert!(!audio.send(&input, &[], output.len(), Duration::ZERO));
    assert!(host.serve(Duration::ZERO, |input, output| output.copy_from_slice(input)));
    assert!(audio.ready(Instant::now()));
    assert!(audio.send(&input, &[], output.len(), Duration::ZERO));
    assert_eq!(audio.unanswered.load(Ordering::Relaxed), 0);
    assert!(host.serve(Duration::ZERO, |input, output| output.copy_from_slice(input)));
    assert_eq!(audio.finish(&mut output, Duration::ZERO), Outcome::Done);
    assert_eq!(output, input);
}

use super::*;
use std::sync::mpsc::Sender;

#[test]
fn a_lock_waited_for_until_a_deadline_gives_up_after_it() {
    let mutex = Mutex::new(1);
    // Free: taken whatever the deadline.
    assert!(lock_until(&mutex, Instant::now()).is_some());
    // Held: a deadline already passed gives up at once, a later one is waited for.
    let held = lock(&mutex);
    assert!(lock_until(&mutex, Instant::now()).is_none());
    thread::scope(|scope| {
        let waiting = scope.spawn(|| lock_until(&mutex, Instant::now() + Duration::from_secs(60)).is_some());
        drop(held);
        assert!(waiting.join().unwrap());
    });
}

/// Space in a plug-in's window: the host's unasked `transport_key` line is counted and wakes
/// the window, in that order, so the frame it wakes plays at once (#714). It is not left in
/// the replies for a request to find.
#[test]
fn a_transport_key_from_a_host_is_counted_then_wakes_the_window() {
    let (address, name) = control_name().unwrap();
    let listener = ListenerOptions::new().name(name).create_sync().unwrap();
    let mut remote = connect(&address).unwrap();
    let control = listener.accept().unwrap();
    let replies = read_lines(control);
    // The waker says how many keys were counted when it was called.
    let (woken, wakes) = mpsc::channel();
    set_waker(move || {
        let _ = woken.send(TRANSPORT_KEYS.load(Ordering::Relaxed));
    });
    let line = format!("{}\n", Message::TransportKey.encode());
    remote.write_all(line.as_bytes()).unwrap();
    remote.flush().unwrap();
    // Another test's host may wake the window too; this key's wake is the one with it counted.
    let counted = loop {
        let keys = wakes.recv_timeout(Duration::from_secs(60)).expect("the window was not woken");
        if keys > 0 {
            break keys;
        }
    };
    assert!(counted >= 1);
    assert!(take_transport_keys() >= 1);
    assert!(matches!(replies.try_recv(), Err(mpsc::TryRecvError::Empty)), "not queued as a reply");
}

/// A host process that is alive but runs nothing: a real child and a control socket, with
/// what the test `feed`s a group, so `host_memory` can be tried without the executable.
fn living_process() -> (HostProcess, Sender<Message>) {
    let (address, name) = control_name().unwrap();
    let listener = ListenerOptions::new().name(name).create_sync().unwrap();
    // A listener takes a connection made before it is asked, so a connection that fails ends
    // the test here and `accept` has no one to wait for. The remote half lives until then:
    // one that is gone by `accept` leaves it nothing to take.
    let remote = connect(&address).unwrap();
    let control = listener.accept().unwrap();
    drop(remote);
    // A child that lives on and ends by itself. `timeout` waits for a key and errors when
    // there is no console to take it from; a ping to no host waits without input.
    let mut child = Command::new(if cfg!(windows) { "cmd" } else { "sleep" });
    if cfg!(windows) {
        child.args(["/c", "ping", "-n", "11", "127.0.0.1", ">nul"]);
    } else {
        child.arg("10");
    }
    let child = child.stdout(Stdio::null()).stderr(Stdio::null()).spawn().unwrap();
    let (feed, replies) = mpsc::channel();
    (HostProcess { child, control: std::sync::Arc::new(Mutex::new(control)), replies, edits: Arc::default() }, feed)
}

/// An instance let go of takes its edits out of the inbox, the others' stay (#1205).
#[test]
fn an_instance_let_go_of_leaves_no_edits_in_the_inbox() {
    let inbox = EditInbox::default();
    take_in_edit(&inbox, 4, 1, 0.25);
    take_in_edit(&inbox, 7, 1, 0.75);
    drop(Forget { edits: Arc::clone(&inbox), instance: 4 });
    assert_eq!(*lock(&inbox), vec![(7, 1, 0.75)]);
}

/// What the user moved in an editor is taken while the group is busy with a request (its
/// lock held), each parameter once at its latest value and only by its own instance, and is
/// kept to be set again after a restart (#1205).
#[test]
fn editor_edits_are_taken_while_a_request_holds_the_group() {
    let (process, _feed) = living_process();
    let inbox = Arc::clone(&process.edits);
    let mut group = Group::new();
    group.process = Some(process);
    let group = Mutex::new(group);
    let since = Arc::new(Mutex::new(vec![(9, 0.5)]));
    let watch = EditWatch { inbox: lock(&group).edit_inbox(), instance: 4, since: Arc::clone(&since) };

    let busy = lock(&group);
    take_in_edit(&inbox, 4, 1, 0.25);
    take_in_edit(&inbox, 7, 1, 0.75);
    take_in_edit(&inbox, 4, 1, 0.5);
    take_in_edit(&inbox, 4, 2, 1.0);
    assert_eq!(watch.take_param_edits(), vec![(1, 0.5), (2, 1.0)]);
    assert!(watch.take_param_edits().is_empty(), "each edit is taken once");
    drop(busy);

    assert_eq!(*lock(&inbox), vec![(7, 1, 0.75)], "another instance's edit waits for it");
    assert_eq!(*lock(&since), vec![(9, 0.5), (1, 0.5), (2, 1.0)]);
}

#[test]
fn a_host_that_reports_memory_is_counted_and_one_that_died_is_not() {
    let (process, feed) = living_process();
    let mut group = Group::new();
    group.process = Some(process);
    let group = Arc::new(Mutex::new(group));
    lock(&GROUPS).insert((PathBuf::from("the test host"), "memory-test".into()), Arc::downgrade(&group));

    // What the host sent is taken in and counted.
    feed.send(Message::Memory { bytes: 300 }).unwrap();
    assert_eq!(host_memory(), vec![300]);
    // Reading does not consume the report; it stands until the next one.
    assert_eq!(host_memory(), vec![300]);

    // A process that has stopped is not counted, even with its last report in hand.
    {
        let mut locked = lock(&group);
        if let Some(process) = locked.process.as_mut() {
            let _ = process.child.kill();
        }
    }
    // An ended child stays "running" its own last moment: let it be gone. (`alive` reaps
    // what has gone, so the loop is what reads it.)
    let deadline = Instant::now() + Duration::from_secs(60);
    while {
        let mut locked = lock(&group);
        locked.alive()
    } {
        assert!(Instant::now() < deadline, "the ended host still looks alive");
        thread::sleep(Duration::from_millis(10));
    }
    assert!(host_memory().is_empty(), "a dead host's report must not be counted");
    assert_eq!(lock(&group).ram, None, "the dead host's report is let go of");
}

/// A plug-in that applies a state a moment after it is given it answers the load, and reads for
/// a while after, with exactly the state it had: those reads do not replace the state given, so a
/// backup or a restart in that moment keeps it; the first read of anything else is its state.
/// One that answers with something new took the state in a form of its own, which stands (#1674).
#[test]
fn a_state_applied_late_is_kept_until_the_plug_in_reports_something_else() {
    let state = |bytes: &[u8]| SavedState { data: bytes.to_vec(), controller: Some(Vec::new()) };
    let (initial, given, edited) = (state(b"initial"), state(b"given"), state(b"edited"));
    let mut kept = KeptState::default();
    kept.given(given.clone(), initial.clone(), Some(initial.clone()));
    assert_eq!(kept.get(), Some(&given));
    assert_eq!(kept.read(initial.clone()), (given.clone(), false), "still its old state: the given one stands");
    assert_eq!(kept.get(), Some(&given));
    assert_eq!(kept.read(given.clone()), (given.clone(), true), "applied");
    // Settled: what it reports now is its state, the old one too if its editor goes back to it.
    assert_eq!(kept.read(edited.clone()), (edited.clone(), true));
    assert_eq!(kept.read(initial.clone()), (initial.clone(), true));
    assert_eq!(kept.get(), Some(&initial));

    // A plug-in that takes the state at once: every read is its state.
    let mut at_once = KeptState::default();
    at_once.given(given.clone(), given.clone(), Some(initial.clone()));
    assert_eq!(at_once.read(initial.clone()), (initial.clone(), true));

    // One that takes it in a form of its own (a VST2 bank read by its VST3 successor): that form
    // is its state from the first read.
    let own = state(b"own form");
    let mut converted = KeptState::default();
    converted.given(given.clone(), own.clone(), Some(initial.clone()));
    assert_eq!(converted.read(own.clone()), (own.clone(), true));
    assert_eq!(converted.get(), Some(&own));

    // What it had before not known: every read is its state, as before.
    let mut unknown = KeptState::default();
    unknown.given(given.clone(), initial.clone(), None);
    assert_eq!(unknown.read(initial.clone()), (initial, true));

    // An edit made before the given state shows is taken at once.
    let mut edited_early = KeptState::default();
    edited_early.given(given.clone(), state(b"before"), Some(state(b"before")));
    assert_eq!(edited_early.read(edited.clone()), (edited.clone(), true));
    assert_eq!(edited_early.get(), Some(&edited));
}
