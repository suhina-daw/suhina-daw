// Copyright (C) 2026 The Suhina contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! How a VST3 state carries an ARA document's archive beside the component's own state. Kept
//! apart from [`crate::ara`] so a host built without ARA (the 32-bit Windows host) reads and
//! writes the same states: a state without an archive is the component's alone, unchanged.

/// What starts a VST3 state that carries the plug-in's ARA edits as well.
const STATE_MAGIC: &[u8; 16] = b"SUHINA-ARA-STATE";

/// The state a project keeps for a plug-in bound to an ARA document: the component's own
/// state and the document archive (its id and bytes), in one blob. Without an archive it is the
/// component's state as it is.
pub(crate) fn wrap_state(component: Vec<u8>, archive: Option<(String, Vec<u8>)>) -> Vec<u8> {
    let Some((id, bytes)) = archive else { return component };
    let mut out = Vec::with_capacity(component.len() + bytes.len() + id.len() + 40);
    out.extend_from_slice(STATE_MAGIC);
    out.extend_from_slice(&1u32.to_le_bytes());
    for part in [component.as_slice(), id.as_bytes(), bytes.as_slice()] {
        out.extend_from_slice(&(part.len() as u64).to_le_bytes());
        out.extend_from_slice(part);
    }
    out
}

/// Splits a state [`wrap_state`] made into the component's state and the archive; any other
/// state is the component's alone. A blob that starts like one but does not hold together is
/// taken as the component's, untouched.
pub(crate) fn unwrap_state(data: &[u8]) -> (&[u8], Option<(String, &[u8])>) {
    type Parts<'a> = (&'a [u8], Option<(String, &'a [u8])>);
    let parts = || -> Option<Parts<'_>> {
        let rest = data.strip_prefix(STATE_MAGIC.as_slice())?;
        let (version, mut rest) = rest.split_at_checked(4)?;
        if version != 1u32.to_le_bytes() {
            return None;
        }
        let mut take = || -> Option<&[u8]> {
            let (len, after) = rest.split_at_checked(8)?;
            let len = usize::try_from(u64::from_le_bytes(len.try_into().ok()?)).ok()?;
            let (part, after) = after.split_at_checked(len)?;
            rest = after;
            Some(part)
        };
        let (component, id, bytes) = (take()?, take()?, take()?);
        let id = String::from_utf8(id.to_vec()).ok()?;
        Some((component, Some((id, bytes))))
    };
    parts().unwrap_or((data, None))
}
