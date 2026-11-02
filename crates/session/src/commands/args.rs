// Copyright (C) 2026 The Suhina contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! [`Args`]: a command's parameters read by name, each as the type it is, in place of reading
//! `&Value` by hand (#764 › 1). Dispatch has checked already that every parameter has its declared
//! JSON type and that the required ones are there ([`super::check_params`]), so what is left here
//! is what a type alone does not say: a number that is finite, a name that is not blank. Every
//! command reads its parameters through it, so each rule and its error is written once.

use serde_json::Value;

use crate::CommandError;

/// The parameters one command was given.
#[derive(Debug, Clone, Copy)]
pub(crate) struct Args<'a>(&'a Value);

impl<'a> Args<'a> {
    pub(crate) fn new(params: &'a Value) -> Self {
        Self(params)
    }

    /// The value of `name`, or `None` when it is left out (missing or `null`).
    pub(crate) fn value(self, name: &str) -> Option<&'a Value> {
        self.0.get(name).filter(|v| !v.is_null())
    }

    /// Whether `name` was given (and is not `null`).
    pub(crate) fn has(self, name: &str) -> bool {
        self.value(name).is_some()
    }

    /// A finite number.
    pub(crate) fn f64(self, name: &str) -> Result<f64, CommandError> {
        self.opt_f64(name)?.ok_or_else(|| format!("{name} must be a number").into())
    }

    /// A finite number, or `None` when it is left out.
    pub(crate) fn opt_f64(self, name: &str) -> Result<Option<f64>, CommandError> {
        match self.value(name) {
            None => Ok(None),
            Some(v) => {
                v.as_f64().filter(|n| n.is_finite()).map(Some).ok_or_else(|| format!("{name} must be a number").into())
            }
        }
    }

    /// Like [`Args::f64`], but also the text `"-inf"`, for a level in decibels that is silence.
    pub(crate) fn f64_or_neg_infinity(self, name: &str) -> Result<f64, CommandError> {
        match self.value(name) {
            Some(Value::String(s)) if s == "-inf" => Ok(f64::NEG_INFINITY),
            _ => self.f64(name),
        }
    }

    /// A whole number from 0, as an index; `what` says what it counts in the error.
    pub(crate) fn index(self, name: &str, what: &str) -> Result<usize, CommandError> {
        self.opt_index(name, what)?.ok_or_else(|| format!("{name} must be a whole number {what}").into())
    }

    /// Like [`Args::index`], or `None` when it is left out.
    pub(crate) fn opt_index(self, name: &str, what: &str) -> Result<Option<usize>, CommandError> {
        match self.value(name) {
            None => Ok(None),
            Some(v) => v
                .as_u64()
                .and_then(|i| usize::try_from(i).ok())
                .map(Some)
                .ok_or_else(|| format!("{name} must be a whole number {what}").into()),
        }
    }

    /// `true` or `false`, which must be given.
    pub(crate) fn bool(self, name: &str) -> Result<bool, CommandError> {
        self.opt_bool(name)?.ok_or_else(|| format!("{name} must be true or false").into())
    }

    /// `true` or `false`, or `None` when it is left out.
    pub(crate) fn opt_bool(self, name: &str) -> Result<Option<bool>, CommandError> {
        match self.value(name) {
            None => Ok(None),
            Some(v) => v.as_bool().map(Some).ok_or_else(|| format!("{name} must be true or false").into()),
        }
    }

    /// A switch that is off when it is left out.
    pub(crate) fn flag(self, name: &str) -> Result<bool, CommandError> {
        Ok(self.opt_bool(name)?.unwrap_or(false))
    }

    /// A text with something in it, trimmed.
    pub(crate) fn text(self, name: &str) -> Result<&'a str, CommandError> {
        self.opt_text(name)?.ok_or_else(|| format!("{name} must be a text, not empty").into())
    }

    /// Like [`Args::text`], or `None` when it is left out.
    pub(crate) fn opt_text(self, name: &str) -> Result<Option<&'a str>, CommandError> {
        match self.value(name) {
            None => Ok(None),
            Some(v) => v
                .as_str()
                .map(str::trim)
                .filter(|n| !n.is_empty())
                .map(Some)
                .ok_or_else(|| format!("{name} must be a text, not empty").into()),
        }
    }

    /// A text as it was given, untrimmed and possibly empty; `None` when it is left out or not a
    /// text.
    pub(crate) fn raw_str(self, name: &str) -> Option<&'a str> {
        self.value(name).and_then(Value::as_str)
    }

    /// Like [`Args::text`], owned.
    pub(crate) fn name(self, name: &str) -> Result<String, CommandError> {
        self.text(name).map(str::to_string)
    }

    /// Like [`Args::opt_text`], owned.
    pub(crate) fn opt_name(self, name: &str) -> Result<Option<String>, CommandError> {
        Ok(self.opt_text(name)?.map(str::to_string))
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
        assert_eq!(args.opt_index("gone", "from 0"), Ok(None));
        assert_eq!(args.text("name"), Ok("Drop"));
        assert_eq!(args.opt_text("gone"), Ok(None));
        assert!(!args.has("gone"));
        assert!(args.has("bad"));
    }

    #[test]
    fn switches_and_silence() {
        let params = json!({ "on": true, "off": false, "level": "-inf", "bad": 1 });
        let args = Args::new(&params);
        assert_eq!(args.bool("on"), Ok(true));
        assert_eq!(args.flag("gone"), Ok(false));
        assert_eq!(args.opt_bool("off"), Ok(Some(false)));
        assert!(args.flag("bad").is_err());
        assert!(args.bool("gone").is_err());
        assert_eq!(args.f64_or_neg_infinity("level"), Ok(f64::NEG_INFINITY));
    }
}
