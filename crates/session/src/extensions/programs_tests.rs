// Copyright (C) 2026 The Suhina contributors
// SPDX-License-Identifier: GPL-3.0-or-later

use super::*;

/// A stop that arrives before the process exists starts nothing: the watch is over
/// cleanly, no child was spawned, and no crash is reported.
#[test]
fn a_stop_before_the_process_exists_starts_nothing() {
    let (to, from) = channel();
    to.send(ToProgram::Stop).expect("the stop is sent");
    let events: Shared = Arc::new(Queue::default());
    let pid = std::sync::atomic::AtomicU32::new(0);
    let mut ports = (Vec::new(), Vec::new());
    let watch_result = watch(
        Path::new("/nonexistent/runner"),
        Path::new("script.lua"),
        &from,
        &events,
        &pid,
        &QuitReset::default(),
        &mut ports,
        &mut Pending::default(),
    );
    assert_eq!(watch_result, Ok(()));
    assert_eq!(pid.load(std::sync::atomic::Ordering::Relaxed), 0, "no process was started");
    let queue = events.queue.lock().unwrap_or_else(std::sync::PoisonError::into_inner);
    assert!(!queue.iter().any(|event| matches!(event, Event::Crashed(_))), "{queue:?}");
}

/// A stop while the runner is alive but has not connected ends the start without a
/// crash: the caller's join is not held through the connection's timeout, the process is
/// ended and reaped, and the watch is over the `Ok` way.
#[cfg(unix)]
#[test]
fn a_stop_while_the_runner_connects_ends_the_start_without_a_crash() {
    let dir = std::env::temp_dir().join(format!(
        "suhina-watch-stop-{}-{}",
        std::process::id(),
        std::time::Instant::now().elapsed().as_nanos()
    ));
    std::fs::create_dir_all(&dir).expect("the folder is made");
    // The fake runner says it is alive by opening the fifo — the test's blocking open is
    // the explicit signal that the child is past its spawn — then sleeps, never
    // connecting to the control socket.
    let fifo = dir.join("started.fifo");
    assert!(std::process::Command::new("mkfifo").arg(&fifo).status().map(|s| s.success()).unwrap_or(false));
    let runner = dir.join("sleeper.sh");
    std::fs::write(&runner, format!("#!/bin/sh\necho started > '{}'\nsleep 5\n", fifo.display()))
        .expect("the runner is written");
    let mut permissions = std::fs::metadata(&runner).expect("the runner is there").permissions();
    use std::os::unix::fs::PermissionsExt;
    permissions.set_mode(0o755);
    std::fs::set_permissions(&runner, permissions).expect("the runner is executable");
    let script = dir.join("script.lua");
    std::fs::write(&script, "-- unused\n").expect("the script is written");

    let (to, from) = channel();
    let events: Shared = Arc::new(Queue::default());
    let pid = Arc::new(std::sync::atomic::AtomicU32::new(0));
    let result: std::sync::mpsc::Receiver<Result<(), String>> = {
        let (done, done_rx) = channel();
        let thread_events = Arc::clone(&events);
        let thread_pid = Arc::clone(&pid);
        let runner = runner.clone();
        let script = script.clone();
        std::thread::spawn(move || {
            let mut ports = (Vec::new(), Vec::new());
            let _ = done.send(watch(
                &runner,
                &script,
                &from,
                &thread_events,
                &thread_pid,
                &QuitReset::default(),
                &mut ports,
                &mut Pending::default(),
            ));
        });
        done_rx
    };
    // The child announced itself: it is alive in the accept loop's window, not connected.
    let announced = std::fs::File::open(&fifo).expect("the runner announced itself");
    let mut announced = announced;
    use std::io::Read;
    let mut line = String::new();
    announced.read_to_string(&mut line).expect("the announcement is read");
    assert!(pid.load(std::sync::atomic::Ordering::Relaxed) != 0, "the child was spawned");

    to.send(ToProgram::Stop).expect("the stop is sent");
    let watch_result = result.recv().expect("the watch thread ended");
    assert_eq!(watch_result, Ok(()), "the stop is not a crash");
    let queue = events.queue.lock().unwrap_or_else(std::sync::PoisonError::into_inner);
    assert!(!queue.iter().any(|event| matches!(event, Event::Crashed(_))), "{queue:?}");
    let _ = std::fs::remove_dir_all(&dir);
}

