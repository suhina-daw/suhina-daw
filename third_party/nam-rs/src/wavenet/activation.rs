//! Pointwise activation functions used by WaveNet layers and the gating module.
//!
//! Ported from NeuralAmpModelerCore `NAM/activations.{h,cpp}`. Shared by
//! `layer.rs` (the post-conv activation) and `gating.rs` (primary/secondary).

use crate::error::Error;

/// Pointwise activation applied after the dilated conv + mix-in.
#[derive(Debug, Clone, Copy, PartialEq)]
pub(super) enum Activation {
    Tanh,
    Relu,
    Sigmoid,
    /// LeakyReLU with the given negative slope (`x > 0 ? x : slope*x`).
    LeakyRelu(f32),
}

impl Activation {
    pub(super) fn from_spec(spec: &crate::model::ActivationSpec) -> Result<Self, Error> {
        use crate::model::ActivationSpec;
        match spec {
            ActivationSpec::Named {
                name,
                negative_slope,
            } => match name.as_str() {
                "Tanh" => Ok(Self::Tanh),
                "ReLU" => Ok(Self::Relu),
                "Sigmoid" => Ok(Self::Sigmoid),
                "LeakyReLU" => Ok(Self::LeakyRelu(negative_slope.unwrap_or(0.01))),
                other => Err(Error::UnsupportedActivation(other.to_string())),
            },
            ActivationSpec::Unsupported(v) => {
                Err(Error::UnsupportedFeature(format!("activation: {v}")))
            }
        }
    }

    #[inline]
    pub(super) fn apply(self, x: f32) -> f32 {
        match self {
            Self::Tanh => crate::fast_math::fast_tanh(x),
            Self::Relu => x.max(0.0),
            Self::Sigmoid => sigmoid(x),
            Self::LeakyRelu(slope) => {
                if x > 0.0 {
                    x
                } else {
                    slope * x
                }
            }
        }
    }
}

#[inline]
fn sigmoid(x: f32) -> f32 {
    1.0 / (1.0 + (-x).exp())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn from_spec_resolves_named_activations() {
        use crate::model::ActivationSpec;
        let named = |n: &str| ActivationSpec::Named {
            name: n.into(),
            negative_slope: None,
        };
        assert_eq!(
            Activation::from_spec(&named("Tanh")).unwrap(),
            Activation::Tanh
        );
        assert_eq!(
            Activation::from_spec(&named("ReLU")).unwrap(),
            Activation::Relu
        );
        assert_eq!(
            Activation::from_spec(&named("Sigmoid")).unwrap(),
            Activation::Sigmoid
        );
        assert_eq!(
            Activation::from_spec(&named("LeakyReLU")).unwrap(),
            Activation::LeakyRelu(0.01)
        );
        assert_eq!(
            Activation::from_spec(&ActivationSpec::Named {
                name: "LeakyReLU".into(),
                negative_slope: Some(0.2)
            })
            .unwrap(),
            Activation::LeakyRelu(0.2)
        );
    }

    #[test]
    fn from_spec_rejects_unknown_and_unsupported() {
        use crate::model::ActivationSpec;
        let bad_name = ActivationSpec::Named {
            name: "Softsign".into(),
            negative_slope: None,
        };
        assert!(matches!(
            Activation::from_spec(&bad_name),
            Err(crate::Error::UnsupportedActivation(_))
        ));
        let list = ActivationSpec::Unsupported(serde_json::json!(["ReLU", "Tanh"]));
        assert!(matches!(
            Activation::from_spec(&list),
            Err(crate::Error::UnsupportedFeature(_))
        ));
    }

    #[test]
    fn leaky_relu_applies_slope() {
        let a = Activation::LeakyRelu(0.01);
        assert_eq!(a.apply(2.0), 2.0);
        assert!((a.apply(-2.0) - (-0.02)).abs() < 1e-9);
        assert_eq!(a.apply(0.0), 0.0);
    }

    #[test]
    fn sigmoid_matches_reference() {
        // sigmoid(0) = 0.5 exactly; pin the formula gating relies on.
        assert_eq!(Activation::Sigmoid.apply(0.0), 0.5);
        let want = 1.0_f32 / (1.0 + (-1.5_f32).exp());
        assert!((Activation::Sigmoid.apply(1.5) - want).abs() < 1e-9);
    }
}
