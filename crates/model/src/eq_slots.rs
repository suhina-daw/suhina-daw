// Copyright (C) 2026 The Suhina contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! The built-in EQ's values as they were before its 24 band slots, moved onto the slots
//! (docs/spec/eq.md › Projects saved with two curves). The EQ had eight bands in two curves, A
//! (`a1…a8`) and B (`b1…b8`), and a global Mode: Stereo ran curve A on both sides, Left/Right ran
//! A on the left and B on the right, Mid/Side A on the mid and B on the side. Now every band has
//! its own placement: curve A stays bands 1–8, curve B becomes bands 9–16, and the Mode becomes
//! the placements of those sixteen bands, so the EQ sounds as it did.

use crate::{Device, DeviceKind, DeviceParam};

/// The id a two-curve EQ's parameter has now: `b<n><Field>` is band 8 + n; any other id is kept.
pub fn eq_slot_id(id: &str) -> Option<String> {
    let rest = id.strip_prefix('b')?;
    let digits = rest.chars().take_while(char::is_ascii_digit).count();
    let band: usize = rest.get(..digits)?.parse().ok()?;
    let field = rest.get(digits..)?;
    ((1..=8).contains(&band) && ["On", "Type", "Frequency", "Gain", "Q"].contains(&field))
        .then(|| format!("a{}{field}", band + 8))
}

/// Moves a two-curve EQ's values onto the band slots: curve B's to bands 9–16, and the Mode, which
/// it removes, to the bands' placements (Left/Right: 1–8 Left, 9–16 Right; Mid/Side: 1–8 Mid,
/// 9–16 Side) with bands 9–16 in use. Values already in the slots' ids are left as they are, so
/// it does nothing to an EQ saved with slots.
pub fn eq_two_curves_to_slots(params: &mut Vec<DeviceParam>) {
    let mode = params.iter().find(|p| p.id == "mode").map(|p| p.value.round());
    params.retain(|p| p.id != "mode");
    for param in params.iter_mut() {
        if let Some(id) = eq_slot_id(&param.id) {
            param.id = id;
        }
    }
    let places = match mode.map(|m| m as i64) {
        Some(1) => Some((1.0, 2.0)),
        Some(2) => Some((3.0, 4.0)),
        _ => None,
    };

    if let Some((first, second)) = places {
        let mut set = |id: String, value: f64| match params.iter_mut().find(|p| p.id == id) {
            Some(param) => param.value = value,
            None => params.push(DeviceParam::new(id, value)),
        };
        for band in 1..=8 {
            set(format!("a{band}Place"), first);
            set(format!("a{}Place", band + 8), second);
            set(format!("a{}InUse", band + 8), 1.0);
        }
    }
}

/// [`eq_two_curves_to_slots`] for a device, when it is the built-in EQ and saved with two
/// curves: its values, and its automation lanes' parameters with them. A lane of the Mode, which
/// has no slot, is dropped.
pub fn eq_device_to_slots(device: &mut Device) {
    if device.kind != DeviceKind::Builtin("eq".into()) {
        return;
    }
    let legacy = |id: &str| id == "mode" || eq_slot_id(id).is_some();
    if !device.params.iter().any(|p| legacy(&p.id)) && !device.automation.iter().any(|l| legacy(&l.param)) {
        return;
    }
    eq_two_curves_to_slots(&mut device.params);
    device.automation.retain(|lane| lane.param != "mode");
    for lane in &mut device.automation {
        if let Some(id) = eq_slot_id(&lane.param) {
            lane.param = id;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn value(params: &[DeviceParam], id: &str) -> Option<f64> {
        params.iter().find(|p| p.id == id).map(|p| p.value)
    }

    #[test]
    fn curve_b_lands_on_bands_9_to_16() {
        assert_eq!(eq_slot_id("b1Frequency").as_deref(), Some("a9Frequency"));
        assert_eq!(eq_slot_id("b8Q").as_deref(), Some("a16Q"));
        assert_eq!(eq_slot_id("a3Gain"), None);
        assert_eq!(eq_slot_id("b9Gain"), None);
        assert_eq!(eq_slot_id("b1InUse"), None);
        assert_eq!(eq_slot_id("balance"), None);
    }

    #[test]
    fn the_mode_becomes_the_placements() {
        for (mode, first, second) in [(1.0, 1.0, 2.0), (2.0, 3.0, 4.0)] {
            let mut params =
                vec![DeviceParam::new("mode", mode), DeviceParam::new("a3Gain", 6.0), DeviceParam::new("b4Gain", -3.0)];
            eq_two_curves_to_slots(&mut params);
            assert_eq!(value(&params, "mode"), None);
            assert_eq!(value(&params, "a3Gain"), Some(6.0));
            assert_eq!(value(&params, "a12Gain"), Some(-3.0));
            for band in 1..=8 {
                assert_eq!(value(&params, &format!("a{band}Place")), Some(first));
                assert_eq!(value(&params, &format!("a{}Place", band + 8)), Some(second));
                assert_eq!(value(&params, &format!("a{}InUse", band + 8)), Some(1.0));
            }
        }
        // Stereo used curve A only: curve B's values move but its bands stay unused.
        let mut params = vec![DeviceParam::new("mode", 0.0), DeviceParam::new("b1On", 1.0)];
        eq_two_curves_to_slots(&mut params);
        assert_eq!(params, vec![DeviceParam::new("a9On", 1.0)]);
    }

    #[test]
    fn a_devices_automation_moves_with_its_values() {
        let mut device = Device::new("EQ", DeviceKind::Builtin("eq".into()));
        device.params = vec![DeviceParam::new("mode", 2.0)];
        let lane =
            |param: &str| crate::AutomationLane { param: param.into(), points: Default::default(), enabled: true };
        device.automation = vec![lane("b2Frequency"), lane("mode"), lane("a1Gain")];
        eq_device_to_slots(&mut device);
        let lanes: Vec<_> = device.automation.iter().map(|l| l.param.as_str()).collect();
        assert_eq!(lanes, ["a10Frequency", "a1Gain"]);
        assert_eq!(value(&device.params, "a9InUse"), Some(1.0));
        // Another device's ids are not touched.
        let mut other = Device::new("Utility", DeviceKind::Builtin("utility".into()));
        other.params = vec![DeviceParam::new("mode", 1.0)];
        eq_device_to_slots(&mut other);
        assert_eq!(other.params, vec![DeviceParam::new("mode", 1.0)]);
    }
}
