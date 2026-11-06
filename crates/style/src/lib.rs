// Copyright (C) 2026 The Suhina contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! Suhina's style language: bounded text in, values out. No GUI or file I/O.
pub mod anchor;
pub mod artwork;
pub mod cascade;
pub mod edit;
pub mod gradient;
pub mod layout;
pub mod media;
pub mod motion;
pub mod paint;
pub mod property;
pub mod registered;
pub mod resolver;
pub mod selector;
pub mod sheet;
pub mod syntax;
pub mod value;
mod variables;

/// Maximum UTF-8 source size of one style sheet.
pub const MAX_SOURCE_BYTES: usize = 128 * 1024;
/// Maximum nesting of blocks and functions, including values of custom properties.
pub const MAX_NESTING: usize = 64;

/// Resolves `var()` references among `raw` custom properties, as an element does: a property in
/// a cycle or referring to a missing one without a fallback is left out.
pub fn resolve_variables(
    raw: &std::collections::BTreeMap<String, String>,
) -> Result<std::collections::BTreeMap<String, String>, String> {
    variables::resolve(raw)
}
