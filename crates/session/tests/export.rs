// Copyright (C) 2026 The Suhina contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! `project.export`'s tests: the export is the float render round-tripped through the file,
//! the range and tail bound it, the refusals say the spec's texts.

#![allow(clippy::unwrap_used, clippy::panic)] // test helpers outside #[test] functions

use std::path::{Path, PathBuf};

use serde_json::{Value, json};
use suhina_engine::model::*;
use suhina_session::Session;

const SR: u32 = 48_000;

fn run(session: &mut Session, params: Value) -> Result<Value, String> {
    session.execute("project.export", &params).map_err(String::from)
}

/// A file to export to, in a folder of its own that goes when the file does.
struct TempFile {
    path: PathBuf,
    _dir: crate::temp_dir::TempDir,
}

impl std::ops::Deref for TempFile {
    type Target = Path;

    fn deref(&self) -> &Path {
        &self.path
    }
}

impl AsRef<Path> for TempFile {
    fn as_ref(&self) -> &Path {
        &self.path
    }
}

fn temp_file(name: &str, extension: &str) -> TempFile {
    let dir = crate::temp_dir::TempDir::new(&format!("export-{name}"));
    TempFile { path: dir.join(format!("{name}.{extension}")), _dir: dir }
}

fn samples_of(path: &Path) -> Vec<f32> {
    hound::WavReader::open(path).unwrap().samples::<f32>().map(Result::unwrap).collect()
}

/// A project with one track ("Bass", one note on one beat) and its channel, ids assigned.
fn one_track() -> Project {
    let mut project = Project {
        tracks: vec![Track {
            content: ContentKinds { notes: true, audio: false },
            channel: Some(ChannelId(1)),
            ..Track::new(TrackId(1), "Bass")
        }],
        channels: vec![Channel::new(ChannelId(1), "Bass")],
        contents: vec![ClipContent {
            id: ContentId(1),
            name: "Riff".into(),
            length: 1.0,
            lanes: vec![ContentLane { target: None, data: LaneData::Notes(vec![Note::new(0.0, 1.0, 69, 0.8)]) }],
            extensions: Extensions::default(),
            scale: None,
        }],
        arrangement: Arrangement {
            lanes: vec![ArrangementLane {
                id: Default::default(),
                name: "Bass".into(),
                track: Some(TrackId(1)),
                clips: vec![Clip::new(ContentId(1), 0.0, 1.0)],
            }],
            markers: Vec::new(),
        },
        ..Project::default()
    };
    assert_eq!(project.assign_ids(), Ok(()));
    project
}

#[test]
fn the_whole_arrangement_export_round_trips_the_float_render() {
    let project = one_track();
    let mut session = Session::new(project.clone());
    let path = temp_file("whole", "wav");
    let result = run(&mut session, json!({ "path": path.display().to_string() })).unwrap();
    assert_eq!(result["format"], "wav");
    assert_eq!(result["bit_depth"], "32f");
    let written = samples_of(&path);
    let rendered = suhina_engine::render_offline(&project, SR, 512).unwrap();
    assert_eq!(written, rendered);
}

#[test]
fn an_empty_arrangement_refuses_to_export() {
    let mut project = suhina_session::blank_project();
    assert_eq!(project.assign_ids(), Ok(()));
    let mut session = Session::new(project);
    let path = temp_file("empty", "wav");
    let error = run(&mut session, json!({ "path": path.display().to_string() })).unwrap_err();
    assert_eq!(error, "the arrangement is empty; add a clip before exporting");
}

#[test]
fn preparation_warnings_reach_the_finished_export() {
    let mut session = Session::new(one_track());
    let mut data = session.export_data();
    data.warnings.push("Test instrument: reading its settings failed; using its last settings".into());
    let path = temp_file("state-warning", "wav");
    let result = suhina_session::export_from(&data, &json!({"path": path.display().to_string()})).unwrap();
    assert_eq!(result["warnings"], json!(data.warnings));
    assert!(samples_of(&path).iter().any(|sample| sample.abs() > 0.01));
}

#[test]
fn a_worker_export_keeps_the_commands_parameter_validation() {
    let mut session = Session::new(one_track());
    let data = session.export_data();
    let path = temp_file("unknown-parameter", "wav");
    let params = json!({"path": path.display().to_string(), "normalise": true});
    let direct = session.execute("project.export", &params).unwrap_err();
    let worker = suhina_session::export_from(&data, &params).unwrap_err();
    assert_eq!(worker.message(), direct.message());
    assert!(worker.message().contains("unknown parameter"));
    assert!(!path.exists());
}

