//! Real-time LSTM inference (NAM recurrent architecture).
//!
//! Mirrors the WaveNet runtime's contract: built once from a parsed [`NamModel`]
//! (allocating), then run on the audio thread with zero allocation. State (`h`,`c`)
//! is initialised from the model's **exported** initial hidden/cell vectors — the
//! core burned in over silence — not zeros, matching NAM Core / NeuralAudio.

mod cell;

use cell::LstmCell;

use crate::error::Error;
use crate::model::{LstmConfig, ModelConfig, NamModel};
use crate::reader::Reader;

/// A ready-to-run LSTM, all scratch pre-allocated in [`Lstm::new`].
#[derive(Debug)]
pub struct Lstm {
    cells: Vec<LstmCell>,
    /// Head: `Linear(H, 1)` — weight length `H`, scalar bias.
    head_w: Vec<f32>,
    head_b: f32,
    /// Carry buffers for the inter-layer hidden signal (ping-ponged), width `H`.
    buf_a: Vec<f32>,
    buf_b: Vec<f32>,
}

impl Lstm {
    /// Build a runnable LSTM from a parsed `.nam`. All allocation happens here.
    pub fn new(model: &NamModel) -> Result<Self, Error> {
        let cfg = match &model.config {
            ModelConfig::Lstm(cfg) => cfg,
            ModelConfig::WaveNet(_) | ModelConfig::Slimmable(_) => {
                return Err(Error::UnsupportedArchitecture(model.architecture.clone()))
            }
        };

        let expected = expected_weight_count(cfg)?;
        if expected != model.weights.len() {
            return Err(Error::WeightCountMismatch {
                expected,
                found: model.weights.len(),
            });
        }

        let h = cfg.hidden_size;
        let mut r = Reader::new(&model.weights);
        let mut cells = Vec::with_capacity(cfg.num_layers);
        for layer in 0..cfg.num_layers {
            let in_dim = if layer == 0 { cfg.input_size } else { h };
            let w = r.take(4 * h * (in_dim + h));
            let b = r.take(4 * h);
            let h0 = r.take(h);
            let c0 = r.take(h);
            cells.push(LstmCell::new(in_dim, h, w, b, h0, c0));
        }
        let head_w = r.take(h);
        let head_b = r.take(1)[0];
        // Up-front check guarantees `expected == weights.len()`; assert the other half
        // of the invariant — that building consumed exactly `expected`, so the count
        // formula and the consumption order have not drifted apart.
        debug_assert_eq!(
            r.remaining(),
            0,
            "Lstm::new consumed fewer weights than expected_weight_count claimed"
        );

        Ok(Self {
            cells,
            head_w,
            head_b,
            buf_a: vec![0.0; h.max(1)],
            buf_b: vec![0.0; h.max(1)],
        })
    }

    /// Process a buffer of mono samples in place. Allocation-free.
    pub fn process_buffer(&mut self, io: &mut [f32]) {
        for s in io.iter_mut() {
            *s = self.process_sample(*s);
        }
    }

    /// Process one mono sample. Allocation-free.
    pub fn process_sample(&mut self, x: f32) -> f32 {
        if self.cells.is_empty() {
            return self.head_b;
        }
        let h = self.cells[0].hidden_size();
        let x0 = [x];

        let out = self.cells[0].process(&x0);
        self.buf_a[..h].copy_from_slice(out);

        for i in 1..self.cells.len() {
            let out = self.cells[i].process(&self.buf_a[..h]);
            self.buf_b[..h].copy_from_slice(out);
            std::mem::swap(&mut self.buf_a, &mut self.buf_b);
        }

        // head: dot(head_w, last hidden) + bias
        let mut y = self.head_b;
        for j in 0..h {
            y += self.head_w[j] * self.buf_a[j];
        }
        y
    }

    /// Reset all recurrent state to the exported initial hidden/cell vectors.
    pub fn reset(&mut self) {
        for c in &mut self.cells {
            c.reset();
        }
        self.buf_a.fill(0.0);
        self.buf_b.fill(0.0);
    }
}

