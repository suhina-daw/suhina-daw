// Copyright (C) 2026 The Suhina contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! cargo run -p suhina-device-sdk --example sampler_composition -- target/sampler-composition
use std::{path::Path, sync::Arc};
use suhina_device_sdk::sample_resource::{PreparedSamples, SampleAsset, SliceMode};
use suhina_device_sdk::{
    Connection as C, DeviceGraph, DeviceRole, Node as N, NodeKind as K, ProcessContext, granular_template,
    sampler_template,
};
fn main() -> Result<(), Box<dyn std::error::Error>> {
    let directory = std::env::args().nth(1).ok_or("pass an output directory")?;
    let directory = Path::new(&directory);
    std::fs::create_dir_all(directory)?;
    let pcm = (0..48000).map(|i| (std::f32::consts::TAU * 220. * i as f32 / 48000.).sin() * 0.2).collect::<Vec<_>>();
    let mut samples = PreparedSamples::default();
    samples.insert("sample", Arc::new(SampleAsset::new(Arc::new(pcm), 48000, 1)?.with_tempo(120.)?))?;
    let mut loops = sampler_template("sample", 4)?;
    for p in &mut loops.parameters {
        if p.id == "playback" {
            p.default = 2.;
        }
        if p.id == "ping_pong" {
            p.default = 1.;
        }
    }
    let mut reverse = loops.clone();
    for p in &mut reverse.parameters {
        if p.id == "reverse" {
            p.default = 1.;
        }
    }
    let granular = granular_template("sample", 2, 16, 73)?;
    let mut slices = DeviceGraph::new("org.suhina.slices-example", "Slices", DeviceRole::Instrument);
    slices.nodes = vec![
        N::new("map", samples.slice_map("sample", SliceMode::TimeGrid { seconds: 0.25 })?),
        N::new("key", K::VoiceInput { controller: 1 }),
        N::new("index", K::KeyToSlice),
        N::new("select", K::SliceSelect),
        N::new("player", K::SlicePlayback { sample: "sample".into(), one_shot: true }),
        N::audio_output("out"),
    ];
    slices.nodes.push(N::new("count", K::Constant { value: 4. }));
    slices.connections = vec![
        C::ports("key", "key", "index", "key"),
        C::ports("count", "out", "index", "count"),
        C::ports("map", "out", "select", "map"),
        C::ports("index", "out", "select", "index"),
        C::ports("select", "out", "player", "region"),
        C::new("player", "out"),
    ];
    let mut stretch = DeviceGraph::new("org.suhina.stretch-example", "Tempo playback", DeviceRole::Instrument);
    stretch.nodes = vec![
        N::new("player", K::TimeStretchPlayer { sample: "sample".into() }),
        N::new("tempo", K::Tempo),
        N::new("source_tempo", K::Constant { value: 120. }),
        N::new("ratio", K::TempoRatio),
        N::audio_output("out"),
    ];
    stretch.connections = vec![
        C::ports("tempo", "out", "ratio", "target_bpm"),
        C::ports("source_tempo", "out", "ratio", "source_bpm"),
        C::ports("ratio", "out", "player", "speed"),
        C::new("player", "out"),
    ];
    for (name, graph) in
        [("loops", loops), ("reverse", reverse), ("slices", slices), ("granular", granular), ("tempo", stretch)]
    {
        let sd = graph.to_sd()?;
        let graph = DeviceGraph::from_sd(&sd)?;
        let mut processor = graph.compile_with_samples_at(48000., &samples)?;
        let mut audio = vec![0.; 48000 * 2];
        let context = ProcessContext { tempo: 150., beat: 0., playing: true };
        for (i, block) in audio.chunks_mut(256).enumerate() {
            let events =
                if i == 0 { &[(0, suhina_device_api::NoteEvent::On { key: 60, velocity: 0.8 })][..] } else { &[] };
            processor.process_instrument_context(&context, events, block);
        }
        if !audio.iter().any(|v| v.abs() > 0.001) {
            return Err(format!("{name} produced no audio").into());
        }
        std::fs::write(directory.join(format!("{name}.sd")), sd)?;
        write_wav(&directory.join(format!("{name}.wav")), &audio)?;
        println!("{name}: {} nodes, {} output frames", graph.nodes.len(), audio.len() / 2);
    }
    Ok(())
}
fn write_wav(path: &Path, audio: &[f32]) -> std::io::Result<()> {
    use std::io::Write;
    let bytes = u32::try_from(audio.len().saturating_mul(2)).map_err(std::io::Error::other)?;
    let mut file = std::fs::File::create(path)?;
    file.write_all(b"RIFF")?;
    file.write_all(&bytes.saturating_add(36).to_le_bytes())?;
    file.write_all(b"WAVEfmt ")?;
    for value in [16u32, 0x0002_0001, 48000, 192000, 0x0010_0004] {
        file.write_all(&value.to_le_bytes())?;
    }
    file.write_all(b"data")?;
    file.write_all(&bytes.to_le_bytes())?;
    for &sample in audio {
        file.write_all(&((sample.clamp(-1., 1.) * 32767.) as i16).to_le_bytes())?;
    }
    Ok(())
}
