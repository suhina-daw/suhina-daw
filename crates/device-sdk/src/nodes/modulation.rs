// Copyright (C) 2026 The Suhina contributors
// SPDX-License-Identifier: GPL-3.0-or-later

use super::{NodeInfo, PortInfo};

pub const OSCILLATOR: NodeInfo = NodeInfo {
    kind: "oscillator",
    name: "Oscillator",
    inputs: &[PortInfo { name: "frequency", default: Some(1.0), unit: "Hz" }],
    outputs: &["out", "left", "right"],
    description: "Free-running phase in cycles, frequency limited to +/-0.49*sample rate. Starts at phase zero on reset. Waveforms are not band-limited.",
    configuration: " shape: Waveform",
    example: "{\"kind\":\"oscillator\",\"shape\":\"sine\"}",
    rust_variant: "Oscillator",
};

pub const SYNCED_LFO: NodeInfo = NodeInfo {
    kind: "synced_lfo",
    name: "Synced LFO",
    inputs: &[PortInfo { name: "rate", default: Some(1.0), unit: "cycles/beat" }],
    outputs: &["out", "left", "right"],
    description: "Waveform at transport beat * rate; follows song position, including stopped transport. No phase state.",
    configuration: " shape: Waveform",
    example: "{\"kind\":\"synced_lfo\",\"shape\":\"sine\"}",
    rust_variant: "SyncedLfo",
};

pub const NOISE: NodeInfo = NodeInfo {
    kind: "noise",
    name: "Noise",
    inputs: &[],
    outputs: &["out", "left", "right"],
    description: "Deterministic xorshift32 white noise -1..1, broadcast to both channels; seed zero becomes one. Reset restores seed.",
    configuration: " seed: u32",
    example: "{\"kind\":\"noise\",\"seed\":1}",
    rust_variant: "Noise",
};

pub const SMOOTH: NodeInfo = NodeInfo {
    kind: "smooth",
    name: "Smooth",
    inputs: &[
        PortInfo { name: "in", default: None, unit: "signal" },
        PortInfo { name: "time", default: Some(0.02), unit: "seconds" },
    ],
    outputs: &["out", "left", "right"],
    description: "One-pole smoothing per channel, time 0..60 seconds; zero is instantaneous. State resets to zero.",
    configuration: "none",
    example: "{\"kind\":\"smooth\"}",
    rust_variant: "Smooth",
};

pub const ENVELOPE: NodeInfo = NodeInfo {
    kind: "envelope",
    name: "Envelope",
    inputs: &[
        PortInfo { name: "in", default: None, unit: "signal" },
        PortInfo { name: "attack", default: Some(0.01), unit: "seconds" },
        PortInfo { name: "release", default: Some(0.1), unit: "seconds" },
    ],
    outputs: &["out", "left", "right"],
    description: "Per-channel peak follower, or exponential mean-square follower followed by sqrt when rms=true. Attack/release 0..60 seconds. State resets to zero.",
    configuration: " rms: bool",
    example: "{\"kind\":\"envelope\",\"rms\":false}",
    rust_variant: "Envelope",
};

pub const SAMPLE_HOLD: NodeInfo = NodeInfo {
    kind: "sample_hold",
    name: "Sample Hold",
    inputs: &[
        PortInfo { name: "in", default: None, unit: "signal" },
        PortInfo { name: "trigger", default: None, unit: "signal" },
    ],
    outputs: &["out", "left", "right"],
    description: "Captures input on each nonpositive-to-positive trigger transition per channel. Held value and previous trigger reset to zero.",
    configuration: "none",
    example: "{\"kind\":\"sample_hold\"}",
    rust_variant: "SampleHold",
};

pub const FILTER: NodeInfo = NodeInfo {
    kind: "filter",
    name: "Filter",
    inputs: &[
        PortInfo { name: "in", default: None, unit: "signal" },
        PortInfo { name: "frequency", default: Some(1000.0), unit: "Hz" },
        PortInfo { name: "q", default: Some(std::f64::consts::FRAC_1_SQRT_2), unit: "Q" },
        PortInfo { name: "gain", default: Some(0.0), unit: "dB" },
    ],
    outputs: &["out", "left", "right"],
    description: "Stereo biquad using suhina-dsp; frequency 1..0.49*sample rate, Q 0.025..100, gain +/-48 dB. Independent channel state resets to zero. Modulation updates coefficients only when controls change.",
    configuration: " mode: FilterMode",
    example: "{\"kind\":\"filter\",\"mode\":\"lowpass\"}",
    rust_variant: "Filter",
};

pub const SAMPLE_DELAY: NodeInfo = NodeInfo {
    kind: "sample_delay",
    name: "Sample Delay",
    inputs: &[PortInfo { name: "in", default: None, unit: "signal" }],
    outputs: &["out", "left", "right"],
    description: "One-frame stereo delay; reads previous input before current input is committed. Breaks feedback cycles. Reset clears history.",
    configuration: "none",
    example: "{\"kind\":\"sample_delay\"}",
    rust_variant: "SampleDelay",
};