/// An empty read is "nothing now", never a close: an idle connection and one whose peer
/// has gone answer `None` alike — on Windows they cannot be told apart, a nonblocking
/// pipe with no data answering with the same zero-byte read a close gives — and what
/// ends a program whose process is over is the watch's `try_wait`, not the read. What a
/// live peer sends still comes back whole.
#[test]
fn an_empty_read_is_nothing_now_and_never_a_close() {
    let (address, name) = control_name().expect("the socket's name");
    let listener = ListenerOptions::new().name(name).create_sync().expect("the listener");
    // The peer holds the connection open until the test lets it go; its line is written
    // before any read, so what waits is only the line's travel.
    let (may_close, close_ok) = channel::<()>();
    let peer = std::thread::spawn(move || {
        let mut stream = suhina_plugins::connect(&address).expect("the peer connects");
        stream.write_all(b"hello\n").expect("the line goes");
        stream.flush().expect("the writer flushes");
        close_ok.recv().expect("the test allows the close");
    });
    let stream = listener.accept().expect("the peer connects");
    let mut link = Link::new(stream).expect("the link is made");
    // The line that is there comes back whole.
    assert_eq!(link.read().expect("a read").as_deref(), Some("hello"));
    // Idle: nothing whole is there, and that answers `None`, not an error.
    link.reader.get_mut().set_nonblocking(true).expect("the socket is nonblocking");
    for _ in 0..10 {
        assert_eq!(link.read().expect("an idle read"), None);
        std::thread::sleep(POLL);
    }
    // The peer goes: the reads answer the same, never a "closed" error — the process's
    // own end is the signal the watch runs on.
    may_close.send(()).expect("the close is allowed");
    peer.join().expect("the peer ends");
    for _ in 0..10 {
        assert_eq!(link.read().expect("a closed read"), None);
        std::thread::sleep(POLL);
    }
}

/// A write that waits keeps reading: a full duplex pair whose buffers both run small can
/// jam — the peer blocked on a write that needs our reads, our write blocked on the
/// peer's reads — and a send that only retried without reading left both wedged for the
/// whole deadline and killed a healthy runner (#1670; Windows' pipes are a few KiB, and
/// CI caught it there). What waits parks the peer's lines, the peer's write drains, room
/// opens both ways, and the parked lines come back first, whole and in their order. The
/// test's timeout is not production's two seconds: the dance is two threads' scheduling,
/// and what this test is about must not turn on a loaded machine's (AGENTS.md rule 7).
#[test]
fn a_write_that_waits_keeps_reading_and_parks_the_peers_lines() {
    let (address, name) = control_name().expect("the socket's name");
    let listener = ListenerOptions::new().name(name).create_sync().expect("the listener");
    // The peer jams its direction first: a MiB of lines past any socket buffer, whose write
    // cannot finish until the host reads — the state a real runner is in when a
    // send-back blocks it.
    let peer = std::thread::spawn(move || {
        let mut stream = suhina_plugins::connect(&address).expect("the peer connects");
        stream.write_all(b"one\ntwo\nthree\n").expect("the lines go");
        for n in 0..16 {
            let jam = format!("jam{n} {}\n", "x".repeat(64 * 1024));
            stream.write_all(jam.as_bytes()).expect("the jam goes");
        }
        stream.flush().expect("the writer flushes");
        // Only the host's parking can reach this read, and the host's blocked write
        // only finishes once it does.
        let mut reader = BufReader::new(stream);
        let mut line = String::new();
        reader.read_line(&mut line).expect("the message arrives");
        assert!(line.ends_with('\n'), "a whole line");
    });
    let stream = listener.accept().expect("the peer connects");
    stream.set_nonblocking(true).expect("the socket is nonblocking");
    let mut link = Link::new(stream).expect("the link is made");
    link.write_timeout = Duration::from_secs(30);
    // A message past any socket buffer: the send cannot finish until the peer reads.
    let big = Message::Send { port: "APC out".into(), bytes: vec![b'a'; 1024 * 1024] };
    link.send(&big).expect("the send completes while the peer is jammed");
    // The parked lines came first, whole and in their order.
    assert_eq!(link.read().expect("a read").as_deref(), Some("one"));
    assert_eq!(link.read().expect("a read").as_deref(), Some("two"));
    assert_eq!(link.read().expect("a read").as_deref(), Some("three"));
    for n in 0..16 {
        let line = link.read().expect("a jam line").expect("the jam line is there");
        assert!(line.starts_with(&format!("jam{n} ")), "{line:.16}");
    }
    assert_eq!(link.read().expect("a read"), None, "nothing else came");
    peer.join().expect("the peer ends");
}

