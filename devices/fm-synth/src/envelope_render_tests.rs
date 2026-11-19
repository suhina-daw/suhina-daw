// Copyright (C) 2026 The Suhina contributors
// SPDX-License-Identifier: GPL-3.0-or-later

use super::*;

/// Self-authored modern-envelope scenario, frozen before the shared-envelope extraction.
#[test]
fn modern_envelopes_preserve_the_reference_render() {
    let mut s = synth();
    for (id, value) in [
        ("op1.envMode", 1.0),
        ("op1.env.attack", 0.01),
        ("op1.env.decay", 0.06),
        ("op1.env.release", 0.04),
        ("op1.env.sustain", -12.0),
        ("op1.env.end", -18.0),
        ("op1.env.timeByVel", 0.25),
        ("op1.env.timeByKey", 0.33),
        ("op1.env.attackSlope", 0.25),
        ("op1.env.decaySlope", -0.5),
        ("op1.env.releaseSlope", 0.75),
        ("op1.env.repeat", 0.0),
        ("time", -0.2),
        ("filterOn", 1.0),
        ("filterFreq", 900.0),
        ("filterEnv", 0.5),
        ("filterEnv.attack", 0.03),
        ("filterEnv.decay", 0.08),
        ("filterEnv.sustain", 0.3),
    ] {
        let i = index(id);
        assert_ne!(i, usize::MAX, "{id}");
        s.set_param(i, value);
    }
    s.reset();
    s.event(on(79, 0.73));
    let mut audio = vec![0.0; 512 * 128];
    for (block, out) in audio.as_chunks_mut::<128>().0.iter_mut().enumerate() {
        match block {
            96 => s.set_param(index("op1.env.sustain"), -8.0),
            160 => s.set_param(index("op1.env.loop"), 2.0),
            192 => s.set_param(index("time"), 0.2),
            224 => s.set_param(index("op1.env.loop"), 3.0),
            320 => s.event(off(79)),
            _ => {}
        }
        let context = ProcessContext { tempo: 120.0, beat: 0.1 + block as f64 * 64.0 / 24_000.0, playing: block < 288 };
        s.process(&context, out);
    }
    let bytes = include_bytes!("../tests/golden/time-envelope.f32");
    assert_eq!(bytes.len(), audio.len() * 4);
    assert!(audio.iter().any(|v| v.abs() > 0.01));
    for (i, (actual, bytes)) in audio.iter().zip(bytes.as_chunks::<4>().0).enumerate() {
        let expected = f32::from_le_bytes(*bytes);
        assert!(actual.is_finite() && (actual - expected).abs() < 1e-6, "sample {i}: {actual} != {expected}");
    }
}
