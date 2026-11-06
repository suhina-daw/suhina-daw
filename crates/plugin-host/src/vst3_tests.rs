// Copyright (C) 2026 The Suhina contributors
// SPDX-License-Identifier: GPL-3.0-or-later

use super::*;

#[test]
fn the_trace_watch_merges_peaks_and_starts_each_interval_from_silence() {
    let watch = Watch::default();
    watch.merge_peak(&[0.25, -0.75, 0.5]);
    watch.merge_peak(&[0.5, f32::NAN]);
    watch.merge_peak(&[]);
    assert_eq!(watch.take_peak(), 0.75);
    assert_eq!(watch.take_peak(), 0.0);
    watch.merge_peak(&[-0.125]);
    assert_eq!(watch.take_peak(), 0.125);
}

/// The main thread resets the counters while the audio thread keeps adding to them: no
/// frame is lost, and a peak that was taken never comes back. The work runs in rounds that
/// both threads start and end at a barrier: in each, the audio thread adds a fixed number of
/// blocks while the main thread resets, and after the end barrier one more reset closes the
/// round while the audio thread waits at the next round's start. So a reset always falls
/// between one round's blocks and the next round's (the round's last block is the peak that
/// reset or an earlier one took, checked each round), every round's interval holds exactly
/// its blocks whatever order the threads ran in inside it, and the counts below are exact.
/// The resets inside a round may or may not interleave with its blocks; nothing asserted
/// depends on which.
#[test]
fn resetting_the_trace_watch_loses_no_frames_and_repeats_no_peak() {
    const ROUNDS: u32 = 500;
    const BLOCKS_PER_ROUND: u32 = 400;
    const RESETS_PER_ROUND: u32 = 100;
    // 200 000 blocks, below 2^24, so every block's number is an exact `f32`.
    const BLOCKS: u32 = ROUNDS * BLOCKS_PER_ROUND;
    let watch = Arc::new(Watch::default());
    let round = Arc::new(std::sync::Barrier::new(2));
    let audio = {
        let (watch, round) = (watch.clone(), round.clone());
        std::thread::spawn(move || {
            for first in (0..ROUNDS).map(|r| r * BLOCKS_PER_ROUND + 1) {
                round.wait();
                for block in first..first + BLOCKS_PER_ROUND {
                    watch.merge_peak(&[block as f32]);
                    watch.frames.fetch_add(1, Ordering::Release);
                }
                round.wait();
            }
        })
    };
    let mut peaks = Vec::new();
    let take = |peaks: &mut Vec<f32>| {
        let frames = watch.take_frames();
        let peak = watch.take_peak();
        if peak > 0.0 {
            peaks.push(peak);
        }
        frames
    };
    for r in 0..ROUNDS {
        round.wait();
        let mut frames: u64 = (0..RESETS_PER_ROUND).map(|_| take(&mut peaks)).sum();
        round.wait();
        frames += take(&mut peaks);
        assert_eq!(frames, u64::from(BLOCKS_PER_ROUND), "round {r}: frames were erased by a reset");
        assert_eq!(peaks.last().copied(), Some(((r + 1) * BLOCKS_PER_ROUND) as f32), "round {r}: its last block");
    }
    audio.join().unwrap();
    assert_eq!(peaks.last().copied(), Some(BLOCKS as f32));
    assert!(peaks.windows(2).all(|pair| pair[0] < pair[1]), "an old peak was written back");
}

#[test]
fn a_module_is_loaded_once_and_kept() {
    let (first, _) = open("test:vst3").unwrap();
    let (second, _) = open("test:vst3").unwrap();
    assert!(Rc::ptr_eq(&first, &second));
    let weak = Rc::downgrade(&first);
    drop((first, second));
    // Nothing uses it, and it stays loaded: the next open gets the same module, and its
    // `ExitDll` never runs while the process lives.
    let (third, _) = open("test:vst3").unwrap();
    assert!(weak.upgrade().is_some_and(|kept| Rc::ptr_eq(&kept, &third)));
}

#[test]
fn class_ids_round_trip_as_hex() {
    let cid: TUID = [0x12, -1, 0, 0x7f, 1, 2, 3, 4, 5, 6, 7, 8, 9, 10, 11, -128];
    let hex = cid_hex(&cid);
    let expected = if cfg!(windows) { "7F00FF120201040305060708090A0B80" } else { "12FF007F0102030405060708090A0B80" };
    assert_eq!(hex, expected);
    assert_eq!(cid_from_hex(&hex), Some(cid));
    assert_eq!(cid_from_hex("12"), None);
    assert_eq!(cid_from_hex(&"zz".repeat(16)), None);
}

