// Copyright (C) 2026 The Suhina contributors
// SPDX-License-Identifier: GPL-3.0-or-later

use super::*;

/// The bindings follow the CPU's packing: 8 bytes on ARM, 1 on x86. With x86's on ARM, a
/// plug-in read the region sequence's musical context 4 bytes late and crashed (PitchNet on an
/// Apple silicon Mac, on any track with audio).
#[test]
fn the_bindings_lay_structs_out_as_the_headers_do_on_this_cpu() {
    let (context, region_start) = if cfg!(target_arch = "aarch64") { (24, 16) } else { (20, 12) };
    assert_eq!(std::mem::offset_of!(ARARegionSequenceProperties, musicalContextRef), context);
    assert_eq!(std::mem::offset_of!(ARAPlaybackRegionProperties, startInModificationTime), region_start);
}

/// A plug-in's source ref for the tests, never read.
fn source_ref(n: usize) -> ARAAudioSourceRef {
    std::ptr::without_provenance_mut(n)
}

/// Progress as `analysis` holds it, reported just now.
fn reported(list: &[(usize, f32)]) -> Mutex<Vec<(usize, f32, Instant)>> {
    let now = Instant::now();
    Mutex::new(list.iter().map(|&(key, progress)| (key, progress, now)).collect())
}

/// The keys and progress `analysis` holds.
fn progress(analysis: &Mutex<Vec<(usize, f32, Instant)>>) -> Vec<(usize, f32)> {
    analysis.lock().unwrap().iter().map(|&(key, progress, _)| (key, progress)).collect()
}

/// A plug-in that said an analysis *started* and then found it done (restored from an
/// archive) never says *completed*: asking it takes the source out, while one it still
/// analyses stays (#816).
#[test]
fn an_analysis_the_plug_in_says_is_complete_stops_showing() {
    let notes = kARAContentTypeNotes as ARAContentType;
    let analysis = reported(&[(1, 0.0), (2, 0.4), (9, 0.5)]);
    let sources = [(1, source_ref(11)), (2, source_ref(12)), (3, source_ref(13))];
    // Source 12 is still being analysed; 11 is done; 9 is not this document's.
    assert!(settle_analysis(&analysis, &sources, &[notes], |source, _| source == source_ref(12)));
    assert_eq!(progress(&analysis), vec![(2, 0.4), (9, 0.5)]);
    // Nothing more to take out: false, and the list stays.
    assert!(!settle_analysis(&analysis, &sources, &[notes], |source, _| source == source_ref(12)));
    assert_eq!(progress(&analysis), vec![(2, 0.4), (9, 0.5)]);
    // Source 3 is done but was never listed: nothing changes.
    assert!(!settle_analysis(&Mutex::new(Vec::new()), &sources, &[notes], |_, _| false));
}

/// One progress covers all of a source's analyses: a plug-in whose notes are done while it
/// still analyses the tempo keeps showing it, until that is done too, or until it has not
/// reported for a while (a type it never analyses unasked). With no types listed, the notes
/// alone settle it.
#[test]
fn an_analysis_of_another_content_type_keeps_showing() {
    let notes = kARAContentTypeNotes as ARAContentType;
    let tempo = kARAContentTypeTempoEntries as ARAContentType;
    let types = [notes, tempo];
    let sources = [(1, source_ref(11))];
    let tempo_left = |_, kind| kind == tempo;

    let analysis = reported(&[(1, 0.6)]);
    assert!(!settle_analysis(&analysis, &sources, &types, tempo_left));
    assert_eq!(progress(&analysis), vec![(1, 0.6)]);
    // The tempo done too.
    assert!(settle_analysis(&analysis, &sources, &types, |_, _| false));
    assert!(analysis.lock().unwrap().is_empty());

    // No report for a while: the tempo is not being analysed.
    let long_ago = Instant::now().checked_sub(ANALYSIS_QUIET).expect("an instant that long ago");
    let analysis = Mutex::new(vec![(1, 0.0, long_ago)]);
    assert!(settle_analysis(&analysis, &sources, &types, tempo_left));
    // The notes not done: it stays, however long ago.
    let analysis = Mutex::new(vec![(1, 0.0, long_ago)]);
    assert!(!settle_analysis(&analysis, &sources, &types, |_, kind| kind == notes));

    // No analysable types listed: the notes alone.
    let analysis = reported(&[(1, 0.6)]);
    assert!(settle_analysis(&analysis, &sources, &[], tempo_left));
}

