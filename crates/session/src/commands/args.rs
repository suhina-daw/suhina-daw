// Copyright (C) 2026 The Suhina contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! [`Args`]: a command's parameters read by name, each as the type it is, in place of reading
//! `&Value` by hand (#764 › 1). Dispatch has checked already that every parameter has its declared
//! JSON type and that the required ones are there ([`super::check_params`]), so what is left here
//! is what a type alone does not say: a number that is finite, a name that is not blank. Commands
//! move over to it one namespace at a time; `marker.*` and `scene.setTempo` are the first.

use serde_json::Value;

use crate::CommandError;

/// The parameters one command was given.
#[derive(Debug, Clone, Copy)]
pub(crate) struct Args<'a>(&'a Value);

impl<'a> Args<'a> {
    pub(crate) fn new(params: &'a Value) -> Self {
        Self(params)
    }

    fn get(self, name: &str) -> Option<&'a Value> {
        self.0.get(name).filter(|v| !v.is_null())
    }

    /// A finite number.
    pub(crate) fn f64(self, name: &str) -> Result<f64, CommandError> {
        self.opt_f64(name)?.ok_or_else(|| format!("{name} must be a number").into())
    }

    /// A finite number, or `None` when it is left out.
    pub(crate) fn opt_f64(self, name: &str) -> Result<Option<f64>, CommandError> {
        match self.get(name) {
            None => Ok(None),
            Some(v) => {
                v.as_f64().filter(|n| n.is_finite()).map(Some).ok_or_else(|| format!("{name} must be a number").into())
            }
        }
    }

    /// A whole number from 0, as an index; `what` says what it counts in the error.
    pub(crate) fn index(self, name: &str, what: &str) -> Result<usize, CommandError> {
        self.get(name)
            .and_then(Value::as_u64)
            .and_then(|i| usize::try_from(i).ok())
            .ok_or_else(|| format!("{name} must be a whole number {what}").into())
    }

    /// A text with something in it, trimmed.
    pub(crate) fn name(self, name: &str) -> Result<String, CommandError> {
        self.opt_name(name)?.ok_or_else(|| format!("{name} must be a text, not empty").into())
    }

    /// Like [`Args::name`], or `None` when it is left out.
    pub(crate) fn opt_name(self, name: &str) -> Result<Option<String>, CommandError> {
        match self.get(name) {
            None => Ok(None),
            Some(v) => v
                .as_str()
                .map(str::trim)
                .filter(|n| !n.is_empty())
                .map(|n| Some(n.to_string()))
                .ok_or_else(|| format!("{name} must be a text, not empty").into()),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn values_are_read_as_their_type_and_left_out_ones_are_none() {
        let params = json!({ "beat": 4.5, "name": "  Drop ", "index": 2, "gone": null, "bad": "x" });
        let args = Args::new(&params);
        assert_eq!(args.f64("beat"), Ok(4.5));
        assert_eq!(args.opt_f64("gone"), Ok(None));
        assert!(args.f64("bad").is_err());
        assert_eq!(args.name("name"), Ok("Drop".into()));
        assert!(args.name("gone").is_err());
        assert_eq!(args.index("index", "from 0"), Ok(2));
    }
}
