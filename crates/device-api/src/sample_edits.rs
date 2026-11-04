// Copyright (C) 2026 The Suhina contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! Bounded, immutable source corrections prepared before audio processing.
const LIMIT: usize = 4096;
const HEADER: &[u8] = b"SSE1";

#[derive(Clone, Debug, Default, PartialEq)]
pub struct SampleEdits {
    pins: Vec<f64>,
    suppressed: Vec<(u8, i64)>,
}

impl SampleEdits {
    pub fn new(mut pins: Vec<f64>, mut suppressed: Vec<(u8, i64)>) -> Result<Self, &'static str> {
        if pins.len() > LIMIT
            || suppressed.len() > LIMIT
            || pins.iter().any(|p| !p.is_finite() || *p <= 0. || *p >= 1.)
            || suppressed.iter().any(|(method, _)| *method > 2)
        {
            return Err("invalid or oversized sample corrections");
        }
        pins.sort_unstable_by(f64::total_cmp);
        pins.dedup();
        suppressed.sort_unstable();
        suppressed.dedup();
        Ok(Self { pins, suppressed })
    }
    pub fn pins(&self) -> &[f64] {
        &self.pins
    }
    pub fn suppressed(&self) -> &[(u8, i64)] {
        &self.suppressed
    }
    pub fn excludes(&self, method: u8, identity: i64) -> bool {
        self.suppressed.binary_search(&(method, identity)).is_ok()
    }
    /// Encode off the audio thread. The source correction format has its own version.
    pub fn encode(&self) -> Vec<u8> {
        let mut bytes = HEADER.to_vec();
        bytes.extend_from_slice(&(self.pins.len() as u32).to_le_bytes());
        for point in &self.pins {
            bytes.extend_from_slice(&point.to_le_bytes());
        }
        for (method, identity) in &self.suppressed {
            bytes.push(*method);
            bytes.extend_from_slice(&identity.to_le_bytes());
        }
        bytes
    }
    pub fn decode(bytes: &[u8]) -> Option<Self> {
        if bytes.len() > 8 + LIMIT * 17 || bytes.get(..4)? != HEADER {
            return None;
        }
        let count = u32::from_le_bytes(bytes.get(4..8)?.try_into().ok()?) as usize;
        if count > LIMIT {
            return None;
        }
        let split = 8usize.checked_add(count.checked_mul(8)?)?;
        let pin_bytes = bytes.get(8..split)?;
        let rest = bytes.get(split..)?;
        if !rest.len().is_multiple_of(9) || rest.len() / 9 > LIMIT {
            return None;
        }
        let pins = pin_bytes.as_chunks::<8>().0.iter().map(|b| f64::from_le_bytes(*b)).collect();
        let suppressed = rest
            .as_chunks::<9>()
            .0
            .iter()
            .map(|b| {
                let mut identity = [0; 8];
                identity.copy_from_slice(&b[1..]);
                (b[0], i64::from_le_bytes(identity))
            })
            .collect();
        Self::new(pins, suppressed).ok()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn corrections_round_trip_and_are_canonical() {
        let edits = SampleEdits::new(vec![0.7, 0.2, 0.2], vec![(1, -2), (0, 17), (1, -2)]).unwrap();
        assert_eq!(edits.pins(), &[0.2, 0.7]);
        assert!(edits.excludes(1, -2));
        assert!(!edits.excludes(2, -2));
        assert_eq!(SampleEdits::decode(&edits.encode()), Some(edits));
    }
    #[test]
    fn malformed_and_future_versions_are_rejected() {
        let bytes = SampleEdits::new(vec![0.2], vec![(2, 4)]).unwrap().encode();
        for len in 0..16 {
            assert!(SampleEdits::decode(&bytes[..len]).is_none());
        }
        assert!(SampleEdits::new(vec![f64::NAN], vec![]).is_err());
        assert!(SampleEdits::new(vec![0.], vec![]).is_err());
        assert!(SampleEdits::new(vec![0.5; LIMIT + 1], vec![]).is_err());
        let mut future = bytes.clone();
        future[3] = b'2';
        assert!(SampleEdits::decode(&future).is_none());
        let mut oversized = bytes;
        oversized[4..8].copy_from_slice(&u32::MAX.to_le_bytes());
        assert!(SampleEdits::decode(&oversized).is_none());
    }
}