/// A plug-in whose analysis an archive replaced may keep saying the notes are incomplete while
/// it has them (#816): once a track has every region's notes, a source of it the plug-in has not
/// reported for a while is taken out; one reported just now, or another track's, stays.
#[test]
fn a_quiet_analysis_of_a_track_with_its_notes_stops_showing() {
    let long_ago = Instant::now().checked_sub(ANALYSIS_QUIET).expect("an instant that long ago");
    let analysis = Mutex::new(vec![(1, 0.0, long_ago), (2, 0.3, Instant::now()), (9, 0.0, long_ago)]);
    assert!(settle_quiet(&analysis, &[1, 2]));
    assert_eq!(progress(&analysis), vec![(2, 0.3), (9, 0.0)]);
    // Nothing quiet left of the track's: false.
    assert!(!settle_quiet(&analysis, &[1, 2]));
    assert!(!settle_quiet(&Mutex::new(Vec::new()), &[1]));
}

/// A track removed while its source is analysed leaves nothing behind: a plug-in added again
/// for that source is asked for its analysis again and starts with no stale progress (#816).
#[test]
fn a_track_that_leaves_forgets_its_sources_analysis() {
    let analysis = reported(&[(1, 0.0), (2, 0.3)]);
    let asked = RefCell::new(vec![source_ref(11), source_ref(12)]);
    forget_analysis(&analysis, &asked, &[(1, source_ref(11))]);
    assert_eq!(progress(&analysis), vec![(2, 0.3)]);
    assert_eq!(*asked.borrow(), vec![source_ref(12)]);
}

/// Two tracks play source `a`, the first also `b`: when the first leaves, only `b` is
/// forgotten, so the second track's analysis of `a` keeps showing; when the second leaves
/// too, `a` is forgotten.
#[test]
fn a_source_another_track_plays_keeps_its_analysis() {
    let ids = |v: &[&str]| v.iter().map(|s| (*s).to_owned()).collect::<Vec<_>>();
    let first = ids(&["a", "b"]);
    let second = ids(&["a"]);
    let mut owners = ids(&["a", "b", "a"]);
    let analysis = reported(&[(1, 0.2), (2, 0.5)]);
    let asked = RefCell::new(vec![source_ref(11), source_ref(12)]);
    let keys = |gone: &[String]| -> Vec<(usize, ARAAudioSourceRef)> {
        gone.iter().map(|id| if id == "a" { (1, source_ref(11)) } else { (2, source_ref(12)) }).collect()
    };

    let gone = leave_sources(&mut owners, &first);
    assert_eq!(gone, ids(&["b"]));
    forget_analysis(&analysis, &asked, &keys(&gone));
    assert_eq!(progress(&analysis), vec![(1, 0.2)]);
    assert_eq!(*asked.borrow(), vec![source_ref(11)]);

    let gone = leave_sources(&mut owners, &second);
    assert_eq!(gone, ids(&["a"]));
    forget_analysis(&analysis, &asked, &keys(&gone));
    assert!(analysis.lock().unwrap().is_empty());
    assert!(owners.is_empty());
}

#[test]
fn a_position_asked_for_is_shown_until_the_engine_is_there() {
    let rate = 48_000.0;
    // Nothing asked for: the engine's position.
    assert_eq!(shown_position(1000, rate, false), 1000);
    // Asked for 2 s while the engine is still at its old place: 2 s is shown.
    hold_position(2.0);
    assert_eq!(shown_position(1000, rate, false), 96_000);
    assert_eq!(shown_position(1512, rate, false), 96_000);
    // The engine gets there: its own from then on, the hold let go of.
    assert_eq!(shown_position(96_010, rate, false), 96_010);
    assert_eq!(shown_position(1000, rate, false), 1000);
    // Playing, the engine's position always.
    hold_position(3.0);
    assert_eq!(shown_position(1000, rate, true), 1000);
    assert_eq!(shown_position(1000, rate, false), 1000);
}