#[test]
fn an_existing_file_is_never_replaced_without_overwrite() {
    let mut session = Session::new(one_track());
    let path = temp_file("again", "wav");
    run(&mut session, json!({ "path": path.display().to_string() })).unwrap();
    let error = run(&mut session, json!({ "path": path.display().to_string() })).unwrap_err();
    assert_eq!(error, format!("{} already exists; pass overwrite: true to replace it", path.display()));
    run(&mut session, json!({ "path": path.display().to_string(), "overwrite": true })).unwrap();
}

#[test]
fn a_flac_export_decodes_to_the_source() {
    let project = one_track();
    let mut session = Session::new(project.clone());
    let source = suhina_engine::render_offline(&project, SR, 512).unwrap();
    for depth in ["24", "16"] {
        let path = temp_file("flac", "flac");
        let result = run(&mut session, json!({ "path": path.display().to_string(), "bitDepth": depth })).unwrap();
        assert_eq!(result["format"], "flac");
        assert_eq!(result["bit_depth"], depth);
        let decoded = suhina_media::decode_file(&path).unwrap();
        assert_eq!(decoded.sample_rate, SR);
        assert_eq!(decoded.channels, 2);
        assert_eq!(decoded.samples.len(), source.len(), "the float render's length");
        // Within two least-significant bits of the dither's span.
        let lsb = if depth == "24" { 1.0 / 8_388_608.0 } else { 1.0 / 32_768.0 };
        let worst = source.iter().zip(&decoded.samples).fold(0.0f32, |m, (a, b)| m.max((a - b).abs()));
        assert!(worst <= 2.01 * lsb, "{depth}: worst {worst} against {lsb}");
    }
    // The float depth is refused, with the text the dialog's note shows before the click.
    let refused = temp_file("flac", "flac");
    let error = run(&mut session, json!({ "path": refused.display().to_string(), "bitDepth": "32f" })).unwrap_err();
    assert_eq!(error, "flac is written at 24 or 16 bits, not 32f");
}

#[test]
fn tail_goes_with_a_range() {
    let mut session = Session::new(one_track());
    let path = temp_file("tail", "wav");
    // A tail without a range says so, rather than being ignored.
    let error = run(&mut session, json!({ "path": path.display().to_string(), "tail": 2.0 })).unwrap_err();
    assert_eq!(error, "tail goes with range");
    // A negative one is refused on the path that reads it.
    let error = run(
        &mut session,
        json!({ "path": path.display().to_string(), "range": { "start": 0.0, "end": 1.0 }, "tail": -1.0 }),
    )
    .unwrap_err();
    assert_eq!(error, "tail must be at least 0 beats");
}

#[test]
fn a_range_over_the_sample_budget_is_refused_by_bytes() {
    let mut session = Session::new(one_track());
    let path = temp_file("budget", "wav");
    // 1 500 beats at the default tempo is 750 s; at 384 000 Hz that is 2.1 GiB of f32
    // samples, past the 2 GiB cap — refused before a byte is rendered.
    let error = run(
        &mut session,
        json!({ "path": path.display().to_string(), "sampleRate": 384_000, "range": { "start": 0.0, "end": 1500.0 } }),
    )
    .unwrap_err();
    assert!(error.starts_with("the export would be 2.1 GiB of samples"), "{error}");
    // The same range at 48 000 Hz is 0.3 GiB, under the cap and under the seconds cap too.
    let ok = run(&mut session, json!({ "path": path.display().to_string(), "range": { "start": 0.0, "end": 1500.0 } }))
        .unwrap();
    assert_eq!(ok["seconds"], 750.0);
}

