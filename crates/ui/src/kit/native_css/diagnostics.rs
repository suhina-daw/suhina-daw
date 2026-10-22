// Copyright (C) 2026 The Suhina contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! Token-level input budgets and native-painter notices. Stylo owns CSS semantics.
use cssparser::{Parser, ParserInput, Token};

const MAX_NESTING: usize = 64;

/// Names a preserved declaration which the native painter does not implement yet.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Notice {
    pub line: u32,
    pub property: String,
}

fn not_painted(name: &str) -> bool {
    name.starts_with("transform")
        || matches!(
            name,
            "filter"
                | "backdrop-filter"
                | "mix-blend-mode"
                | "background-blend-mode"
                | "font-family"
                | "font-weight"
                | "font-style"
                | "text-decoration"
                | "mask"
                | "clip-path"
                | "perspective"
        )
}

fn walk(
    parser: &mut Parser<'_, '_>,
    depth: usize,
    declarations: bool,
    notices: &mut Vec<Notice>,
) -> Result<(), String> {
    if depth > MAX_NESTING {
        return Err("Advanced CSS exceeds 64 nested blocks or functions".into());
    }
    let mut identifier = None;
    let mut candidate: Option<Notice> = None;
    let emit = |candidate: &mut Option<Notice>, notices: &mut Vec<Notice>| {
        if let Some(notice) = candidate.take()
            && not_painted(&notice.property)
            && notices.len() < 32
            && !notices.contains(&notice)
        {
            notices.push(notice);
        }
    };
    while !parser.is_exhausted() {
        let line = parser.current_source_location().line + 1;
        let Ok(token) = parser.next_including_whitespace_and_comments().cloned() else {
            break;
        };
        match token {
            Token::Ident(name) if declarations && candidate.is_none() => identifier = Some((name.to_string(), line)),
            Token::Colon if declarations => {
                if let Some((property, line)) = identifier.take() {
                    candidate = Some(Notice { line, property });
                }
            }
            Token::Semicolon => {
                emit(&mut candidate, notices);
                identifier = None;
            }
            Token::CurlyBracketBlock => {
                candidate = None;
                identifier = None;
                let mut budget_error = None;
                let result = parser.parse_nested_block(|nested| {
                    if let Err(error) = walk(nested, depth + 1, true, notices) {
                        budget_error = Some(error);
                    }
                    Ok::<(), cssparser::ParseError<'_, ()>>(())
                });
                if let Some(error) = budget_error {
                    return Err(error);
                }
                let _ = result;
            }
            Token::Function(_) | Token::ParenthesisBlock | Token::SquareBracketBlock => {
                let mut budget_error = None;
                let result = parser.parse_nested_block(|nested| {
                    if let Err(error) = walk(nested, depth + 1, false, notices) {
                        budget_error = Some(error);
                    }
                    Ok::<(), cssparser::ParseError<'_, ()>>(())
                });
                if let Some(error) = budget_error {
                    return Err(error);
                }
                let _ = result;
            }
            Token::WhiteSpace(_) | Token::Comment(_) => {}
            _ => identifier = None,
        }
    }
    emit(&mut candidate, notices);
    Ok(())
}

pub fn inspect(source: &str) -> Result<Vec<Notice>, String> {
    if source.len() > super::super::theme_css::MAX_CSS {
        return Err("Advanced CSS exceeds 128 KiB".into());
    }
    let mut input = ParserInput::new(source);
    let mut parser = Parser::new(&mut input);
    let mut notices = Vec::new();
    walk(&mut parser, 0, false, &mut notices)?;
    Ok(notices)
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn notices_skip_selector_colons_comments_and_value_strings() {
        let notices=inspect("/* transform: none */ .button:hover { color:rgb(1 2 3); text-shadow:1px 2px black; } @media (width > 1px) { .browser { font-family:'a:b;c'; } }").unwrap();
        assert_eq!(notices.iter().map(|n| n.property.as_str()).collect::<Vec<_>>(), ["font-family"]);
    }
    #[test]
    fn nested_css_is_bounded_without_rejecting_quoted_braces() {
        assert!(inspect(".browser { --text:'{{{{{{{{'; color:var(--ink,rgb(1 2 3)); }").is_ok());
        let deep = format!("{}color:red;{}", ".a {".repeat(66), "}".repeat(66));
        assert!(inspect(&deep).unwrap_err().contains("64 nested"));
    }
}