#[test]
fn reading_fills_each_channel_with_silence_outside() {
    let audio = AudioSource::from_interleaved(&[1.0, -1.0, 2.0, -2.0], 2, 48_000.0).unwrap();
    let source = Box::new(Source { audio: AudioSource::open(&audio.address()).unwrap() });
    let source_ref: *const Source = &*source;
    // SAFETY: a reader of a live source, used and destroyed here.
    let reader = unsafe { create_reader(std::ptr::null_mut(), source_ref.cast_mut().cast(), 0) };
    let (mut left, mut right) = ([9.0f32; 4], [9.0f32; 4]);
    let buffers = [left.as_mut_ptr().cast::<c_void>(), right.as_mut_ptr().cast()];
    // SAFETY: two buffers of four floats for a stereo source.
    let ok = unsafe { read_samples(std::ptr::null_mut(), reader, -1, 4, buffers.as_ptr()) };
    assert_eq!(ok, 1);
    assert_eq!((left, right), ([0.0, 1.0, 2.0, 0.0], [0.0, -1.0, -2.0, 0.0]));
    // A negative count and a missing buffer list are refused.
    // SAFETY: as above.
    assert_eq!(unsafe { read_samples(std::ptr::null_mut(), reader, 0, -1, buffers.as_ptr()) }, 0);
    // SAFETY: as above.
    assert_eq!(unsafe { read_samples(std::ptr::null_mut(), reader, 0, 4, std::ptr::null()) }, 0);
    // SAFETY: the reader made above, destroyed once.
    unsafe { destroy_reader(std::ptr::null_mut(), reader) };

    // Doubles, for a plug-in that asks for 64-bit samples.
    // SAFETY: as above.
    let wide = unsafe { create_reader(std::ptr::null_mut(), source_ref.cast_mut().cast(), 1) };
    let (mut l64, mut r64) = ([9.0f64; 2], [9.0f64; 2]);
    let buffers = [l64.as_mut_ptr().cast::<c_void>(), r64.as_mut_ptr().cast()];
    // SAFETY: two buffers of two doubles.
    assert_eq!(unsafe { read_samples(std::ptr::null_mut(), wide, 1, 2, buffers.as_ptr()) }, 1);
    assert_eq!((l64, r64), ([2.0, 0.0], [-2.0, 0.0]));
    // SAFETY: as above.
    unsafe { destroy_reader(std::ptr::null_mut(), wide) };
}

#[test]
fn a_state_carries_the_archive_and_reads_back() {
    let wrapped = wrap_state(vec![1, 2, 3], Some(("com.example.doc".into(), vec![9; 5])));
    let (component, archive) = unwrap_state(&wrapped);
    assert_eq!(component, [1, 2, 3]);
    assert_eq!(archive, Some(("com.example.doc".to_string(), [9u8; 5].as_slice())));
    // Without an archive, and any state of the plug-in's own, it is the component's.
    assert_eq!(wrap_state(vec![4, 5], None), [4, 5]);
    assert_eq!(unwrap_state(&[4, 5]), ([4u8, 5].as_slice(), None));
    // Cut short, it is not split.
    let cut = &wrapped[..wrapped.len() - 1];
    assert_eq!(unwrap_state(cut), (cut, None));
    let mut huge = wrapped.clone();
    huge[20..28].copy_from_slice(&u64::MAX.to_le_bytes());
    assert_eq!(unwrap_state(&huge).1, None);
}

#[test]
fn archives_are_read_and_written_within_bounds() {
    let mut archive = Archive { bytes: Vec::new(), id: CString::new("com.example.doc").unwrap() };
    let writer: ARAArchiveWriterHostRef = std::ptr::from_mut(&mut archive).cast();
    // SAFETY: an archive live for the calls; buffers of the lengths given.
    let write = |at: usize, bytes: &[u8], len: usize| unsafe {
        write_archive(std::ptr::null_mut(), writer, at, len, bytes.as_ptr())
    };
    assert_eq!(write(2, b"abc", 3), 1);
    assert_eq!(write(usize::MAX, b"ab", 2), 0);
    assert_eq!(write(0, b"a", MAX_ARCHIVE + 1), 0);
    assert_eq!(archive.bytes, [0, 0, b'a', b'b', b'c']);
    let reader: ARAArchiveReaderHostRef = std::ptr::from_mut(&mut archive).cast();
    let mut out = [0u8; 3];
    // SAFETY: as above.
    assert_eq!(unsafe { archive_size(std::ptr::null_mut(), reader) }, 5);
    // SAFETY: as above.
    assert_eq!(unsafe { read_archive(std::ptr::null_mut(), reader, 2, 3, out.as_mut_ptr()) }, 1);
    // SAFETY: as above.
    assert_eq!(unsafe { read_archive(std::ptr::null_mut(), reader, 4, 3, out.as_mut_ptr()) }, 0);
    // SAFETY: as above.
    assert!(!unsafe { archive_id(std::ptr::null_mut(), reader) }.is_null());
    assert_eq!(&out, b"abc");
}