#[test]
fn a_node_render_names_the_returns_its_sends_go_to() {
    // Bass sends to the return Echo; a node render of Bass leaves the return out and says so.
    let mut project = one_track();
    project.tracks.push(Track { channel: Some(ChannelId(7)), ..Track::new(TrackId(7), "Echo") });
    project.channels.push(Channel { role: ChannelRole::Effect, ..Channel::new(ChannelId(7), "Echo") });
    project.channels[0].sends = vec![Send { target: ChannelId(7), level: 1.0, pre_fader: false }];
    assert_eq!(project.assign_ids(), Ok(()));
    let mut session = Session::new(project.clone());
    let path = temp_file("send", "wav");
    let result = run(&mut session, json!({ "path": path.display().to_string(), "source": "Bass" })).unwrap();
    let warnings: Vec<String> = serde_json::from_value(result["warnings"].clone()).unwrap();
    assert_eq!(warnings, vec!["sends to Echo are not in this render".to_string()], "{warnings:?}");
    // The whole mix renders its sends, so it says nothing.
    let whole = run(&mut session, json!({ "path": path.display().to_string(), "overwrite": true })).unwrap();
    let warnings: Vec<String> = serde_json::from_value(whole["warnings"].clone()).unwrap();
    assert!(warnings.is_empty(), "{warnings:?}");
}

#[test]
fn an_mp3_export_decodes_to_the_signal_and_is_deterministic() {
    let project = one_track();
    let mut session = Session::new(project.clone());
    let source = suhina_engine::render_offline(&project, SR, 512).unwrap();
    let path = temp_file("mp3", "mp3");
    let result = run(&mut session, json!({ "path": path.display().to_string(), "mp3Rate": "320" })).unwrap();
    assert_eq!(result["format"], "mp3");
    // An mp3 has no bit depth, so the result names none.
    assert!(result["bit_depth"].is_null(), "{}", result["bit_depth"]);
    let decoded = suhina_media::decode_file(&path).unwrap();
    assert_eq!(decoded.sample_rate, SR);
    assert_eq!(decoded.channels, 2);
    // The encoder's delay and padding ride along; the length is within a few frames.
    assert!(
        ((decoded.samples.len() as i64) - source.len() as i64).abs() < 10_000,
        "decoded {} samples for {}",
        decoded.samples.len(),
        source.len()
    );
    let rms = |values: &[f32]| {
        let sum: f64 = values.iter().map(|s| f64::from(*s) * f64::from(*s)).sum();
        (sum / values.len().max(1) as f64).sqrt()
    };
    let source_rms = rms(&source);
    let decoded_rms = rms(&decoded.samples);
    let db = 20.0 * (decoded_rms / source_rms).log10();
    assert!(db.abs() < 1.5, "the mp3 decodes at {db:.2} dB of the source's level");
    // The same export writes the same bytes, header and LAME tag included.
    let second = temp_file("mp3-second", "mp3");
    run(&mut session, json!({ "path": second.display().to_string(), "mp3Rate": "320" })).unwrap();
    assert_eq!(std::fs::read(&path).unwrap(), std::fs::read(&second).unwrap());
}

#[test]
fn an_mp3_export_refuses_what_does_not_apply() {
    let mut session = Session::new(one_track());
    let path = temp_file("mp3-no", "mp3");
    // A rate the encoder does not write.
    let error = run(&mut session, json!({ "path": path.display().to_string(), "sampleRate": 96_000 })).unwrap_err();
    assert!(error.contains("96000 is not one the encoder writes"), "{error}");
    // A depth mp3 does not take.
    let error = run(&mut session, json!({ "path": path.display().to_string(), "bitDepth": "16" })).unwrap_err();
    assert_eq!(error, "bitDepth does not apply to mp3");
    // The mp3 rate goes with mp3 only.
    let wav = temp_file("mp3-no", "wav");
    let error = run(&mut session, json!({ "path": wav.display().to_string(), "mp3Rate": "320" })).unwrap_err();
    assert_eq!(error, "mp3Rate goes with mp3");
    // An unknown rate name.
    let error = run(&mut session, json!({ "path": path.display().to_string(), "mp3Rate": "v9" })).unwrap_err();
    assert_eq!(error, "v9 is not an mp3 rate; 128, 192, 256, 320, v0 or v2");
    // A format the command does not know is refused, not passed over for the extension; its
    // case does not matter.
    let error = run(&mut session, json!({ "path": wav.display().to_string(), "format": "aiff" })).unwrap_err();
    assert_eq!(error, "aiff is not a format; wav, flac, ogg or mp3");
    let error = run(&mut session, json!({ "path": wav.display().to_string(), "format": "MP3" })).unwrap_err();
    assert_eq!(error, "the file name says .wav, and the format says mp3");
    assert!(!wav.exists(), "a refused export wrote the file");
}
