// Copyright (C) 2026 The Suhina contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! Source-preserving native declaration edits. Conditional rules remain authored text.
use std::ops::Range;

use cssparser::{Parser, ParserInput, Token};

#[derive(Debug)]
struct Declaration {
    selector: String,
    property: String,
    value: Range<usize>,
    whole: Range<usize>,
}

fn consume_block(parser: &mut Parser<'_, '_>) -> Result<(), String> {
    // inspect() has already bounded nesting. Explicitly consume blocks so their
    // delimiters and semicolons never become declarations of the containing rule.
    while !parser.is_exhausted() {
        let token = parser.next_including_whitespace_and_comments().map_err(|e| format!("CSS: {e:?}"))?.clone();
        if matches!(
            token,
            Token::Function(_) | Token::ParenthesisBlock | Token::SquareBracketBlock | Token::CurlyBracketBlock
        ) {
            nested(parser, consume_block)?;
        }
        if matches!(
            token,
            Token::BadString(_)
                | Token::BadUrl(_)
                | Token::CloseCurlyBracket
                | Token::CloseParenthesis
                | Token::CloseSquareBracket
        ) {
            return Err("Malformed CSS cannot be edited safely".into());
        }
    }
    Ok(())
}

fn nested<T>(
    parser: &mut Parser<'_, '_>,
    read: impl FnOnce(&mut Parser<'_, '_>) -> Result<T, String>,
) -> Result<T, String> {
    let mut outcome = None;
    parser
        .parse_nested_block(|inner| {
            outcome = Some(read(inner));
            Ok::<(), cssparser::ParseError<'_, ()>>(())
        })
        .map_err(|e| format!("CSS block: {e:?}"))?;
    outcome.ok_or("Missing CSS block")?
}

fn declarations(source: &str) -> Result<Vec<Declaration>, String> {
    crate::syntax::validate(source)?;
    let mut input = ParserInput::new(source);
    let mut parser = Parser::new(&mut input);
    let mut result = Vec::new();
    let mut start = None;
    while !parser.is_exhausted() {
        let position = parser.position().byte_index();
        let token = parser.next_including_whitespace_and_comments().map_err(|e| format!("CSS: {e:?}"))?.clone();
        match token {
            Token::WhiteSpace(_) | Token::Comment(_) if start.is_none() => {}
            Token::Semicolon => start = None,
            Token::CurlyBracketBlock => {
                let selector = source.get(start.unwrap_or(position)..position).ok_or("Invalid CSS range")?.trim();
                if selector.starts_with('@') {
                    nested(&mut parser, consume_block)?;
                } else {
                    nested(&mut parser, |body| {
                        while !body.is_exhausted() {
                            body.skip_whitespace();
                            let whole_start = body.position().byte_index();
                            if body.try_parse(|p| p.expect_semicolon()).is_ok() {
                                continue;
                            }
                            let property =
                                body.expect_ident_cloned().map_err(|e| format!("CSS property: {e:?}"))?.to_string();
                            body.expect_colon().map_err(|e| format!("CSS declaration: {e:?}"))?;
                            let mut value_start = None;
                            let mut value_end = body.position().byte_index();
                            while !body.is_exhausted() {
                                let before = body.position().byte_index();
                                let token = body
                                    .next_including_whitespace_and_comments()
                                    .map_err(|e| format!("CSS value: {e:?}"))?
                                    .clone();
                                if matches!(token, Token::Semicolon) {
                                    break;
                                }
                                if matches!(token, Token::WhiteSpace(_) | Token::Comment(_)) {
                                    continue;
                                }
                                value_start.get_or_insert(before);
                                if matches!(
                                    token,
                                    Token::Function(_)
                                        | Token::ParenthesisBlock
                                        | Token::SquareBracketBlock
                                        | Token::CurlyBracketBlock
                                ) {
                                    nested(body, consume_block)?;
                                }
                                value_end = body.position().byte_index();
                            }
                            if result.len() >= 16384 {
                                return Err("Too many CSS declarations".into());
                            }
                            result.push(Declaration {
                                selector: selector.into(),
                                property,
                                value: value_start.unwrap_or(value_end)..value_end,
                                whole: whole_start..body.position().byte_index(),
                            });
                        }
                        Ok(())
                    })?;
                }
                if source.as_bytes().get(parser.position().byte_index().saturating_sub(1)) != Some(&b'}') {
                    return Err("Unclosed CSS rule cannot be edited safely".into());
                }
                start = None;
            }
            Token::Function(_) | Token::ParenthesisBlock | Token::SquareBracketBlock => {
                start.get_or_insert(position);
                nested(&mut parser, consume_block)?;
            }
            Token::BadString(_)
            | Token::BadUrl(_)
            | Token::CloseCurlyBracket
            | Token::CloseParenthesis
            | Token::CloseSquareBracket => return Err("Malformed CSS cannot be edited safely".into()),
            _ => {
                start.get_or_insert(position);
            }
        }
    }
    if start.is_some() {
        return Err("Incomplete CSS rule cannot be edited safely".into());
    }
    Ok(result)
}

