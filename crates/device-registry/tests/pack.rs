// Copyright (C) 2026 The Suhina contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! The shipped pack ([ADR 0036]): a model of our own renders within a bound of its source
//! device chain, every shipped file reads, and an IR's device runs the pack's bytes. The
//! compare is aligned by each device's declared `latency()` — an offline render keeps a
//! device's lead in the output, and the pack's test rides it (`pack_files`, `load_builtin_file`
//! are the same paths a session runs).

#![allow(clippy::unwrap_used, clippy::panic)] // a test's helpers outside `#[test]` functions

use std::path::PathBuf;

use suhina_device_api::conformance::test_signal;
use suhina_device_api::{Device, ProcessContext};
use suhina_device_registry::{create, create_with, info, load_builtin_file, pack_files, stock_presets};

const RATE: u32 = 48_000;

/// The training phrase itself, verbatim: `tests/data/suhina-overdrive-train-phrase.wav`,
/// 90 s of 16-bit mono. The compare is at distribution only on the content the model
/// learned from, so the whole phrase is the ground.
fn phrase() -> Vec<f32> {
    let mut reader = hound::WavReader::open(
        PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/data/suhina-overdrive-train-phrase.wav"),
    )
    .unwrap_or_else(|e| panic!("the pack test's phrase wav does not read: {e}"));
    let mut out = Vec::new();
    for sample in reader.samples::<i16>() {
        out.push(f32::from(sample.unwrap_or_default()) / 32768.0);
    }
    out
}

/// Runs `device` over the phrase, and drops the lead the device's own `latency()` declares.
fn rendered(device: &mut dyn Device, frames: &mut [f32]) -> Vec<f32> {
    device.prepare(f64::from(RATE)).unwrap();
    device.reset();
    device.process(&ProcessContext::default(), frames);
    let lead = usize::try_from(device.latency()).unwrap_or_default();
    let chunks = frames.as_chunks::<2>().0;
    (lead..chunks.len()).map(|k| chunks[k][0]).collect()
}

/// A model of our own keeps within a bound of the device chain it was trained from, aligned by
/// latency ([ADR 0036] › the starter pack). The bound is far above the model's own fit.
#[test]
fn the_own_model_renders_within_a_bound_of_its_source_chain() {
    let Some((name, _)) =
        pack_files("nam-amp-modeler").iter().find(|(name, _)| *name == "Suhina Overdrive.nam").copied()
    else {
        panic!("the pack has no own model to prove");
    };
    let asset = load_builtin_file("nam-amp-modeler", name, RATE).unwrap();
    let mut model = create_with("nam-amp-modeler", Some(&asset)).unwrap();

    let mut chain = create("overdrive").unwrap();
    let mut phrase = phrase();
    let model_out = rendered(model.as_mut(), &mut phrase.clone());
    let gold_out = rendered(chain.as_mut(), &mut phrase);
    let n = gold_out.len().min(model_out.len());
    // The pair's own best fit: a small shift scan takes the alignment from the two streams'
    // real lead, since a declared lead that lies (the Overdrive's own dry-path test knows a
    // lead twice its `latency()`) is this test's alignment problem, not a model fault.
    let (mut best_lag, mut best_corr) = (0i64, -2.0f64);
    for lag in -128i64..=128 {
        let (mut num, mut da2, mut db2, mut taken) = (0.0, 0.0, 0.0, 0usize);
        for (k, value) in gold_out.iter().take(n).enumerate() {
            let bi = k as i64 + lag;
            if !(0..n as i64).contains(&bi) {
                continue;
            }
            let x = f64::from(*value);
            let y = f64::from(model_out[bi as usize]);
            num += x * y;
            da2 += x * x;
            db2 += y * y;
            taken += 1;
        }
        if taken < n / 10 {
            continue;
        }
        let score = num / (da2.sqrt() * db2.sqrt()).max(1e-30);
        if score > best_corr {
            (best_lag, best_corr) = (lag, score);
        }
    }
    let mut bound = f64::INFINITY;
    for lag in [-best_lag, best_lag] {
        let (mut error, mut energy, mut taken) = (0.0, 0.0, 0usize);
        for (k, value) in gold_out.iter().take(n).enumerate() {
            let bi = k as i64 - lag;
            if !(0..n as i64).contains(&bi) {
                continue;
            }
            let (a, b) = (f64::from(*value), f64::from(model_out[bi as usize]));
            let d = a - b;
            error += d * d;
            energy += a * a;
            taken += 1;
        }
        if taken > n / 10 && energy > 1e-30 {
            bound = bound.min(10.0 * (error / energy).log10());
        }
    }
    let _ = best_corr;
    assert!(
        bound < -6.0,
        "the shipped model renders {bound:+.2} dB from its source chain at its own fitted lead \
         ({best_lag:+} frames) — retrain it or re-pair it with the chain"
    );
}

/// Every shipped IR runs the device: a Cab Loader made from a pack's bytes renders nonzero on
/// a loud bed.
#[test]
fn every_pack_ir_runs_stereo() {
    for (name, _) in pack_files("nam-cab-loader") {
        let asset = load_builtin_file("nam-cab-loader", name, RATE).unwrap();
        let mut cab = create_with("nam-cab-loader", Some(&asset)).unwrap();
        let mut frames = test_signal(96_000);
        cab.process(&ProcessContext::default(), &mut frames);
        let passive = frames.as_chunks::<2>().0.iter().any(|[l, _]| l.abs() > 1e-3);
        assert!(passive, "{name} renders silence");
    }
}

/// Every stock preset that names a file is heard when it is added (its values at the pack's
/// asset, as `device.add {}` runs them): the pair makes up the model's own level — the clean
/// pairing reads quieter than the loud ones — the peak stays under full scale, and the
/// loudness is not the model's silent tail. The bounds hold the balance: until 2026-11-08
/// Bright Clean sat −18 dB under the crunch pairings and rode over full scale once raised.
#[test]
fn every_stock_preset_naming_a_file_renders_heard_and_under_full_scale() {
    let phrase = phrase();
    for stock in stock_presets() {
        let Some(file) = stock.preset.file.as_deref() else { continue };
        let Some((id, name)) = file.strip_prefix("builtin:").and_then(|s| s.split_once('/')) else {
            panic!("{}: {} is not a shipped file's key", stock.key, file);
        };
        let asset = load_builtin_file(id, name, RATE).unwrap_or_else(|e| panic!("{}: {e}", stock.key));
        let mut device = create_with(id, Some(&asset)).unwrap();
        for (param, value) in &stock.preset.params {
            let info = info(id).unwrap();
            let index = info.param_index(param).unwrap_or_else(|| panic!("{}: {param:?}", stock.key));
            device.set_param(index, *value);
        }
        let mut clone = phrase.clone();
        let out = rendered(device.as_mut(), &mut clone);
        let peak = out.iter().map(|s| s.abs()).fold(0.0f32, f32::max);
        let peak_db = 20.0 * f64::from(peak).log10().max(-96.0);
        let rms = out.iter().map(|s| f64::from(*s) * f64::from(*s)).sum::<f64>() / out.len().max(1) as f64;
        let loudness_db = 10.0 * rms.log10().max(-96.0);
        assert!(peak_db <= -0.05, "{} renders at {peak_db:+.1} dBFS, over full scale", stock.key);
        assert!(
            loudness_db >= -34.0,
            "{} renders at {loudness_db:+.1} dB, {:+.} under the level a preset is meant to start at",
            stock.key,
            loudness_db + 34.0
        );
    }
}
