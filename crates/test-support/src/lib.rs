// Copyright (C) 2026 The Suhina contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! What tests across the workspace build again and again, written once (#764 › 8): a project
//! ([`ProjectBuilder`]), a WAV file ([`write_wav`]), a test signal and its measures ([`sine`],
//! [`peak`], [`rms`]) and the comparison with a golden file ([`assert_golden_f32`],
//! [`assert_golden_text`]).
//!
//! A dev-dependency only, and it depends on `suhina-model` alone, so any crate's tests can use it
//! without building more of the workspace.

// Test helpers: a failure is a panic, as an assertion in the test itself would be.
#![allow(clippy::panic, clippy::unwrap_used, clippy::expect_used)]

mod golden;
mod project;
mod signal;
mod wav;

pub use golden::{BLESS, assert_golden_f32, assert_golden_text, blessing, golden_f32};
pub use project::{ProjectBuilder, with_ids};
pub use signal::{peak, rms, sine};
pub use wav::{WavFormat, write_wav};
