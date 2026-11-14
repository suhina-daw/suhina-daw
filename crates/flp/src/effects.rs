// SPDX-License-Identifier: GPL-3.0-or-later
// Copyright (C) 2026 ItsAlphaNeon
// Copyright (C) 2026 The Suhina contributors

//! State layouts adapted from Lemon Studio os-engine/src/effects/{balance,eq,reverb,delay}.rs
//! at 011a03f23d9db7f10f07cd7d165cc978a9dda263, https://github.com/ItsAlphaNeon/Lemon-Studio.
//! Changes: map only known states to approximate Suhina substitutes; retain the source bytes.

use suhina_model::{Device, DeviceKind, DeviceParam, Extension};

pub(crate) fn substitute_with_budget(source: &Device, tempo: f64, budget: usize) -> Option<Vec<Device>> {
    substitute(source, tempo).filter(|devices| suhina_model::walk_devices(devices).len() <= budget)
}

fn int(bytes: &[u8], index: usize) -> Option<i32> {
    let at = index.checked_mul(4)?;
    Some(i32::from_le_bytes(bytes.get(at..at.checked_add(4)?)?.try_into().ok()?))
}

/// A supported substitute and an output-level correction, when wet/dry had separate gains.
pub(crate) fn substitute(source: &Device, tempo: f64) -> Option<Vec<Device>> {
    let DeviceKind::Foreign { id, .. } = &source.kind else {
        return None;
    };
    let bytes = source.state.as_deref()?;
    let name = id.to_ascii_lowercase();
    let mut params = Vec::new();
    let mut set = |id: &str, value| params.push(DeviceParam::new(id, value));
    let mut output = 1.;
    let (id, title) = match name.as_str() {
        "fruity balance" if bytes.len() == 8 => {
            let pan = f64::from(int(bytes, 0)?);
            let volume = f64::from(int(bytes, 1)?);
            if !(-128. ..=128.).contains(&pan) || !(0. ..=320.).contains(&volume) {
                return None;
            }
            let gain = (11f64.powf(volume / 256.) - 1.) / 10.;
            set("gain", if gain > 0. { 20. * gain.log10() } else { -120. });
            set("mute", if gain == 0. { 1. } else { 0. });
            set("balance", pan / 128.);
            ("utility", "Utility")
        }
        "fruity parametric eq 2" if bytes.len() >= 354 && int(bytes, 0)? == 8 => {
            for band in 0..7 {
                let kind = int(bytes, 22 + band)?;
                let target = match kind {
                    0 | 6 => 3.,
                    1 => 6.,
                    3 => 1.,
                    4 => 4.,
                    5 => 2.,
                    7 => 5.,
                    _ => return None,
                };
                let frequency = f64::from(int(bytes, 8 + band)?) / 65536.;
                let gain = f64::from(int(bytes, 1 + band)?) / 100.;
                if !(0. ..=1.).contains(&frequency) || !(-18. ..=18.).contains(&gain) {
                    return None;
                }
                set(&format!("a{}On", band + 1), if kind != 0 && bytes.get(318)? & (1 << band) != 0 { 1. } else { 0. });
                set(&format!("a{}Type", band + 1), target);
                set(&format!("a{}Gain", band + 1), gain.clamp(-15., 15.));
                set(&format!("a{}Frequency", band + 1), 20. * 1000f64.powf(frequency));
                // Width laws differ. Explicit rather than depending on a device preset.
                set(&format!("a{}Q", band + 1), std::f64::consts::FRAC_1_SQRT_2);
            }
            set("a8On", 0.);
            set("output", (f64::from(int(bytes, 36)?) / 100.).clamp(-12., 12.));
            ("eq", "EQ")
        }
        "fruity reeverb 2" if matches!(int(bytes, 0)?, 10000 | 10001) => {
            let version = int(bytes, 0)?;
            let flags = if version == 10001 { 64 } else { 56 };
            let sync = *bytes.get(flags)? != 0;
            let predelay = f64::from(int(bytes, 3)?);
            set("predelay", (if sync { predelay / 192. * 60. / tempo } else { predelay / 384. }).clamp(0.0005, 0.25));
            set("size", f64::from(int(bytes, 4)?).clamp(1., 100.));
            set("decay", (f64::from(int(bytes, 6)?) / 10.).clamp(0.2, 20.));
            set("diffusion", (f64::from(int(bytes, 5)?) / 100.).clamp(0.001, 0.96));
            set("highDampingFrequency", (f64::from(int(bytes, 7)?) * 100.).clamp(20., 16000.));
            let curve = |position: f64| (11f64.powf(position) - 1.) / 10.;
            let dry = curve(f64::from(int(bytes, 11)?).clamp(0., 160.) / 128.);
            let wet = curve(f64::from(int(bytes, 13)?).clamp(0., 160.) / 256.);
            output = dry.hypot(wet);
            set("mix", if output > 0. { wet.atan2(dry) / std::f64::consts::FRAC_PI_2 } else { 0. });
            ("reverb", "Reverb")
        }
        "fruity delay 3" if bytes.len() >= 108 && int(bytes, 0)? == 1 => {
            let time = f64::from(int(bytes, 5)?).clamp(1., 768.);
            let seconds = if int(bytes, 3)? != 0 {
                time / 192. * 60. / tempo
            } else {
                let x = (time - 1.) / 767. * 0.94 + 0.06;
                ((x.powi(3) - 0.000216) * 1.0002160466660799) * 0.999 + 0.001
            };
            set("syncLeft", 0.);
            set("syncRight", 0.);
            set("timeLeft", seconds.clamp(0.001, 5.));
            set("timeRight", seconds.clamp(0.001, 5.));
            set("feedback", (f64::from(int(bytes, 15)?) / 4800.).clamp(0., 0.95));
            set("pingPong", if int(bytes, 2)? == 2 { 1. } else { 0. });
            set("filter", 0.);
            let dry = f64::from(int(bytes, 26)?).clamp(0., 6000.) / 6000.;
            let wet = if int(bytes, 2)? == 3 { 0. } else { f64::from(int(bytes, 24)?).clamp(0., 6000.) / 6000. };
            output = dry + wet;
            set("mix", if output > 0. { wet / output } else { 0. });
            ("delay", "Delay")
        }
        _ => return None,
    };
    let mut result = Device::new(title, DeviceKind::Builtin(id.into()));
    result.enabled = source.enabled;
    result.params = params;
    result.extensions = source.extensions.clone();
    result.extensions.0.push(Extension { format: "flp".into(), key: source.name.clone(), data: bytes.to_vec() });
    let mut devices = vec![result];
    if (output - 1.).abs() > 1e-12 {
        let mut gain = Device::new("Effect output", DeviceKind::Builtin("utility".into()));
        gain.enabled = source.enabled;
        gain.params = vec![
            DeviceParam::new("gain", if output > 0. { 20. * output.log10() } else { -120. }),
            DeviceParam::new("mute", if output == 0. { 1. } else { 0. }),
        ];
        devices.push(gain);
    }
    Some(devices)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn foreign(name: &str, bytes: Vec<u8>) -> Device {
        let mut device = Device::new(name, DeviceKind::Foreign { format: "flp".into(), id: name.into() });
        device.state = Some(bytes.into());
        device
    }

    #[test]
    fn balance_unity_and_silence_are_explicit_and_unknown_states_stay_foreign() {
        let bytes = [0i32, 256].into_iter().flat_map(i32::to_le_bytes).collect::<Vec<_>>();
        let source = foreign("Fruity Balance", bytes.clone());
        let mapped = substitute(&source, 120.).unwrap();
        assert_eq!(mapped[0].kind, DeviceKind::Builtin("utility".into()));
        assert_eq!(mapped[0].params.iter().find(|p| p.id == "gain").unwrap().value, 0.);
        assert_eq!(mapped[0].extensions.0[0].data, bytes);
        for end in 0..bytes.len() {
            assert!(substitute(&foreign("Fruity Balance", bytes[..end].to_vec()), 120.).is_none());
        }
        let mute = substitute(&foreign("Fruity Balance", vec![0; 8]), 120.).unwrap();
        assert_eq!(mute[0].params.iter().find(|p| p.id == "mute").unwrap().value, 1.);
        assert!(substitute(&foreign("Unknown", bytes), 120.).is_none());
    }

    #[test]
    fn delay_keeps_two_independent_output_levels_and_bypass() {
        let mut knobs = [0i32; 27];
        knobs[0] = 1;
        knobs[2] = 2;
        knobs[3] = 1;
        knobs[5] = 192;
        knobs[15] = 2400;
        knobs[24] = 6000;
        knobs[26] = 6000;
        let bytes = knobs.into_iter().flat_map(i32::to_le_bytes).collect::<Vec<_>>();
        let mut source = foreign("Fruity Delay 3", bytes.clone());
        source.enabled = false;
        let mapped = substitute(&source, 120.).unwrap();
        assert_eq!(mapped.len(), 2);
        assert!(substitute_with_budget(&source, 120., 1).is_none());
        assert_eq!(substitute_with_budget(&source, 120., 2).unwrap().len(), 2);
        assert!(mapped.iter().all(|d| !d.enabled));
        let value = |id| mapped[0].params.iter().find(|p| p.id == id).unwrap().value;
        assert_eq!(value("timeLeft"), 0.5);
        assert_eq!(value("feedback"), 0.5);
        assert_eq!(value("mix"), 0.5);
        let gain = mapped[1].params.iter().find(|p| p.id == "gain").unwrap().value;
        assert!((10f64.powf(gain / 20.) * value("mix") - 1.).abs() < 1e-12);
        for end in 0..bytes.len() {
            assert!(substitute(&foreign("Fruity Delay 3", bytes[..end].to_vec()), 120.).is_none());
        }
    }

    #[test]
    fn reverb_dry_coefficient_survives_the_equal_power_mix_law() {
        let mut knobs = [0i32; 16];
        knobs[0] = 10001;
        knobs[4] = 50;
        knobs[6] = 15;
        knobs[11] = 128;
        knobs[13] = 128;
        let mut bytes = knobs.into_iter().flat_map(i32::to_le_bytes).collect::<Vec<_>>();
        bytes.extend([0, 0]);
        let mapped = substitute(&foreign("Fruity Reeverb 2", bytes), 120.).unwrap();
        let mix = mapped[0].params.iter().find(|p| p.id == "mix").unwrap().value;
        let gain_db = mapped[1].params.iter().find(|p| p.id == "gain").unwrap().value;
        let output = 10f64.powf(gain_db / 20.);
        assert!((output * (mix * std::f64::consts::FRAC_PI_2).cos() - 1.).abs() < 1e-12);
        assert!((output * (mix * std::f64::consts::FRAC_PI_2).sin() - (11f64.sqrt() - 1.) / 10.).abs() < 1e-12);
    }
}
