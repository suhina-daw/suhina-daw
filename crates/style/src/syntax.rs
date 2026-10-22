// Copyright (C) 2026 The Suhina contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! Token-level budgets shared by parsing and source editing.
use cssparser::{Parser, ParserInput, Token};

/// Validate syntax structure before recursing into selectors or values. This does
/// not advertise property support; that belongs to the native property registry.
pub fn validate(source: &str) -> Result<(), String> {
    if source.len() > crate::MAX_SOURCE_BYTES {
        return Err("Style sheet exceeds 128 KiB".into());
    }
    let mut input = ParserInput::new(source);
    walk(&mut Parser::new(&mut input), 0)
}

fn walk(parser: &mut Parser<'_, '_>, depth: usize) -> Result<(), String> {
    if depth > crate::MAX_NESTING {
        return Err("Style sheet exceeds 64 nested blocks or functions".into());
    }
    while !parser.is_exhausted() {
        let token = parser.next_including_whitespace_and_comments().map_err(|e| format!("CSS: {e:?}"))?.clone();
        match token {
            Token::BadString(_)
            | Token::BadUrl(_)
            | Token::CloseCurlyBracket
            | Token::CloseParenthesis
            | Token::CloseSquareBracket => return Err("Malformed style sheet token".into()),
            Token::Function(_) | Token::ParenthesisBlock | Token::SquareBracketBlock | Token::CurlyBracketBlock => {
                let mut result = Ok(());
                parser
                    .parse_nested_block(|nested| {
                        result = walk(nested, depth + 1);
                        Ok::<(), cssparser::ParseError<'_, ()>>(())
                    })
                    .map_err(|e| format!("CSS block: {e:?}"))?;
                result?;
            }
            _ => {}
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    #[test]
    fn hostile_nesting_is_bounded_but_quoted_braces_are_text() {
        assert!(super::validate("button { --label: '{{{{{{{{'; color: var(--ink, rgb(1 2 3)); }").is_ok());
        assert!(super::validate(&format!("{}x{}", "a(".repeat(65), ")".repeat(65))).is_err());
        assert!(super::validate(&" ".repeat(crate::MAX_SOURCE_BYTES + 1)).is_err());
    }
}
