// Copyright (C) 2026 The Suhina contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! Fixed-point FM building blocks, independent of any device or operator routing.
//!
//! Each render function writes exactly [`N`] frames without allocating or performing I/O.
//! Prepare [`Tables`] off the audio thread and share it immutably between voices. Phase and
//! modulation use the low 24 bits for one cycle; frequency is a phase increment per sample.
//! Gain is linear Q24 (`1 << 24` is unity). Gains advance before each sample, using the
//! rounded, wrapping step from [`gain_step`]. Output addition also wraps.
//!
//! The caller owns persistent phase: after each block, advance it by
//! `frequency.wrapping_shl(LG_N)` using wrapping addition. Only [`render_feedback`] updates
//! persistent state itself, through its caller-owned two-sample history.
//!
//! These primitives do not choose an algorithm, silence threshold, envelope or voice policy.
//! Their Apache-2.0 implementation provenance is recorded in each source file and
//! `ATTRIBUTION.md`; see this crate's `LICENSE-APACHE`.

mod operator;
mod tables;

pub use operator::{gain_step, render_feedback, render_modulated, render_sine, scale, write};
pub use tables::{LG_N, N, PORTA_STEPS, Tables};
