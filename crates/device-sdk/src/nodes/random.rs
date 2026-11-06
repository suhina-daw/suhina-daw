// Copyright (C) 2026 The Suhina contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! Deterministic held random controls and spread; seed state is private to an instance.
use super::{NodeInfo, PortInfo};
use crate::{NodeKind, ProcessContext, native_node::NativeNode};
use suhina_device_api::NoteEvent;
pub(crate) fn bipolar(seed: &mut u32) -> f64 {
    *seed = seed.wrapping_mul(1664525).wrapping_add(1013904223);
    f64::from(*seed) / f64::from(u32::MAX) * 2. - 1.
}
pub(crate) const RANDOM_HOLD: NodeInfo = NodeInfo {
    kind: "random_hold",
    name: "RandomHold",
    rust_variant: "RandomHold",
    inputs: &[
        PortInfo { name: "trigger", default: Some(0.0), unit: "signal" },
        PortInfo { name: "min", default: Some(-1.0), unit: "signal" },
        PortInfo { name: "max", default: Some(1.0), unit: "signal" },
        PortInfo { name: "events", default: Some(0.0), unit: "midi" },
    ],
    outputs: &["out", "left", "right"],
    description: "Seeded random value held between triggers or note-ons.",
    configuration: "seed u32",
    example: r#"{"kind":"random_hold","seed":1}"#,
};
pub(crate) const RANDOM_SPREAD: NodeInfo = NodeInfo {
    kind: "random_spread",
    name: "RandomSpread",
    rust_variant: "RandomSpread",
    inputs: &[
        PortInfo { name: "value", default: Some(0.0), unit: "signal" },
        PortInfo { name: "spread", default: Some(1.0), unit: "signal" },
        PortInfo { name: "trigger", default: Some(0.0), unit: "signal" },
        PortInfo { name: "min", default: Some(-3.4028234663852886e+38), unit: "signal" },
        PortInfo { name: "max", default: Some(3.4028234663852886e+38), unit: "signal" },
    ],
    outputs: &["out", "left", "right"],
    description: "Seeded held bipolar offset added to a live control.",
    configuration: "seed u32",
    example: r#"{"kind":"random_spread","seed":1}"#,
};

struct Kernel {
    spread: bool,
    initial: u32,
    seeds: [u32; 2],
    held: [f64; 2],
    trigger: [bool; 2],
    note: bool,
}
pub(crate) fn prepare(kind: &NodeKind) -> Option<Box<dyn NativeNode>> {
    let seed = match kind {
        NodeKind::RandomHold { seed } | NodeKind::RandomSpread { seed } | NodeKind::Jitter { seed } => *seed,
        _ => return None,
    };
    Some(Box::new(Kernel {
        spread: matches!(kind, NodeKind::RandomSpread { .. } | NodeKind::Jitter { .. }),
        initial: seed,
        seeds: [seed, seed ^ 0x9e3779b9],
        held: [0.; 2],
        trigger: [false; 2],
        note: false,
    }))
}
impl NativeNode for Kernel {
    fn prepare_block(&mut self, _: &ProcessContext) {}
    fn process(&mut self, inputs: [&[f32]; 5], output: &mut [f32]) {
        for (frame, pair) in output.as_chunks_mut::<2>().0.iter_mut().enumerate() {
            for (c, sample) in pair.iter_mut().enumerate() {
                let x: [f64; 5] = std::array::from_fn(|p| {
                    let v = inputs[p].get(frame * 2 + c).copied().unwrap_or(0.);
                    if v.is_finite() { f64::from(v) } else { 0. }
                });
                let trigger = x[if self.spread { 2 } else { 0 }] > 0.;
                if self.note || (trigger && (self.spread || !self.trigger[c])) {
                    self.held[c] = bipolar(&mut self.seeds[c]);
                }
                self.trigger[c] = trigger;
                let value = if self.spread {
                    x[0] + self.held[c] * x[1]
                } else {
                    x[1] + (self.held[c] + 1.) * 0.5 * (x[2] - x[1])
                };
                let (lo, hi) = if self.spread { (x[3], x[4]) } else { (x[1], x[2]) };
                *sample = value.clamp(lo.min(hi), lo.max(hi)) as f32;
            }
            self.note = false;
        }
    }
    fn event(&mut self, event: NoteEvent) {
        if !self.spread && matches!(super::events::normalized(event), Some(NoteEvent::On { .. })) {
            self.note = true;
        }
    }
    fn reset(&mut self) {
        self.seeds = [self.initial, self.initial ^ 0x9e3779b9];
        self.held = [0.; 2];
        self.trigger = [false; 2];
        self.note = false;
    }
    fn as_any_mut(&mut self) -> &mut dyn std::any::Any {
        self
    }
}

pub(crate) const JITTER: NodeInfo = NodeInfo {
    kind: "jitter",
    name: "Jitter",
    rust_variant: "Jitter",
    example: r#"{"kind":"jitter","seed":1}"#,
    ..RANDOM_SPREAD
};

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn held_random_and_consecutive_grain_spread_have_distinct_trigger_contracts() {
        for spread in [false, true] {
            let kind = if spread { NodeKind::RandomSpread { seed: 7 } } else { NodeKind::RandomHold { seed: 7 } };
            let mut kernel = prepare(&kind).unwrap();
            let inputs = if spread {
                [[0.; 8], [1.; 8], [1.; 8], [-1.; 8], [1.; 8]]
            } else {
                [[1.; 8], [-1.; 8], [1.; 8], [0.; 8], [0.; 8]]
            };
            let mut a = [0.; 8];
            kernel.process(inputs.each_ref().map(|v| v.as_slice()), &mut a);
            if spread {
                assert_ne!(a[0], a[2]);
            } else {
                assert_eq!(a[0], a[2]);
            }
            kernel.reset();
            let mut b = [0.; 8];
            kernel.process(inputs.each_ref().map(|v| v.as_slice()), &mut b);
            assert_eq!(a, b);
        }
    }
}
