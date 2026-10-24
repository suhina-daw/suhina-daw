// Copyright (C) 2026 The Suhina contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! Problems in a theme's style sheet, from the same parser that styles the window.
use suhina_style::sheet::Sheet;

/// A declaration, selector or rule the native engine skips, with where it is.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Notice {
    pub line: u32,
    pub column: u32,
    pub message: String,
}

/// `Err` when the sheet cannot be used at all (too large, too deeply nested, unbalanced);
/// otherwise what it skips. A skipped declaration never stops the rest from applying.
pub fn inspect(source: &str) -> Result<Vec<Notice>, String> {
    if source.len() > super::super::theme_css::MAX_CSS {
        return Err("Advanced CSS exceeds 128 KiB".into());
    }
    let sheet = Sheet::parse(source)?;
    Ok(sheet
        .notices
        .into_iter()
        .map(|notice| Notice { line: notice.line, column: notice.column, message: notice.message })
        .collect())
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn notices_name_unpainted_properties_and_skip_comments_and_strings() {
        let notices = inspect(
            "/* transform: none */ .button:hover { color:rgb(1 2 3); text-shadow:1px 2px black; } @media (width > 1px) { .browser { font-family:'a:b;c'; } }",
        )
        .unwrap();
        assert_eq!(notices.len(), 1, "{notices:?}");
        assert!(notices[0].message.contains("font-family"), "{notices:?}");
    }
    #[test]
    fn nested_css_is_bounded_without_rejecting_quoted_braces() {
        assert!(inspect(".browser { --text:'{{{{{{{{'; color:var(--ink,rgb(1 2 3)); }").is_ok());
        let deep = format!("{}color:red;{}", ".a {".repeat(66), "}".repeat(66));
        assert!(inspect(&deep).is_err());
    }
}
