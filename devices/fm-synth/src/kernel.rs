// Copyright (C) 2026 The Suhina contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! The sample-accurate operator kernel: the same algorithms as the ported one in `core.rs`, with
//! waveforms, a frequency that moves every sample and feedback per operator. The ported kernel runs
//! whenever none of those is in use, so old sounds stay exact (ADR 0034).

use crate::core::{self, Buses, FB_IN, FB_OUT, LEVEL_THRESHOLD, OUT_BUS_ADD, OpParams};
use crate::ext::{Frame, Mods, NoteExt};
use crate::tables::{LG_N, N, Tables};
use crate::wave::{self, TABLE};

/// What an operator plays this block.
enum Source<'a> {
    Sine,
    Table(&'a [f32; TABLE], f32),
    White,
}

const ONE: f32 = (1u32 << 24) as f32;

/// Renders a block of a note into `output`. `fb_shift` 16 or more turns the voice's feedback off.
#[allow(clippy::too_many_arguments)]
pub fn render(
    output: &mut [i32; N],
    params: &mut [OpParams; 6],
    algorithm: usize,
    fb: &mut [i32; 2],
    fb_shift: i32,
    x: &mut NoteExt,
    buses: &mut Buses,
    t: &Tables,
    frame: &Frame,
    mods: &Mods,
) {
    let ext = frame.ext;
    let interpolate = ext.interpolate;
    let mut has_contents = [true, false, false];
    let lfo = x.lfo;
    for (op, param) in params.iter_mut().enumerate() {
        let flags = core::flags(algorithm, op);
        let mut add = flags & OUT_BUS_ADD != 0;
        let in_bus = usize::from((flags >> 4) & 3);
        let out_bus = usize::from(flags & 3);
        let gain1 = param.gain_out;
        let gain2 = t.exp2(param.level_in.wrapping_sub(14 << 24));
        param.gain_out = gain2;
        let settings = ext.ops.get(op);
        let (freq, depth) = (param.freq, mods.lfo_depth.get(op).copied().unwrap_or(0.0));
        let mut phase = param.phase;
        let step = core::gain_step(gain1, gain2);

        if gain1 >= LEVEL_THRESHOLD || gain2 >= LEVEL_THRESHOLD {
            if !has_contents.get(out_bus).copied().unwrap_or(false) {
                add = false;
            }
            let modulated = in_bus != 0 && has_contents.get(in_bus).copied().unwrap_or(false);
            let wave_index = settings.map_or(wave::SINE, |o| o.wave);
            let source = if wave_index == wave::SINE {
                Source::Sine
            } else if wave_index == wave::NOISE_WHITE {
                Source::White
            } else if wave_index == wave::DRAWN {
                match frame.drawn.get(op) {
                    Some(drawn) => {
                        let (table, gain) = drawn.table(freq, ext.antialias);
                        Source::Table(table, gain)
                    }
                    None => Source::Sine,
                }
            } else {
                match wave::builtin(wave_index, freq, ext.antialias) {
                    Some(table) => Source::Table(table, 1.0),
                    None => Source::Sine,
                }
            };
            // The voice's feedback, on the algorithm's loop.
            let voice_loop = !modulated && flags & (FB_IN | FB_OUT) == FB_IN | FB_OUT && fb_shift < 16;
            let own = (settings.map_or(0.0, |o| o.feedback) + mods.feedback).clamp(0.0, 1.0);
            let own_loop = !modulated && !voice_loop && own > 0.0;
            let mut state = if voice_loop { *fb } else { x.op_fb.get(op).copied().unwrap_or([0; 2]) };
            let voice_shift = (fb_shift + 1).clamp(0, 31);

            // Read a copy of the input, as the output may be the same bus.
            let input: Option<[i32; N]> = if modulated {
                Some(match in_bus {
                    1 => buses.0[0],
                    2 => buses.0[1],
                    _ => [0; N],
                })
            } else {
                None
            };
            let [bus1, bus2] = &mut buses.0;
            let out = core::pick(out_bus, output, bus1, bus2);
            let mut gain = gain1;
            for (i, slot) in out.iter_mut().enumerate() {
                gain = gain.wrapping_add(step);
                let mut modulation = input.as_ref().map_or(0, |m| m.get(i).copied().unwrap_or(0));
                if voice_loop {
                    modulation = modulation.wrapping_add(state[0].wrapping_add(state[1]) >> voice_shift);
                } else if own_loop {
                    let sum = state[0].wrapping_add(state[1]) as f32;
                    modulation = modulation.wrapping_add((sum * own * 0.25) as i32);
                }
                let at = phase.wrapping_add(modulation);
                let y = match &source {
                    Source::Sine => t.sin(at),
                    Source::Table(table, table_gain) => (wave::read(table, at, interpolate) * *table_gain * ONE) as i32,
                    Source::White => {
                        let mut n = x.noise;
                        n ^= n << 13;
                        n ^= n >> 17;
                        n ^= n << 5;
                        x.noise = n;
                        ((n as f32 / u32::MAX as f32 * 2.0 - 1.0) * ONE) as i32
                    }
                };
                let y = core::scale(y, gain);
                if voice_loop || own_loop {
                    state = [state[1], y];
                }
                core::write(slot, y, add);
                let wobble = lfo.get(i).copied().unwrap_or(0.0) * depth;
                let moved = if wobble == 0.0 { freq } else { freq.wrapping_add((freq as f32 * wobble) as i32) };
                phase = phase.wrapping_add(moved);
            }
            if voice_loop {
                *fb = state;
            } else if own_loop && let Some(slot) = x.op_fb.get_mut(op) {
                *slot = state;
            }
            if let Some(slot) = has_contents.get_mut(out_bus) {
                *slot = true;
            }
            param.phase = phase;
        } else {
            if !add && let Some(slot) = has_contents.get_mut(out_bus) {
                *slot = false;
            }
            param.phase = param.phase.wrapping_add(param.freq.wrapping_shl(LG_N));
        }
    }
}
