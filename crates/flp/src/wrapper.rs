// SPDX-License-Identifier: GPL-3.0-or-later
// Copyright (C) 2026 ItsAlphaNeon
// Copyright (C) 2026 The Suhina contributors

//! Adapted from Lemon Studio os-flp/src/wrapper.rs, os-vst2/src/state.rs and
//! os-vst3/src/{fl_state,lib}.rs at 011a03f23d9db7f10f07cd7d165cc978a9dda263,
//! https://github.com/ItsAlphaNeon/Lemon-Studio. Changes: bounded borrowed records,
//! canonical class ids, conversion to existing host state, preserved source bytes.

use suhina_model::{Device, DeviceKind, Extension};
use suhina_plugins::vst2::{self, Preset};

fn word(bytes: &[u8], at: usize) -> Option<u32> {
    Some(u32::from_le_bytes(bytes.get(at..at.checked_add(4)?)?.try_into().ok()?))
}

struct Records<'a>(Vec<(u32, &'a [u8])>);
impl<'a> Records<'a> {
    fn parse(bytes: &'a [u8]) -> Option<Self> {
        word(bytes, 0)?;
        let mut at = 4usize;
        let mut records = Vec::new();
        while at < bytes.len() {
            if records.len() >= 4096 {
                return None;
            }
            let id = word(bytes, at)?;
            let start = at.checked_add(12)?;
            let size =
                usize::try_from(u64::from_le_bytes(bytes.get(at.checked_add(4)?..start)?.try_into().ok()?)).ok()?;
            let end = start.checked_add(size)?;
            // Conflicting identities or states must not depend on record order.
            if records.iter().any(|(key, _)| *key == id) {
                return None;
            }
            records.push((id, bytes.get(start..end)?));
            at = end;
        }
        Some(Self(records))
    }
    fn get(&self, id: u32) -> Option<&'a [u8]> {
        self.0.iter().find(|(key, _)| *key == id).map(|(_, bytes)| *bytes)
    }
    fn text(&self, id: u32) -> String {
        let bytes = self.get(id).unwrap_or_default();
        String::from_utf8_lossy(bytes.split(|b| *b == 0).next().unwrap_or_default()).into_owned()
    }
}

/// Convert a wrapper only if both identity and saved state can reach the host intact.
pub(crate) fn decode(source: &Device) -> Option<Device> {
    if !matches!(&source.kind, DeviceKind::Foreign { format, id } if format == "flp" && id == "Fruity Wrapper") {
        return None;
    }
    let original = source.state.as_deref()?;
    let records = Records::parse(original)?;
    let path = records.text(55).to_ascii_lowercase();
    if path.ends_with(".clap") {
        return None;
    }
    let is_vst3 = path.ends_with(".vst3") || (records.get(51).is_none() && records.get(52).is_some());
    let bytes = records.get(53)?;
    let mut device = source.clone();
    let name = records.text(54);
    if !name.is_empty() {
        device.name = name;
    }
    if is_vst3 {
        let guid: &[u8; 16] = records.get(52)?.try_into().ok()?;
        let id = [3, 2, 1, 0, 5, 4, 7, 6, 8, 9, 10, 11, 12, 13, 14, 15]
            .iter()
            .map(|&i| format!("{:02X}", guid[i]))
            .collect();
        if word(bytes, 0)? != 1 {
            return None;
        }
        let state = Records::parse(bytes)?;
        for (id, body) in &state.0 {
            match id {
                1..=3 => {}
                4 if body.len() == 4usize.checked_add(usize::try_from(word(body, 0)?).ok()?.checked_mul(4)?)? => {}
                _ => return None,
            }
        }
        device.kind = DeviceKind::Vst3 { id };
        device.state = Some(state.get(3)?.to_vec().into());
        if let Some(controller) = state.get(2) {
            device.extensions.0.push(Extension {
                format: "flp".into(),
                key: "Vst3ControllerState".into(),
                data: controller.to_vec(),
            });
        }
    } else {
        let id = u32::from_le_bytes(records.get(51)?.try_into().ok()?);
        device.kind = DeviceKind::Vst2 { id };
        device.state = Some(vst2_state(id, bytes)?.into());
    }
    device.extensions.0.push(Extension { format: "flp".into(), key: "wrapper".into(), data: original.to_vec() });
    Some(device)
}

