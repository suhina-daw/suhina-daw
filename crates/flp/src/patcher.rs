// SPDX-License-Identifier: GPL-3.0-or-later
// Copyright (C) 2026 ItsAlphaNeon
// Copyright (C) 2026 The Suhina contributors

//! Chunk/node layouts adapted from Lemon Studio os-flp/src/patcher.rs at
//! 011a03f23d9db7f10f07cd7d165cc978a9dda263, <https://github.com/ItsAlphaNeon/Lemon-Studio>.
//! Changes: bounded borrowed parsing and conversion into existing device groups.
//! MIDI port classes confirmed from the user's authored project; see the mapping spec.
use std::collections::{BTreeMap, BTreeSet};
use suhina_model::{Branch, Device, DeviceGroup, DeviceKind, Extension};

fn word(b: &[u8], at: usize) -> Option<u32> {
    Some(u32::from_le_bytes(b.get(at..at.checked_add(4)?)?.try_into().ok()?))
}
fn chunks(b: &[u8]) -> Option<Vec<(u32, &[u8])>> {
    let mut at = 0usize;
    let mut out = Vec::new();
    while at < b.len() {
        if out.len() >= 4096 {
            return None;
        }
        let id = word(b, at)?;
        let start = at.checked_add(12)?;
        let n = usize::try_from(u64::from_le_bytes(b.get(at + 4..start)?.try_into().ok()?)).ok()?;
        let end = start.checked_add(n)?;
        out.push((id, b.get(start..end)?));
        at = end;
    }
    Some(out)
}
fn text(b: &[u8]) -> String {
    String::from_utf16_lossy(
        &b.as_chunks::<2>()
            .0
            .iter()
            .map(|c| u16::from_le_bytes([c[0], c[1]]))
            .take_while(|v| *v != 0)
            .collect::<Vec<_>>(),
    )
}
struct Node<'a> {
    kind: u32,
    flags: u32,
    plugin: String,
    name: String,
    state: &'a [u8],
}

