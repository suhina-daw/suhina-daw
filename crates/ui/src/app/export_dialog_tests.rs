// Copyright (C) 2026 The Suhina contributors
// SPDX-License-Identifier: GPL-3.0-or-later

use super::*;
use std::path::PathBuf;

#[test]
fn a_picked_file_reconciles_the_format_and_the_depth() {
    // A format the user chose explicitly follows the picked file's extension, so the job's
    // format and the file name agree — the command refuses the mismatch the other way.
    let mut wav = ExportDialog::new(PathBuf::from("/mixes/Song.wav"));
    wav.format = 1; // WAV, chosen explicitly; the depth is its default 32f.
    wav.accept_pick(Path::new("/mixes/Song.flac"));
    assert_eq!(wav.format, 2, "the picked flac turns the format row");
    assert_eq!(wav.resolved_depth(), Some("24"), "flac takes no float depth");
    let job = wav.job().unwrap();
    assert_eq!(job.format, Some("flac"));
    assert_eq!(job.bit_depth, Some("24"));
    assert_eq!(job.path, PathBuf::from("/mixes/Song.flac"));

    // With the automatic format, browsing to a flac moves the float depth out the same way.
    let mut auto = ExportDialog::new(PathBuf::from("/mixes/Song.wav"));
    auto.format = 0; // From the file name; the depth is 32f.
    auto.accept_pick(Path::new("/mixes/Song.flac"));
    assert_eq!(auto.format, 2);
    assert_eq!(auto.resolved_depth(), Some("24"), "{:?}", auto.job());

    // Ogg takes no depth, whatever the row held; a wav pick leaves a wav choice alone.
    let mut ogg = ExportDialog::new(PathBuf::from("/mixes/Song.wav"));
    ogg.format = 1;
    ogg.accept_pick(Path::new("/mixes/Song.ogg"));
    assert_eq!(ogg.format, 3);
    assert_eq!(ogg.resolved_depth(), None);
    let (_, name) = FORMATS[ogg.format];
    assert_eq!(name, Some("ogg"));

    // A file without one of the format's extensions reads as it did before the pick.
    let mut unknown = ExportDialog::new(PathBuf::from("/mixes/Song.wav"));
    unknown.format = 1;
    unknown.accept_pick(Path::new("/mixes/Song.aif"));
    assert_eq!(unknown.format, 1, "an unknown extension is the command's own refusal");
    assert_eq!(unknown.resolved_depth(), Some("32f"));
}

#[test]
fn choosing_the_format_swaps_the_file_names_extension() {
    assert_eq!(with_extension("/mixes/Song.wav", "ogg"), "/mixes/Song.ogg");
    assert_eq!(with_extension("Song", "flac"), "Song.flac");
    assert_eq!(with_extension("  /a/b/night.drive.WAV ", "ogg"), "/a/b/night.drive.ogg");
    assert_eq!(with_extension("", "wav"), "");
    // The separators the user typed stay as they typed them, a Windows path included.
    assert_eq!(with_extension(r"C:\mixes\Song.wav", "ogg"), r"C:\mixes\Song.ogg");
    // A folder's own path has no file to rename, and neither has "." or "..".
    assert_eq!(with_extension("/mixes/", "ogg"), "/mixes/");
    assert_eq!(with_extension(".", "ogg"), ".");
    assert_eq!(with_extension("..", "ogg"), "..");
    assert_eq!(with_extension("/a/b/Song", "mp3"), "/a/b/Song.mp3");
}

#[test]
fn the_jobs_params_carry_what_the_command_takes() {
    let job = ExportJob {
        path: "/mixes/Song.wav".into(),
        format: None,
        sample_rate: 48_000,
        bit_depth: Some("32f"),
        mp3_rate: None,
        normalize: false,
        dither: Some(false),
        overwrite: false,
    };
    // The depth's dither is asked for even when it is off, or the command would turn it on;
    // the bools that are false by default are left out.
    assert_eq!(
        job.params(),
        json!({ "path": "/mixes/Song.wav", "sampleRate": 48_000, "bitDepth": "32f", "dither": false })
    );
    let off = ExportJob {
        path: "/mixes/Song.wav".into(),
        format: Some("ogg"),
        sample_rate: 44_100,
        bit_depth: None,
        mp3_rate: None,
        normalize: true,
        dither: None,
        overwrite: true,
    };
    assert_eq!(
        off.params(),
        json!({ "path": "/mixes/Song.wav", "format": "ogg", "sampleRate": 44_100, "normalize": true, "overwrite": true })
    );
    let mp3 = ExportJob {
        path: "/mixes/Song.mp3".into(),
        format: Some("mp3"),
        sample_rate: 44_100,
        bit_depth: None,
        mp3_rate: Some("320"),
        normalize: false,
        dither: None,
        overwrite: false,
    };
    assert_eq!(
        mp3.params(),
        json!({ "path": "/mixes/Song.mp3", "format": "mp3", "sampleRate": 44_100, "mp3Rate": "320" })
    );
}