pub const DELAY: NodeInfo = NodeInfo {
    kind: "delay",
    name: "Delay",
    inputs: &[
        PortInfo { name: "in", default: None, unit: "signal" },
        PortInfo { name: "time", default: Some(0.0), unit: "seconds" },
        PortInfo { name: "mode", default: Some(2.0), unit: "0 repitch, 1 fade, 2 jump" },
    ],
    outputs: &["out", "left", "right"],
    description: "Fractional stereo delay with independent channel times and linear interpolation, time limited to one frame..max_seconds. Mode 0 slews time with a 20 ms one-pole (repitch), 1 crossfades read heads over 20 ms, 2 jumps to the new time (the default for older graphs). max_seconds must be >0 and <=60. Breaks feedback only through in port; time remains an ordinary dependency. Reset clears history.",
    configuration: " max_seconds: f64",
    example: "{\"kind\":\"delay\",\"max_seconds\":0.01}",
    rust_variant: "Delay",
};

pub(crate) const RANGE_MAP: NodeInfo = NodeInfo {
    kind: "range_map",
    name: "RangeMap",
    rust_variant: "RangeMap",
    inputs: &[
        PortInfo { name: "value", default: Some(0.0), unit: "signal" },
        PortInfo { name: "in_min", default: Some(0.0), unit: "signal" },
        PortInfo { name: "in_max", default: Some(1.0), unit: "signal" },
        PortInfo { name: "out_min", default: Some(0.0), unit: "signal" },
        PortInfo { name: "out_max", default: Some(1.0), unit: "signal" },
    ],
    outputs: &["out", "left", "right"],
    description: "Affine range conversion with safe zero input span.",
    configuration: "none",
    example: r#"{"kind":"range_map"}"#,
};
fn range_map(x: [f64; 5]) -> f64 {
    if x[2] == x[1] { x[3] } else { x[3] + (x[0] - x[1]) / (x[2] - x[1]) * (x[4] - x[3]) }
}

pub(crate) const CURVE: NodeInfo = NodeInfo {
    kind: "curve",
    name: "Curve",
    rust_variant: "Curve",
    inputs: &[
        PortInfo { name: "value", default: Some(0.0), unit: "signal" },
        PortInfo { name: "exponent", default: Some(1.0), unit: "signal" },
    ],
    outputs: &["out", "left", "right"],
    description: "Normalized power curve for controls and velocity.",
    configuration: "none",
    example: r#"{"kind":"curve"}"#,
};
fn curve(x: [f64; 5]) -> f64 {
    x[0].clamp(0., 1.).powf(x[1].clamp(0.001, 1000.))
}

pub(crate) fn prepare(kind: &super::NodeKind) -> Option<Box<dyn crate::native_node::NativeNode>> {
    match kind {
        super::NodeKind::RangeMap => Some(super::processor::boxed(range_map)),
        super::NodeKind::Curve | super::NodeKind::VelocityCurve => Some(super::processor::boxed(curve)),
        super::NodeKind::KeyTracking => Some(super::processor::boxed(key_tracking)),
        _ => None,
    }
}

pub(crate) const GLIDE: NodeInfo = NodeInfo {
    kind: "glide",
    name: "Glide",
    rust_variant: "Glide",
    inputs: &[
        PortInfo { name: "target", default: Some(0.), unit: "signal" },
        PortInfo { name: "time", default: Some(0.1), unit: "seconds" },
        PortInfo { name: "gate", default: Some(1.), unit: "signal" },
        PortInfo { name: "legato", default: Some(1.), unit: "signal" },
    ],
    outputs: &["out", "left", "right"],
    description: "Exponential pitch glide; gate rise starts at target in legato mode.",
    configuration: "none",
    example: r#"{"kind":"glide"}"#,
};
struct Glide {
    rate: f64,
    value: [f64; 2],
    gate: [bool; 2],
}
pub(crate) fn prepare_glide(kind: &super::NodeKind, rate: f64) -> Option<Box<dyn crate::native_node::NativeNode>> {
    matches!(kind, super::NodeKind::Glide)
        .then(|| Box::new(Glide { rate, value: [0.; 2], gate: [false; 2] }) as Box<dyn crate::native_node::NativeNode>)
}
impl crate::native_node::NativeNode for Glide {
    fn prepare_block(&mut self, _: &crate::ProcessContext) {}
    fn process(&mut self, inputs: [&[f32]; 5], output: &mut [f32]) {
        for (frame, pair) in output.as_chunks_mut::<2>().0.iter_mut().enumerate() {
            for (c, sample) in pair.iter_mut().enumerate() {
                let x: [f64; 4] = std::array::from_fn(|p| {
                    let v = inputs[p].get(frame * 2 + c).copied().unwrap_or(0.);
                    if v.is_finite() { f64::from(v) } else { 0. }
                });
                let gate = x[2] > 0.;
                if gate {
                    if x[3] > 0. && !self.gate[c] {
                        self.value[c] = x[0];
                    } else {
                        let a = crate::modulation::coefficient(x[1], self.rate);
                        self.value[c] = x[0] + a * (self.value[c] - x[0]);
                    }
                }
                self.gate[c] = gate;
                *sample = self.value[c] as f32;
            }
        }
    }
    fn event(&mut self, _: suhina_device_api::NoteEvent) {}
    fn reset(&mut self) {
        self.value = [0.; 2];
        self.gate = [false; 2];
    }
    fn as_any_mut(&mut self) -> &mut dyn std::any::Any {
        self
    }
}