/// Apply the difference between two native projections to authored CSS. Only changed
/// declarations are touched; comments, formatting and conditional rules stay in place.
pub fn update(source: &str, before: &str, after: &str) -> Result<String, String> {
    let old = declarations(before)?;
    let new = declarations(after)?;
    let authored = declarations(source)?;
    let value = |text: &str, range: &Range<usize>| text.get(range.clone()).unwrap_or_default().to_string();
    let same_key = |a: &Declaration, b: &Declaration| a.selector == b.selector && a.property == b.property;
    let mut edits = Vec::new();
    let mut append = String::new();
    for next in &new {
        if old
            .iter()
            .find(|entry| same_key(entry, next))
            .is_some_and(|entry| value(before, &entry.value) == value(after, &next.value))
        {
            continue;
        }
        let replacement = value(after, &next.value);
        // Change every unconditional duplicate: otherwise an earlier !important
        // declaration can silently defeat the editor's change.
        let matches: Vec<_> = authored.iter().filter(|entry| same_key(entry, next)).collect();
        if matches.is_empty() {
            append.push_str(&format!("\n{} {{ {}: {}; }}\n", next.selector, next.property, replacement));
        } else {
            for entry in matches {
                edits.push((entry.value.clone(), replacement.clone()));
            }
        }
    }
    for previous in &old {
        if !new.iter().any(|entry| same_key(entry, previous)) {
            for entry in authored.iter().filter(|entry| same_key(entry, previous)) {
                edits.push((entry.whole.clone(), String::new()));
            }
        }
    }
    edits.sort_by_key(|(range, _)| std::cmp::Reverse(range.start));
    let mut result = source.to_string();
    for (range, replacement) in edits {
        result.replace_range(range, &replacement);
    }
    result.push_str(&append);
    crate::syntax::validate(&result)?;
    Ok(result)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn palette_edit_preserves_comments_unicode_and_conditional_rules() {
        let source = "/* revontuli 🌌 */\n:root { --accent: #123456 /* säilytä */; --text-primary: #ffffff; }\n@media (max-width: 800px) { :root { --accent: #abcdef; } }\n.button { background: url(\"semi;colon.svg\"); }\n";
        let before = ":root { --accent: #123456; --text-primary: #ffffff; }";
        let after = ":root { --accent: #654321; --text-primary: #ffffff; }";
        assert_eq!(update(source, before, after).unwrap(), source.replacen("#123456", "#654321", 1));
        assert_eq!(update(source, before, before).unwrap(), source);
    }

    #[test]
    fn additions_removals_and_duplicates_do_not_rewrite_other_rules() {
        let source = ":root { --accent: #123456 !important; }\n:root { --accent: #111111; }\nmeter { style: needle; }\n/* keep */";
        let before = ":root { --accent: #123456; } meter { style: needle; }";
        let after = ":root { --accent: #654321; } artwork { fps: 10; }";
        let result = update(source, before, after).unwrap();
        assert_eq!(result.matches("#654321").count(), 2);
        assert!(!result.contains("style: needle"));
        assert!(result.contains("/* keep */"));
        assert!(result.contains("artwork { fps: 10; }"));
    }

    #[test]
    fn malformed_or_excessive_source_is_refused_without_panicking() {
        for source in [":root { --accent: #123456;", "}", "button", "button { : bad; }"] {
            assert!(update(source, "", "").is_err(), "{source}");
        }
        assert!(update(&" ".repeat(crate::MAX_SOURCE_BYTES + 1), "", "").is_err());
    }
}