#[test]
fn ogg_takes_no_depth_and_32f_takes_no_dither() {
    let mut state = ExportDialog::new("/mixes/Song.wav".into());
    assert_eq!(state.resolved_format(), Some("wav"));
    assert_eq!(state.resolved_depth(), Some("32f"));
    assert!(!state.dither_applies(), "32f needs no dither");
    state.depth = 2;
    assert_eq!(state.resolved_depth(), Some("16"));
    assert!(state.dither_applies(), "16-bit takes dither");
    state.format = 3; // Ogg Vorbis
    assert_eq!(state.resolved_format(), Some("ogg"));
    assert_eq!(state.resolved_depth(), None);
    assert!(!state.dither_applies());
    assert_eq!(state.job().map(|job| job.bit_depth), Some(None), "the job leaves the depth out for ogg");
}

#[test]
fn mp3_takes_no_depth_and_carries_its_rate() {
    let mut state = ExportDialog::new("/mixes/Song.wav".into());
    state.rate = 96_000; // which the MP3 encoder does not write
    state.format = 4; // MP3
    assert_eq!(state.resolved_format(), Some("mp3"));
    assert_eq!(state.resolved_depth(), None);
    assert!(!state.dither_applies());
    // The rate fell back into the encoder's own list, whose last is 48 000.
    let job = state.job().unwrap();
    assert_eq!(job.sample_rate, 48_000);
    assert_eq!(job.mp3_rate, Some("320"), "320 kbps is the default rate");
    state.mp3_rate = 5; // VBR V2
    assert_eq!(state.job().unwrap().mp3_rate, Some("v2"));
    state.mp3_rate = 3; // 128 kbps
    assert_eq!(state.job().unwrap().mp3_rate, Some("128"));
    // Back to WAV, the rate chosen before comes back with it.
    state.format = 1;
    assert_eq!(state.job().unwrap().sample_rate, 96_000);
}

#[test]
fn switching_to_mp3_keeps_48_000() {
    // The default 48 000 is in both lists; it stays 48 000, not the place it had in the
    // other list (44 100 in mp3's).
    let mut state = ExportDialog::new("/mixes/Song.wav".into());
    state.format = 4; // MP3
    assert_eq!(state.job().unwrap().sample_rate, 48_000);
    let mut typed = ExportDialog::new("/mixes/Song.wav".into());
    typed.path = "/mixes/Song.mp3".into();
    assert_eq!(typed.job().unwrap().sample_rate, 48_000);
}

#[test]
fn a_file_named_mp3_is_an_mp3_from_the_file_name() {
    // "From the file name" with an .mp3 name: no depth sent, the mp3 rate sent, the
    // encoder's rates offered.
    let state = ExportDialog::new("/mixes/Song.MP3".into());
    assert_eq!(state.resolved_format(), Some("mp3"));
    let job = state.job().unwrap();
    assert_eq!(job.format, None);
    assert_eq!(job.bit_depth, None, "mp3 takes no depth");
    assert_eq!(job.mp3_rate, Some("320"));
    assert_eq!(state.rates(), &MP3_SAMPLE_RATES);
}

#[test]
fn flac_warns_at_the_float_depth_and_24_is_its_default() {
    let mut state = ExportDialog::new("/mixes/Song.wav".into());
    assert_eq!(state.refusal(), None, "wav at 32f is fine");
    state.format = 2; // FLAC, still at 32-bit float
    assert_eq!(state.refusal(), Some("flac is written at 24 or 16 bits, not 32f"));
    state.depth = 1; // 24-bit PCM
    assert_eq!(state.refusal(), None);
    assert!(state.dither_applies(), "24-bit flac takes dither");
    let job = state.job().unwrap();
    assert_eq!(job.bit_depth, Some("24"));
    // Dither starts on, as the command's and the spec's default do, and is asked for
    // explicitly so the command does not guess.
    assert_eq!(job.dither, Some(true));
    state.depth = 2; // 16-bit PCM
    assert_eq!(state.refusal(), None);
    assert_eq!(state.job().unwrap().bit_depth, Some("16"));
}
