// Copyright (C) 2026 The Suhina contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! Bounded style rules with source locations and explicit native support diagnostics.
use crate::{media::Media, property::Property, selector::Selector};
use cssparser::{Parser, ParserInput, Token};

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Notice {
    pub line: u32,
    pub column: u32,
    pub message: String,
}

#[derive(Clone, Debug)]
pub(crate) struct Declaration {
    pub name: String,
    pub value: String,
    pub important: bool,
}

#[derive(Clone, Debug)]
pub(crate) struct Rule {
    pub selectors: Vec<Selector>,
    pub declarations: Vec<Declaration>,
    pub media: Vec<Media>,
}

#[derive(Clone, Debug, Default)]
pub struct Sheet {
    pub(crate) rules: Vec<Rule>,
    pub notices: Vec<Notice>,
}

impl Sheet {
    /// True only when this whole sheet can use the staged native paint adapter.
    /// Unknown syntax and layout declarations stay on the compatibility adapter.
    pub fn supports_native_paint(&self) -> bool {
        self.notices.is_empty()
            && self.rules.iter().all(|rule| {
                rule.declarations.iter().all(|declaration| {
                    declaration.name.starts_with("--")
                        || matches!(
                            Property::from_name(&declaration.name),
                            Some(
                                Property::BackgroundImage
                                    | Property::Color
                                    | Property::BackgroundColor
                                    | Property::BorderColor
                                    | Property::Radius
                                    | Property::FontSize
                                    | Property::Opacity
                                    | Property::BoxShadow
                                    | Property::TextShadow
                            )
                        )
                })
            })
    }
    pub fn parse(source: &str) -> Result<Self, String> {
        crate::syntax::validate(source)?;
        let mut input = ParserInput::new(source);
        let mut sheet = Self::default();
        rules(&mut Parser::new(&mut input), source, &[], &mut sheet, 0)?;
        Ok(sheet)
    }

    fn notice(&mut self, at: cssparser::SourceLocation, message: String) {
        if self.notices.len() < 128 {
            self.notices.push(Notice { line: at.line + 1, column: at.column, message });
        }
    }
}

pub(crate) fn nested<T>(
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

pub(crate) fn consume(parser: &mut Parser<'_, '_>) -> Result<(), String> {
    while !parser.is_exhausted() {
        let token = parser.next_including_whitespace_and_comments().map_err(|e| format!("CSS: {e:?}"))?.clone();
        if matches!(
            token,
            Token::Function(_) | Token::ParenthesisBlock | Token::CurlyBracketBlock | Token::SquareBracketBlock
        ) {
            nested(parser, consume)?;
        }
    }
    Ok(())
}

fn rules(
    parser: &mut Parser<'_, '_>,
    source: &str,
    media: &[Media],
    sheet: &mut Sheet,
    depth: usize,
) -> Result<(), String> {
    if depth > 16 {
        return Err("Style rules exceed sixteen nested groups".into());
    }
    let mut start = None;
    let mut location = parser.current_source_location();
    while !parser.is_exhausted() {
        let at = parser.position().byte_index();
        let token = parser.next_including_whitespace_and_comments().map_err(|e| format!("CSS rule: {e:?}"))?.clone();
        match token {
            Token::WhiteSpace(_) | Token::Comment(_) if start.is_none() => {}
            Token::Semicolon => {
                sheet.notice(location, "Unsupported statement; imports are not loaded by the style engine".into());
                start = None;
            }
            Token::CurlyBracketBlock => {
                let header = source.get(start.unwrap_or(at)..at).ok_or("Invalid rule span")?.trim();
                if let Some(condition) =
                    header.strip_prefix("@media").filter(|s| s.starts_with(char::is_whitespace) || s.starts_with('('))
                {
                    match Media::parse(condition.trim()) {
                        Ok(condition) => {
                            let mut conditions = media.to_vec();
                            conditions.push(condition);
                            nested(parser, |parser| rules(parser, source, &conditions, sheet, depth + 1))?;
                        }
                        Err(message) => {
                            sheet.notice(location, message);
                            nested(parser, consume)?;
                        }
                    }
                } else {
                    match Selector::parse_list(header) {
                        Ok(selectors) => {
                            let declarations = nested(parser, |parser| declarations(parser, source, sheet))?;
                            if sheet.rules.len() >= 1024 {
                                return Err("Style sheet exceeds 1024 rules".into());
                            }
                            sheet.rules.push(Rule { selectors, declarations, media: media.to_vec() });
                        }
                        Err(message) => {
                            sheet.notice(location, message);
                            nested(parser, consume)?;
                        }
                    }
                }
                start = None;
            }
            Token::ParenthesisBlock | Token::Function(_) | Token::SquareBracketBlock => {
                if start.is_none() {
                    location = parser.current_source_location();
                    start = Some(at);
                }
                nested(parser, consume)?;
            }
            _ => {
                if start.is_none() {
                    location = parser.current_source_location();
                    start = Some(at);
                }
            }
        }
    }
    if start.is_some() {
        sheet.notice(location, "Incomplete style rule".into());
    }
    Ok(())
}

fn declarations(parser: &mut Parser<'_, '_>, source: &str, sheet: &mut Sheet) -> Result<Vec<Declaration>, String> {
    let mut declarations = Vec::new();
    while !parser.is_exhausted() {
        parser.skip_whitespace();
        if parser.try_parse(|p| p.expect_semicolon()).is_ok() {
            continue;
        }
        let at = parser.current_source_location();
        let property = parser.try_parse(|parser| {
            let name = parser.expect_ident_cloned()?.to_string();
            parser.expect_colon()?;
            Ok::<_, cssparser::BasicParseError<'_>>(name)
        });
        let value_start = parser.position().byte_index();
        let mut value_end = value_start;
        while !parser.is_exhausted() {
            let end = parser.position().byte_index();
            let token =
                parser.next_including_whitespace_and_comments().map_err(|e| format!("CSS value: {e:?}"))?.clone();
            if matches!(token, Token::Semicolon) {
                value_end = end;
                break;
            }
            if matches!(
                token,
                Token::Function(_) | Token::ParenthesisBlock | Token::SquareBracketBlock | Token::CurlyBracketBlock
            ) {
                nested(parser, consume)?;
            }
            value_end = parser.position().byte_index();
        }
        let Ok(mut name) = property else {
            sheet.notice(at, "Expected property: value".into());
            continue;
        };
        // One canonical property id, so the compatibility spelling participates
        // in the same cascade rather than becoming an independent winner.
        if name.eq_ignore_ascii_case("border-radius") {
            name = "radius".into();
        }
        if !name.starts_with("--") {
            name.make_ascii_lowercase();
        }
        if name.len() > 128 {
            sheet.notice(at, "Property name exceeds 128 bytes".into());
            continue;
        }
        if !name.starts_with("--") && Property::from_name(&name).is_none() {
            sheet.notice(at, format!("Unknown native property {name}"));
            continue;
        }
        let raw = source.get(value_start..value_end).ok_or("Invalid value span")?.trim();
        let (value, important) = raw.strip_suffix("!important").map_or((raw, false), |s| (s.trim_end(), true));
        if declarations.len() >= 256 {
            return Err("Style rule exceeds 256 declarations".into());
        }
        if !name.starts_with("--")
            && !value.contains("var(")
            && !matches!(value, "inherit" | "initial" | "unset")
            && let Some(property) = Property::from_name(&name)
            && let Err(message) = property.parse(value)
        {
            sheet.notice(at, format!("{name}: {message}"));
            continue;
        }
        declarations.push(Declaration { name, value: value.into(), important });
    }
    Ok(declarations)
}
