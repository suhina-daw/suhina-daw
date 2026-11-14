use thiserror::Error;

/// Errors produced when loading or building a NAM model.
#[derive(Debug, Error)]
pub enum Error {
    /// The `.nam` JSON could not be parsed.
    #[error("failed to parse .nam JSON: {0}")]
    Json(#[from] serde_json::Error),

    /// The file was read but its contents are not a valid/supported model.
    #[error("failed to read .nam file: {0}")]
    Io(#[from] std::io::Error),

    /// The model's `architecture` field is not one this crate can run.
    #[error("unsupported model architecture: {0:?}")]
    UnsupportedArchitecture(String),

    /// A layer's `activation` field names a function this crate does not implement.
    #[error("unsupported activation function: {0:?}")]
    UnsupportedActivation(String),

    /// A `.nam` config uses a feature this crate does not yet implement (e.g.
    /// multi-channel input, a post-stack head with more than one output channel,
    /// mixed gating modes within one layer array, or an empty container).
    /// Rejected explicitly rather than silently mis-run — see the crate-level
    /// docs for the full list of supported and rejected features.
    #[error("unsupported model feature: {0}")]
    UnsupportedFeature(String),

    /// The flat `weights` array did not contain the number of values the
    /// `config` implies (corrupt file, or a config/weights mismatch).
    #[error("weight count mismatch: config implies {expected} weights, file has {found}")]
    WeightCountMismatch {
        /// Number of weights the `config` implies the file must contain.
        expected: usize,
        /// Number of weights actually present in the file's flat `weights` array.
        found: usize,
    },

    /// The `config`'s declared dimensions are so large that the implied weight
    /// count overflows `usize`, so the model cannot be built. Indicates a corrupt
    /// or adversarial file rather than a real capture.
    #[error("model config dimensions are too large to be valid")]
    ConfigTooLarge,
}
