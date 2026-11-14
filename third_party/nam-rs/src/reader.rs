//! Sequential reader over a `.nam` flat weight blob, consumed in NAM's
//! `export_weights` order. Callers validate the total count up front, so `take`
//! never over-runs.

pub(crate) struct Reader<'a> {
    w: &'a [f32],
    i: usize,
}

impl<'a> Reader<'a> {
    pub(crate) fn new(w: &'a [f32]) -> Self {
        Self { w, i: 0 }
    }

    /// Take the next `n` weights as an owned `Vec`.
    ///
    /// Callers validate the total count up front (and the dimensions that produce it
    /// are computed with checked arithmetic), so `self.i + n` never exceeds the blob.
    /// [`Self::remaining`] lets the builder assert that invariant after the last take.
    pub(crate) fn take(&mut self, n: usize) -> Vec<f32> {
        let chunk = self.w[self.i..self.i + n].to_vec();
        self.i += n;
        chunk
    }

    /// Weights not yet consumed. Used to assert a build consumed exactly as many
    /// weights as `expected_weight_count` claimed — i.e. that the count formula and
    /// the consumption order have not drifted apart.
    pub(crate) fn remaining(&self) -> usize {
        self.w.len() - self.i
    }
}
