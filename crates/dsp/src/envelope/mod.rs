// Copyright (C) 2026 The Suhina contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! Distinct synthesis envelopes: per-sample linear ADSR and block-stepped time envelopes.
//! Lifecycle and stepping methods keep bounded state inline and never allocate, lock or do I/O.

mod linear;
mod time;

pub use linear::{LinearEnvelope, LinearEnvelopeSettings};
pub use time::{EnvelopeClock, LoopMode, SILENCE_DB, TimeEnvelope, TimeEnvelopeSettings};