/// Number of `f32`s the LSTM `config` implies in the flat weight blob.
///
/// Uses checked arithmetic: an absurd or adversarial config whose dimensions overflow
/// `usize` returns [`Error::ConfigTooLarge`] rather than panicking (debug) or wrapping
/// to a wrong, small count (release).
fn expected_weight_count(cfg: &LstmConfig) -> Result<usize, Error> {
    let mul = |a: usize, b: usize| a.checked_mul(b).ok_or(Error::ConfigTooLarge);
    let add = |a: usize, b: usize| a.checked_add(b).ok_or(Error::ConfigTooLarge);

    let h = cfg.hidden_size;
    let mut total = 0usize;
    for layer in 0..cfg.num_layers {
        let in_dim = if layer == 0 { cfg.input_size } else { h };
        total = add(total, mul(mul(4, h)?, add(in_dim, h)?)?)?; // combined W
        total = add(total, mul(4, h)?)?; // bias
        total = add(total, h)?; // h0
        total = add(total, h)?; // c0
    }
    add(add(total, h)?, 1) // head weight + bias
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::NamModel;

    // 1 layer, input_size=1, H=1. Blob order: W(4*1*(1+1)=8), b(4), h0(1), c0(1),
    // head_w(1), head_b(1) = 16 weights.
    // W rows i,f,g,o: i:[1,0] f:[0,0] g:[2,0] o:[0,0]; b=0; h0=c0=0;
    // head_w=[3.0], head_b=0.5.
    const TINY_LSTM: &str = r#"{
        "version": "0.5.4", "architecture": "LSTM",
        "config": { "input_size": 1, "hidden_size": 1, "num_layers": 1 },
        "weights": [1.0,0.0, 0.0,0.0, 2.0,0.0, 0.0,0.0, 0.0,0.0,0.0,0.0, 0.0, 0.0, 3.0, 0.5]
    }"#;

    #[test]
    fn tiny_lstm_matches_hand_computed() {
        let model = NamModel::from_json_str(TINY_LSTM).unwrap();
        let mut net = Lstm::new(&model).unwrap();
        // From the cell test: h after x=0.5 is ~0.220755.
        // y = head_w*h + head_b = 3.0*0.220755 + 0.5 = 1.16227
        let mut buf = [0.5_f32];
        net.process_buffer(&mut buf);
        assert!((buf[0] - 1.1623).abs() < 1e-3, "got {}", buf[0]);
    }

    #[test]
    fn weight_count_mismatch_is_rejected() {
        let bad = TINY_LSTM.replace(", 3.0, 0.5]", ", 3.0]");
        let model = NamModel::from_json_str(&bad).unwrap();
        assert!(matches!(
            Lstm::new(&model),
            Err(crate::Error::WeightCountMismatch { .. })
        ));
    }

    /// A structurally valid config whose dimensions overflow `usize` must return
    /// `ConfigTooLarge`, not panic (debug) or wrap to a wrong count (release).
    #[test]
    fn absurd_dimensions_error_instead_of_overflowing() {
        let json = TINY_LSTM.replace("\"hidden_size\": 1", "\"hidden_size\": 4294967296");
        let model = NamModel::from_json_str(&json).unwrap();
        assert!(matches!(
            Lstm::new(&model),
            Err(crate::Error::ConfigTooLarge)
        ));
    }

    /// Pins the weight-count invariant: `expected_weight_count` must equal exactly
    /// what `Lstm::new` consumes, across (input_size, hidden_size, num_layers) shapes.
    #[test]
    fn weight_count_matches_consumption_across_shapes() {
        for (input_size, hidden_size, num_layers) in [(1, 1, 1), (1, 8, 1), (1, 4, 2), (2, 3, 3)] {
            let cfg = LstmConfig {
                input_size,
                hidden_size,
                num_layers,
            };
            let n = expected_weight_count(&cfg).unwrap();
            let mk_model = |count: usize| NamModel {
                version: "0".into(),
                architecture: "LSTM".into(),
                config: ModelConfig::Lstm(cfg.clone()),
                weights: vec![0.0; count],
                sample_rate: None,
                metadata: None,
            };
            assert!(Lstm::new(&mk_model(n)).is_ok(), "exact count n={n}");
            assert!(matches!(
                Lstm::new(&mk_model(n - 1)),
                Err(crate::Error::WeightCountMismatch { .. })
            ));
            assert!(matches!(
                Lstm::new(&mk_model(n + 1)),
                Err(crate::Error::WeightCountMismatch { .. })
            ));
        }
    }

    #[test]
    fn reset_restores_fresh_output() {
        let model = NamModel::from_json_str(TINY_LSTM).unwrap();
        let mut net = Lstm::new(&model).unwrap();
        let mut warm = [0.3_f32, -0.7, 0.2];
        net.process_buffer(&mut warm);
        net.reset();
        let mut a = [0.5_f32];
        net.process_buffer(&mut a);
        assert!((a[0] - 1.1623).abs() < 1e-3, "got {}", a[0]);
    }
}