#[test]
fn the_memory_stream_reads_writes_and_seeks() {
    let stream = ComWrapper::new(MemoryStream::new(Vec::new()));
    let raw = stream.to_com_ptr::<IBStream>().unwrap_or_else(|| panic!("stream"));
    let mut bytes = *b"hello";
    let mut count = 0;
    // SAFETY: a live stream and buffers that outlive the calls.
    unsafe {
        assert_eq!(raw.write(bytes.as_mut_ptr().cast(), 5, &mut count), kResultOk);
    }
    assert_eq!((stream.bytes(), count), (b"hello".to_vec(), 5));
    let mut at = 0;
    // SAFETY: as above.
    unsafe {
        assert_eq!(raw.seek(1, IBStream_::IStreamSeekMode_::kIBSeekSet as int32, &mut at), kResultOk);
    }
    let mut out = [0u8; 8];
    // SAFETY: as above.
    unsafe {
        assert_eq!(raw.read(out.as_mut_ptr().cast(), 8, &mut count), kResultOk);
    }
    assert_eq!((&out[..4], count), (&b"ello"[..], 4));
    // SAFETY: as above.
    let before_start = unsafe { raw.seek(-10, IBStream_::IStreamSeekMode_::kIBSeekCur as int32, &mut at) };
    assert_eq!(before_start, kInvalidArgument);
    // SAFETY: as above.
    assert_eq!(unsafe { raw.tell(&mut at) }, kResultOk);
    assert_eq!(at, 5);
}

#[test]
fn bundle_layout() {
    let missing = binary(Path::new("no-such.vst3"));
    assert!(missing.is_err());
}

#[test]
fn a_macos_bundles_executable_is_found() {
    let dir = std::env::temp_dir().join(format!("suhina-vst3-bundle-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    let stem = OsStr::new("Thing");
    let macos = dir.join("Thing.vst3").join("Contents").join("MacOS");
    std::fs::create_dir_all(&macos).unwrap();
    // An empty folder holds no executable; the bundle is refused.
    assert_eq!(macos_binary(&macos, stem), None);
    // Named after the bundle without the extension, as most bundles have.
    std::fs::write(macos.join("Thing"), "not really a Mach-O").unwrap();
    assert_eq!(macos_binary(&macos, stem), Some(macos.join("Thing")));
    // On macOS the bundle then loads by that file, whatever its name says.
    if cfg!(target_os = "macos") {
        let found = binary(&dir.join("Thing.vst3")).ok();
        assert_eq!(found, Some(macos.join("Thing")));
    }
    // An executable under another name, which the bundle format allows; a hidden file
    // such as a stray .DS_Store is not taken for it.
    let renamed = dir.join("Other.vst3").join("Contents").join("MacOS");
    std::fs::create_dir_all(&renamed).unwrap();
    std::fs::write(renamed.join(".DS_Store"), "x").unwrap();
    std::fs::write(renamed.join("Other.vst3"), "not really a Mach-O").unwrap();
    assert_eq!(macos_binary(&renamed, stem), Some(renamed.join("Other.vst3")));
    // A folder that is not there holds nothing.
    assert_eq!(macos_binary(&dir.join("missing").join("MacOS"), stem), None);
    let _ = std::fs::remove_dir_all(&dir);
}

/// The facts the module contract's null bundle arm waits on: a folder is the text
/// it carries (CoreFoundation's model is UTF-8, which a normal folder path is), a
/// bare file is no bundle, and a path that is not there is neither.
#[test]
fn a_bundle_folders_text_is_herself_and_the_other_arms_hold_none() {
    let dir = std::env::temp_dir().join(format!("suhina-vst3-bundle-text-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    assert_eq!(bundle_text(&dir).map(str::to_string), dir.to_str().map(str::to_string));
    // A bare file, though named like one, is no bundle.
    let file = dir.with_extension("vst3");
    std::fs::write(&file, b"not a bundle").unwrap();
    assert_eq!(bundle_text(&file), None);
    // A path that is not there is neither.
    assert_eq!(bundle_text(&dir.join("missing")), None);
    let _ = std::fs::remove_dir_all(&dir);
    let _ = std::fs::remove_file(&file);
}

/// A folder whose name is not UTF-8 stays the null arm. Makeable only where a file
/// system takes arbitrary bytes (Linux: macOS's APFS answers "Illegal byte
/// sequence" on creation and its names are UTF-8), which is where this runs — and
/// it can run only here, now that the facts stand in this module rather than in the
/// macOS files only.
#[cfg(all(unix, not(target_os = "macos")))]
#[test]
fn a_non_utf8_folder_stays_the_null_arm() {
    use std::os::unix::ffi::OsStringExt;
    // 0xFF is not UTF-8, so the folder's name cannot come out as `str`.
    let name = std::ffi::OsString::from_vec(
        format!("suhina-vst3-utf8-{}-", std::process::id())
            .into_bytes()
            .into_iter()
            .chain([0xFF, 0x2F, 0x80, 0x81])
            .collect(),
    );
    let folder = std::env::temp_dir().join(name);
    std::fs::create_dir_all(&folder).unwrap_or_else(|e| panic!("create the folder: {e}"));
    assert!(folder.is_dir(), "the folder exists (its name just is not UTF-8)");
    assert_eq!(bundle_text(&folder), None);
    let _ = std::fs::remove_dir_all(&folder);
}