pub(crate) fn decode(source: &Device, instrument: bool, tempo: f64, budget: usize) -> Option<Device> {
    if !matches!(&source.kind, DeviceKind::Foreign { format, id } if format == "flp" && id == "Patcher") {
        return None;
    }
    let original = source.state.as_deref()?;
    if original.len() > 16 * 1024 * 1024 || word(original, 0)? != 4 {
        return None;
    }
    let top = chunks(original.get(4..)?)?;
    if top.last().is_none_or(|(id, b)| *id != 1001 || !b.is_empty()) {
        return None;
    }
    let mut nodes = Vec::new();
    // port id -> (node index, signal class, direction, bus). None is a named host port.
    let mut ports = BTreeMap::new();
    let mut host = BTreeMap::new();
    let mut links = Vec::new();
    for (id, b) in top {
        if id != 0 || b.is_empty() {
            continue;
        }
        let group = chunks(b)?;
        let get = |id| group.iter().find(|(k, _)| *k == id).map(|(_, b)| *b);
        match group.first()?.0 {
            5000 => {
                if nodes.len() >= 64 {
                    return None;
                }
                let header = get(5000)?;
                let node = nodes.len();
                nodes.push(Node {
                    kind: word(header, 0)?,
                    flags: word(header, 12)?,
                    plugin: text(get(5004).unwrap_or_default()),
                    name: text(get(5010).unwrap_or_default()),
                    state: get(5005).unwrap_or_default(),
                });
                for (_, p) in group.iter().filter(|(id, _)| *id == 5009) {
                    if word(p, 12)? != 0
                        || ports.insert(word(p, 8)?, (Some(node), word(p, 0)?, word(p, 16)?, word(p, 4)?)).is_some()
                    {
                        return None;
                    }
                }
            }
            1400 => {
                let p = get(1400)?;
                if word(p, 4)? != 0 {
                    return None;
                }
                let id = word(p, 0)?;
                let name = text(get(1402).unwrap_or_default());
                if host.insert(name, id).is_some() || ports.insert(id, (None, word(p, 8)?, 0, 0)).is_some() {
                    return None;
                }
            }
            1300 => {
                let p = get(1300)?;
                if links.len() >= 256 || word(p, 4)? != 0 || word(p, 12)? != 0 {
                    return None;
                }
                // Only the enabled link encoding observed in the authored fixtures is accepted.
                if word(p, 20)? == 3 {
                    continue;
                }
                if word(p, 20)? != 2 {
                    return None;
                }
                let gain = f64::from(f32::from_bits(word(p, 16)?));
                if !(0. ..=16.).contains(&gain) {
                    return None;
                }
                links.push((word(p, 8)?, word(p, 0)?, gain));
            }
            _ => {}
        }
    }
    let input = *host.get(if instrument { "Notes" } else { "Audio input" })?;
    let output = *host.get("Audio output")?;
    let mut midi = BTreeSet::new();
    let mut edges: BTreeMap<Option<usize>, Vec<(Option<usize>, f64)>> = BTreeMap::new();
    for (from, to, gain) in links {
        let &(a, ac, af, ab) = ports.get(&from)?;
        let &(b, bc, bf, bb) = ports.get(&to)?;
        if ac == 1 && bc == 1 && instrument && from == input && bf == 3 && bb == 0 && gain == 1. {
            let n = b?;
            if nodes.get(n)?.kind != 3 || !midi.insert(n) {
                return None;
            }
        } else if ac == 0
            && bc == 0
            && ab == 0
            && bb == 0
            && (a.is_some() && af == 2 || !instrument && from == input)
            && (b.is_some() && bf == 3 || to == output)
        {
            if a.is_none() && from != input || b.is_none() && to != output {
                return None;
            }
            let list = edges.entry(a).or_default();
            if list.iter().any(|(target, _)| *target == b) {
                return None;
            }
            list.push((b, gain));
        } else {
            return None;
        }
    }
    let starts: Vec<_> =
        if instrument { midi.iter().map(|n| (Some(*n), 1.)).collect() } else { edges.remove(&None)? };
    if starts.is_empty() || starts.len() > 16 {
        return None;
    }
    let mut used = BTreeSet::new();
    let mut branches = Vec::new();
    for (mut cursor, mut gain) in starts {
        let mut branch = Branch::new(format!("Branch {}", branches.len() + 1));
        while let Some(n) = cursor {
            // A cable before a nonlinear effect cannot be moved to the branch output.
            if gain != 1. {
                return None;
            }
            // No node duplication: shared tails/reconvergence require a richer decomposition.
            if !used.insert(n) {
                return None;
            }
            let node = nodes.get(n)?;
            if !matches!(node.flags, 8 | 12) || (node.kind == 3 && !midi.contains(&n)) {
                return None;
            }
            let mut raw =
                Device::new(&node.name, DeviceKind::Foreign { format: "flp".into(), id: node.plugin.clone() });
            raw.state = Some(node.state.to_vec().into());
            if let Some(mut d) = crate::wrapper::decode(&raw) {
                if node.kind == 3 {
                    d.mark_external_instrument();
                }
                branch.devices.push(d);
            } else if node.kind != 3 {
                branch.devices.extend(crate::effects::substitute(&raw, tempo)?);
            } else {
                return None;
            }
            let Some(outgoing) = edges.get(&Some(n)) else {
                if instrument {
                    branch.mute = true;
                    break;
                }
                return None;
            };
            let [(next, level)] = outgoing.as_slice() else {
                return None;
            };
            gain *= level;
            if !gain.is_finite() || gain > 16. {
                return None;
            }
            cursor = *next;
        }
        branch.volume = gain;
        branches.push(branch);
    }
    if edges.keys().any(|n| n.is_some_and(|n| !used.contains(&n))) {
        return None;
    }
    let mut result = Device::group(&source.name, DeviceGroup::with_branches(branches));
    if suhina_model::walk_devices(std::slice::from_ref(&result)).len() > budget {
        return None;
    }
    result.enabled = source.enabled;
    result.extensions = source.extensions.clone();
    result.extensions.0.push(Extension { format: "flp".into(), key: "patcher".into(), data: original.to_vec() });
    Some(result)
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn chunk_lengths_and_record_counts_are_bounded() {
        let mut bytes = 1u32.to_le_bytes().to_vec();
        bytes.extend(u64::MAX.to_le_bytes());
        assert!(chunks(&bytes).is_none());
        let mut records = Vec::new();
        for _ in 0..4097 {
            records.extend([0; 12]);
        }
        assert!(chunks(&records).is_none());
        assert!(chunks(&records[..4096 * 12]).is_some());
    }
}