/// The source host's parameter index is not necessarily the hosted VST3 parameter id.
pub(crate) fn parameter_id(device: &Device, index: u16) -> Option<String> {
    match device.kind {
        DeviceKind::Vst2 { .. } => Some(index.to_string()),
        DeviceKind::Vst3 { .. } => {
            let original = device.extensions.0.iter().find(|e| e.format == "flp" && e.key == "wrapper")?;
            let outer = Records::parse(&original.data)?;
            let state = Records::parse(outer.get(53)?)?;
            let ids = state.get(4)?;
            if u32::from(index) >= word(ids, 0)? {
                return None;
            }
            Some(word(ids, 4 + usize::from(index) * 4)?.to_string())
        }
        _ => None,
    }
}

fn vst2_state(id: u32, bytes: &[u8]) -> Option<Vec<u8>> {
    if word(bytes, 0)? as i32 != -9 || word(bytes, 5)? as i32 != -2 {
        return None;
    }
    let mut preset =
        Preset { kind: vst2::CHUNK_BANK, id, version: 0, programs: 1, current: 0, parameters: 0, little_endian: true };
    match bytes.get(4)? {
        12 | 13 | 15 => {
            let size = usize::try_from(word(bytes, 9)?).ok()?;
            let chunk = bytes.get(21..21usize.checked_add(size)?)?;
            vst2::bank(&preset, chunk).ok()
        }
        4 | 5 => {
            let count = usize::try_from(word(bytes, 21)?).ok()?;
            if count > 65536 || word(bytes, 9)? != 0 {
                return None;
            }
            let values = bytes.get(25..25usize.checked_add(count.checked_mul(4)?)?)?;
            for value in values.as_chunks::<4>().0 {
                if !(0.0..=1.0).contains(&f32::from_le_bytes(*value)) {
                    return None;
                }
            }
            preset.kind = vst2::PROGRAM_BANK;
            preset.parameters = count;
            let mut program = vec![0; 28];
            program.extend(values);
            vst2::bank(&preset, &program).ok()
        }
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn chunk_variants_preserve_payload_and_reject_truncated_lengths() {
        for kind in [12, 13, 15] {
            let mut bytes = (-9i32).to_le_bytes().to_vec();
            bytes.push(kind);
            bytes.extend((-2i32).to_le_bytes());
            bytes.extend(4u32.to_le_bytes());
            bytes.extend([0; 8]);
            bytes.extend([1, 2, 3, 4]);
            let bank = vst2_state(123, &bytes).unwrap();
            assert_eq!(vst2::chunk(&bank), Some([1, 2, 3, 4].as_slice()));
            bytes.pop();
            assert!(vst2_state(123, &bytes).is_none());
            bytes[9..13].copy_from_slice(&u32::MAX.to_le_bytes());
            assert!(vst2_state(123, &bytes).is_none());
        }
    }

    #[test]
    fn parameter_states_become_host_program_banks_and_reject_hostile_values() {
        for kind in [4, 5] {
            let mut bytes = (-9i32).to_le_bytes().to_vec();
            bytes.push(kind);
            bytes.extend((-2i32).to_le_bytes());
            bytes.extend([0; 12]);
            bytes.extend(2u32.to_le_bytes());
            bytes.extend(0.25f32.to_le_bytes());
            bytes.extend(0.75f32.to_le_bytes());
            bytes.extend(b"uninterpreted source program metadata");
            let bank = vst2_state(123, &bytes).unwrap();
            assert_eq!(vst2::program(&bank), Some((0, vec![0.25, 0.75])));
            for invalid in [f32::NAN, f32::INFINITY, -1., 1.1] {
                bytes[25..29].copy_from_slice(&invalid.to_le_bytes());
                assert!(vst2_state(123, &bytes).is_none());
            }
            bytes[21..25].copy_from_slice(&u32::MAX.to_le_bytes());
            assert!(vst2_state(123, &bytes).is_none());
        }
    }

    #[test]
    fn malformed_and_duplicate_records_are_rejected_without_allocation_from_lengths() {
        let mut bytes = 10u32.to_le_bytes().to_vec();
        bytes.extend(51u32.to_le_bytes());
        bytes.extend(u64::MAX.to_le_bytes());
        assert!(Records::parse(&bytes).is_none());
        bytes[8..16].copy_from_slice(&4u64.to_le_bytes());
        bytes.extend(123u32.to_le_bytes());
        for cut in 5..bytes.len() {
            assert!(Records::parse(&bytes[..cut]).is_none());
        }
        let duplicate = bytes[4..].to_vec();
        bytes.extend(duplicate);
        assert!(Records::parse(&bytes).is_none());
    }
}