/// The parked lines hold the send-backs' own caps: a peer streaming while it never reads
/// cannot grow the host's memory. A line past the caps is still read — the peer must be
/// unjammed — and let go, counted for the watch loops to say like the queue's, and the
/// parked lines still come back first and in their order.
#[test]
fn parked_lines_hold_the_send_backs_caps_and_the_excess_is_counted() {
    let (address, name) = control_name().expect("the socket's name");
    let listener = ListenerOptions::new().name(name).create_sync().expect("the listener");
    // The lines are in the socket before the write starts — so four quick reads find them
    // whatever the machine's load — and the peer reads nothing until the test lets it, so
    // the write's wait is the deadline's, which is what the caps are exercised under.
    let (written_ok, written) = channel::<()>();
    let (may_read, read_ok) = channel::<()>();
    let peer = std::thread::spawn(move || {
        let mut stream = suhina_plugins::connect(&address).expect("the peer connects");
        stream.write_all(b"one\ntwo\nthree\nfour\n").expect("the lines go");
        stream.flush().expect("the writer flushes");
        written_ok.send(()).expect("the lines are in");
        read_ok.recv().expect("the test lets the peer read");
        let mut reader = BufReader::new(stream);
        let mut line = String::new();
        let _ = reader.read_line(&mut line);
    });
    let stream = listener.accept().expect("the peer connects");
    stream.set_nonblocking(true).expect("the socket is nonblocking");
    let mut link = Link::new(stream).expect("the link is made");
    // Two lines park; the lines past them go, counted. The message is past any socket
    // buffer, and the peer not reading is what ends the wait.
    link.parked_max = 2;
    link.parked_bytes_max = 1024 * 1024;
    written.recv().expect("the peer wrote first");
    let big = Message::Send { port: "APC out".into(), bytes: vec![b'a'; 4 * 1024 * 1024] };
    let err = link.send(&big).expect_err("the peer is not reading: the write cannot complete");
    assert!(err.contains("did not complete in time"), "{err}");
    assert_eq!(link.take_parked_dropped(), 2, "the lines past the caps were read and counted");
    assert_eq!(link.read().expect("a read").as_deref(), Some("one"));
    assert_eq!(link.read().expect("a read").as_deref(), Some("two"));
    assert_eq!(link.read().expect("a read"), None, "the dropped lines never come back");
    may_read.send(()).expect("the peer may read");
    drop(link); // Its end is the peer's EOF: the write that did not complete left no line.
    peer.join().expect("the peer ends");
}

/// A drop noted outside the send-back queue — the parked lines' — wakes a waiter like the
/// queue's own does: the event is pushed and the signal notified. Windows' small pipes make
/// the parked path the one a storm takes, and a drop with no wake hung the end-to-end test
/// there (#1670). The wait's bound is a liveness check, not a timing one: a notify arrives
/// at once, and only its absence reaches the bound.
#[test]
fn a_parked_drop_wakes_a_waiter() {
    let events: Shared = Arc::new(Queue::default());
    let waiter = {
        let events = Arc::clone(&events);
        std::thread::spawn(move || {
            let queue = events.queue.lock().unwrap_or_else(std::sync::PoisonError::into_inner);
            let (queue, waited) = events
                .signal
                .wait_timeout_while(queue, Duration::from_secs(10), |queue| {
                    !queue.iter().any(|event| matches!(event, Event::Dropped(_)))
                })
                .unwrap_or_else(std::sync::PoisonError::into_inner);
            let found = queue.iter().any(|event| matches!(event, Event::Dropped(_)));
            drop(queue);
            (found, waited)
        })
    };
    let mut dropped = 0;
    note_dropped(&events, &mut dropped);
    let (found, waited) = waiter.join().expect("the waiter ends");
    assert!(found, "the drop's event is in the queue");
    assert!(!waited.timed_out(), "the drop woke the waiter");
    assert_eq!(dropped, 1);
}

/// A send-back queued after the test's last look wakes its pump: without the wake, the
/// marker the test waits for stays in the queue until a look that never comes. The wake is
/// taken without a clock: the enqueue calls it on the calling thread.
#[test]
fn a_queued_send_back_wakes_the_tests_pump() {
    let wake = test_watch();
    // Empty the channel of any wake another test in this process sent before this one.
    while wake.try_recv().is_ok() {}
    let events: Shared = Arc::new(Queue::default());
    enqueue_send_back(&events, "APC out".into(), vec![0xB1, 0x00, 0x01], &mut 0);
    assert!(wake.try_recv().is_ok(), "the send-back did not wake the pump");
    let queue = events.queue.lock().unwrap_or_else(std::sync::PoisonError::into_inner);
    assert!(matches!(queue.front(), Some(Event::SendBack { .. })), "{queue:?}");
}
