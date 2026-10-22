// Copyright (C) 2026 The Suhina contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! Suhina's style language: bounded text in, values out. No GUI or file I/O.
pub mod cascade;
pub mod edit;
pub mod gradient;
pub mod layout;
pub mod media;
pub mod paint;
pub mod property;
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
