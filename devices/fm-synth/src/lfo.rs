// SPDX-License-Identifier: Apache-2.0

// Ported from MSFA (music-synthesizer-for-android) as found in Dexed: lfo.cc.
// Copyright 2013 Google Inc.; Copyright 2016-2025 Pascal Gauthier.
// Licensed under the Apache License, Version 2.0 (see LICENSE-APACHE in this crate).
// Changes: translated to Rust.

//! The LFO: one per synth, shared by every voice, advanced once per engine block.

use suhina_dsp::fm::Tables;

/// LFO frequency in hertz for each speed 0..=99, measured on hardware.
const SPEED_HZ: [f64; 100] = [
    0.062541, 0.125031, 0.312393, 0.437120, 0.624610, 0.750694, 0.936330, 1.125302, 1.249609, 1.436782, 1.560915,
    1.752081, 1.875117, 2.062494, 2.247191, 2.374451, 2.560492, 2.686728, 2.873976, 2.998950, 3.188013, 3.369840,
    3.500175, 3.682224, 3.812065, 4.000800, 4.186202, 4.310716, 4.501260, 4.623209, 4.814636, 4.930480, 5.121901,
    5.315191, 5.434783, 5.617346, 5.750431, 5.946717, 6.062811, 6.248438, 6.431695, 6.564264, 6.749460, 6.868132,
    7.052186, 7.250580, 7.375719, 7.556294, 7.687577, 7.877738, 7.993605, 8.181967, 8.372405, 8.504848, 8.685079,
    8.810573, 8.986341, 9.122423, 9.300595, 9.500285, 9.607994, 9.798158, 9.950249, 10.117361, 11.251125, 11.384335,
    12.562814, 13.676149, 13.904338, 15.092062, 16.366612, 16.638935, 17.869907, 19.193858, 19.425019, 20.833333,
    21.034918, 22.502250, 24.003841, 24.260068, 25.746653, 27.173913, 27.578599, 29.052876, 30.693677, 31.191516,
    32.658393, 34.317090, 34.674064, 36.416606, 38.197097, 38.550501, 40.387722, 40.749796, 42.625746, 44.326241,
    44.883303, 46.772685, 48.590865, 49.261084,
];

/// The LFO's settings, as stored in a voice.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct LfoSettings {
    /// 0..=99
    pub speed: u8,
    /// 0..=99
    pub delay: u8,
    /// Restart the cycle when a key goes down with no other key held.
    pub sync: bool,
    /// 0 triangle, 1 saw down, 2 saw up, 3 square, 4 sine, 5 sample and hold.
    pub wave: u8,
}

#[derive(Debug, Clone, Copy, Default)]
pub struct Lfo {
    phase: u32,
    delta: u32,
    wave: u8,
    random: u8,
    sync: bool,
    delay_state: u32,
    delay_inc: u32,
    delay_inc2: u32,
}

impl Lfo {
    /// Takes new settings; the phase and the delay carry on.
    pub fn set(&mut self, s: LfoSettings, t: &Tables) {
        let hz = SPEED_HZ.get(usize::from(s.speed)).copied().unwrap_or(0.0);
        self.delta = (hz * f64::from(t.lfo_ratio)) as u32;
        let a = 99 - i32::from(s.delay.min(99));
        if a == 99 {
            self.delay_inc = u32::MAX;
            self.delay_inc2 = u32::MAX;
        } else {
            let a = (16 + (a & 15)) << (1 + (a >> 4));
            self.delay_inc = t.lfo_unit.wrapping_mul(a as u32);
            let a = (a & 0xff80).max(0x80);
            self.delay_inc2 = t.lfo_unit.wrapping_mul(a as u32);
        }
        self.wave = s.wave;
        self.sync = s.sync;
    }

    /// The next value, 0..=1 << 24.
    pub fn next(&mut self, t: &Tables) -> i32 {
        self.phase = self.phase.wrapping_add(self.delta);
        let phase = self.phase;
        match self.wave {
            0 => {
                let x = phase >> 7;
                let x = if phase >> 31 != 0 { !x } else { x };
                (x & ((1 << 24) - 1)) as i32
            }
            1 => ((!phase ^ (1 << 31)) >> 8) as i32,
            2 => ((phase ^ (1 << 31)) >> 8) as i32,
            3 => (((!phase) >> 7) & (1 << 24)) as i32,
            4 => (1 << 23) + (t.sin((phase >> 8) as i32) >> 1),
            5 => {
                if phase < self.delta {
                    self.random = (u32::from(self.random) * 179 + 17) as u8;
                }
                let x = i32::from(self.random ^ 0x80);
                (x + 1) << 16
            }
            _ => 1 << 23,
        }
    }

    /// How far the delay has faded the LFO in, 0..=1 << 24.
    pub fn delay(&mut self) -> i32 {
        let inc = if self.delay_state < (1 << 31) { self.delay_inc } else { self.delay_inc2 };
        let d = u64::from(self.delay_state) + u64::from(inc);
        if d > u64::from(u32::MAX) {
            return 1 << 24;
        }
        self.delay_state = d as u32;
        if d < (1 << 31) { 0 } else { ((d >> 7) & ((1 << 24) - 1)) as i32 }
    }

    /// A key went down with no other key held.
    pub fn key_down(&mut self) {
        if self.sync {
            self.phase = (1 << 31) - 1;
        }
        self.delay_state = 0;
    }
}