pub(crate) const VELOCITY_CURVE: NodeInfo = NodeInfo {
    kind: "velocity_curve",
    name: "VelocityCurve",
    rust_variant: "VelocityCurve",
    example: r#"{"kind":"velocity_curve"}"#,
    ..CURVE
};
pub(crate) const KEY_TRACKING: NodeInfo = NodeInfo {
    kind: "key_tracking",
    name: "KeyTracking",
    rust_variant: "KeyTracking",
    inputs: &[
        PortInfo { name: "key", default: Some(60.), unit: "key" },
        PortInfo { name: "root", default: Some(60.), unit: "key" },
        PortInfo { name: "amount", default: Some(1.), unit: "signal/key" },
        PortInfo { name: "offset", default: Some(0.), unit: "signal" },
    ],
    outputs: &["out", "left", "right"],
    description: "Linear key-distance modulation relative to a root.",
    configuration: "none",
    example: r#"{"kind":"key_tracking"}"#,
};
fn key_tracking(x: [f64; 5]) -> f64 {
    x[3] + (x[0] - x[1]) * x[2]
}
pub(crate) const LFO: NodeInfo = NodeInfo {
    kind: "lfo",
    name: "LFO",
    rust_variant: "Lfo",
    inputs: &[
        PortInfo { name: "frequency", default: Some(1.), unit: "Hz" },
        PortInfo { name: "reset", default: Some(0.), unit: "signal" },
        PortInfo { name: "phase", default: Some(0.), unit: "cycles" },
        PortInfo { name: "events", default: Some(0.), unit: "midi" },
    ],
    outputs: &["out", "left", "right"],
    description: "Free LFO with phase offset, rising reset and event retrigger using shared waveforms.",
    configuration: "shape",
    example: r#"{"kind":"lfo","shape":"sine"}"#,
};
struct Lfo {
    shape: crate::Waveform,
    rate: f64,
    phase: [f64; 2],
    reset: [bool; 2],
    note: bool,
}
pub(crate) fn prepare_lfo(kind: &super::NodeKind, rate: f64) -> Option<Box<dyn crate::native_node::NativeNode>> {
    if let super::NodeKind::Lfo { shape } = kind {
        Some(Box::new(Lfo { shape: *shape, rate, phase: [0.; 2], reset: [false; 2], note: false }))
    } else {
        None
    }
}
impl crate::native_node::NativeNode for Lfo {
    fn prepare_block(&mut self, _: &crate::ProcessContext) {}
    fn process(&mut self, inputs: [&[f32]; 5], output: &mut [f32]) {
        for (frame, pair) in output.as_chunks_mut::<2>().0.iter_mut().enumerate() {
            for (c, sample) in pair.iter_mut().enumerate() {
                let x: [f64; 3] = std::array::from_fn(|p| {
                    let v = inputs[p].get(frame * 2 + c).copied().unwrap_or(0.);
                    if v.is_finite() { f64::from(v) } else { 0. }
                });
                let reset = x[1] > 0.;
                if self.note || (reset && !self.reset[c]) {
                    self.phase[c] = 0.;
                }
                self.reset[c] = reset;
                *sample = suhina_dsp::lfo::wave(self.shape.dsp(), self.phase[c] + x[2]) as f32;
                self.phase[c] += x[0].clamp(-self.rate * 0.49, self.rate * 0.49) / self.rate;
                if self.phase[c].abs() > 4294967296. {
                    self.phase[c] = self.phase[c].rem_euclid(4294967296.);
                }
            }
            self.note = false;
        }
    }
    fn event(&mut self, event: suhina_device_api::NoteEvent) {
        if matches!(super::events::normalized(event), Some(suhina_device_api::NoteEvent::On { .. })) {
            self.note = true;
        }
    }
    fn reset(&mut self) {
        self.phase = [0.; 2];
        self.reset = [false; 2];
        self.note = false;
    }
    fn as_any_mut(&mut self) -> &mut dyn std::any::Any {
        self
    }
}
