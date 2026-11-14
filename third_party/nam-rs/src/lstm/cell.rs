//! One LSTM layer: gates from `W·[x; h] + b`, then the standard cell update.
//!
//! `W` is the combined `(4H) × (input_size + H)` matrix in NAM `export_weights`
//! order, row-major, gate order **i, f, g, o** (PyTorch). The exporter pre-sums
//! `bias_ih + bias_hh`, so there is a single bias vector of length `4H`.

use crate::fast_math::{fast_sigmoid, fast_tanh};

#[derive(Debug)]
pub(crate) struct LstmCell {
    hidden_size: usize,
    input_size: usize,
    /// (4H) × (input_size + H), row-major.
    w: Vec<f32>,
    /// 4H.
    b: Vec<f32>,
    /// Exported initial hidden/cell (length H each); `reset()` restores to these.
    h0: Vec<f32>,
    c0: Vec<f32>,
    // --- mutable state + scratch (pre-allocated) ---
    h: Vec<f32>,     // H
    c: Vec<f32>,     // H
    xh: Vec<f32>,    // input_size + H
    gates: Vec<f32>, // 4H
}

impl LstmCell {
    pub(crate) fn new(
        input_size: usize,
        hidden_size: usize,
        w: Vec<f32>,
        b: Vec<f32>,
        h0: Vec<f32>,
        c0: Vec<f32>,
    ) -> Self {
        debug_assert_eq!(w.len(), 4 * hidden_size * (input_size + hidden_size));
        debug_assert_eq!(b.len(), 4 * hidden_size);
        debug_assert_eq!(h0.len(), hidden_size);
        debug_assert_eq!(c0.len(), hidden_size);
        Self {
            hidden_size,
            input_size,
            w,
            b,
            h: h0.clone(),
            c: c0.clone(),
            h0,
            c0,
            xh: vec![0.0; input_size + hidden_size],
            gates: vec![0.0; 4 * hidden_size],
        }
    }

    /// Advance one timestep with input `x` (length `input_size`); returns the new
    /// hidden state `h` (length `hidden_size`). Allocation-free.
    pub(crate) fn process(&mut self, x: &[f32]) -> &[f32] {
        let h = self.hidden_size;
        let row = self.input_size + h;

        // xh = [x ; h_prev]
        self.xh[..self.input_size].copy_from_slice(x);
        self.xh[self.input_size..].copy_from_slice(&self.h);

        // gates = W · xh + b
        for g in 0..4 * h {
            let base = g * row;
            let mut acc = self.b[g];
            for k in 0..row {
                acc += self.w[base + k] * self.xh[k];
            }
            self.gates[g] = acc;
        }

        // cell update, gate order i,f,g,o
        for j in 0..h {
            let i = fast_sigmoid(self.gates[j]);
            let f = fast_sigmoid(self.gates[h + j]);
            let g_ = fast_tanh(self.gates[2 * h + j]);
            let o = fast_sigmoid(self.gates[3 * h + j]);
            let c = f * self.c[j] + i * g_;
            self.c[j] = c;
            self.h[j] = o * fast_tanh(c);
        }
        &self.h
    }

    /// Restore `h`/`c` to the exported initial state.
    pub(crate) fn reset(&mut self) {
        self.h.copy_from_slice(&self.h0);
        self.c.copy_from_slice(&self.c0);
    }

    pub(crate) fn hidden_size(&self) -> usize {
        self.hidden_size
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn single_cell_matches_hand_computed_step() {
        // H=1, input_size=1. Row-major W (4H x (in+H) = 4x2), gate order i,f,g,o.
        // W rows: i:[1,0] f:[0,0] g:[2,0] o:[0,0]; bias=[0,0,0,0]; h0=c0=0.
        let w = vec![
            1.0, 0.0, // i: w_xi=1, w_hi=0
            0.0, 0.0, // f
            2.0, 0.0, // g: w_xg=2
            0.0, 0.0, // o
        ];
        let b = vec![0.0, 0.0, 0.0, 0.0];
        let mut cell = LstmCell::new(1, 1, w, b, vec![0.0], vec![0.0]);

        // x=0.5: i=sig(0.5)=0.622459, f=sig(0)=0.5, g=tanh(1.0)=0.761594, o=sig(0)=0.5
        // c = f*0 + i*g = 0.622459*0.761594 = 0.474061
        // h = o*tanh(c) = 0.5*tanh(0.474061) = 0.5*0.441510 = 0.220755
        // (hand arithmetic; 1e-3 tol — the 1e-5 gate is the parity test vs canonical NAM)
        let h = cell.process(&[0.5]);
        assert!((h[0] - 0.2208).abs() < 1e-3, "got {}", h[0]);
    }

    #[test]
    fn reset_restores_initial_state() {
        let w = vec![1.0, 0.0, 0.0, 0.0, 2.0, 0.0, 0.0, 0.0];
        let b = vec![0.0; 4];
        let mut cell = LstmCell::new(1, 1, w, b, vec![0.0], vec![0.0]);
        let first = cell.process(&[0.5])[0];
        cell.process(&[0.5]); // advance state
        cell.reset();
        let after = cell.process(&[0.5])[0];
        assert!((first - after).abs() < 1e-7, "{first} vs {after}");
    }
}
