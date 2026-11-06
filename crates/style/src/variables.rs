// Copyright (C) 2026 The Suhina contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! Bounded custom-property substitution with inheritance handled at element boundaries.
use cssparser::{Parser, Token};
use std::collections::{BTreeMap, BTreeSet};

const MAX_EXPANSION: usize = 16 * 1024;
const MAX_VARIABLES: usize = 512;
const MAX_WORK: usize = 131072;

pub(crate) fn resolve(raw: &BTreeMap<String, String>) -> Result<BTreeMap<String, String>, String> {
    if raw.len() > MAX_VARIABLES || raw.values().map(String::len).sum::<usize>() > crate::MAX_SOURCE_BYTES {
        return Err("Custom properties exceed their per-element budget".into());
    }
    let mut graph = BTreeMap::new();
    for (name, value) in raw {
        let mut dependencies = BTreeSet::new();
        references(&mut Parser::new(value), &mut dependencies, 0)?;
        graph.insert(name.clone(), dependencies);
    }
    let mut invalid = BTreeSet::new();
    let mut done = BTreeSet::new();
    for name in raw.keys() {
        cycles(name, &graph, &mut Vec::new(), &mut done, &mut invalid)?;
    }
    let mut resolved = BTreeMap::new();
    let mut work = MAX_WORK;
    for (name, value) in raw {
        if !invalid.contains(name)
            && let Ok(value) = expand(value, raw, &invalid, 0, &mut work)
        {
            resolved.insert(name.clone(), value);
        }
    }
    if work == 0 {
        return Err("Variable substitution exceeded its work budget".into());
    }
    Ok(resolved)
}

fn cycles(
    name: &str,
    graph: &BTreeMap<String, BTreeSet<String>>,
    stack: &mut Vec<String>,
    done: &mut BTreeSet<String>,
    invalid: &mut BTreeSet<String>,
) -> Result<(), String> {
    if let Some(at) = stack.iter().position(|current| current == name) {
        invalid.extend(stack.iter().skip(at).cloned());
        return Ok(());
    }
    if done.contains(name) {
        return Ok(());
    }
    if stack.len() >= 32 {
        return Err("Custom-property dependency depth exceeds 32".into());
    }
    stack.push(name.into());
    if let Some(dependencies) = graph.get(name) {
        for dependency in dependencies {
            cycles(dependency, graph, stack, done, invalid)?;
        }
    }
    stack.pop();
    done.insert(name.into());
    Ok(())
}

fn references(parser: &mut Parser<'_>, output: &mut BTreeSet<String>, depth: usize) -> Result<(), String> {
    if depth > crate::MAX_NESTING {
        return Err("Custom-property nesting limit".into());
    }
    while !parser.is_exhausted() {
        let token = parser.next().map_err(|_| "Malformed custom property")?.clone();
        match token {
            Token::Function(name) => {
                crate::sheet::nested(parser, |nested| {
                    if name.eq_ignore_ascii_case("var") {
                        let name = nested.expect_ident_cloned().map_err(|_| "var() needs a custom-property name")?;
                        output.insert(name.to_string());
                    }
                    references(nested, output, depth + 1)
                })?;
            }
            Token::ParenthesisBlock | Token::SquareBracketBlock | Token::CurlyBracketBlock => {
                crate::sheet::nested(parser, |nested| references(nested, output, depth + 1))?;
            }
            _ => {}
        }
    }
    Ok(())
}

pub(crate) fn substitute(source: &str, variables: &BTreeMap<String, String>) -> Result<String, String> {
    let mut work = MAX_WORK;
    expand(source, variables, &BTreeSet::new(), 0, &mut work)
}

fn expand(
    source: &str,
    variables: &BTreeMap<String, String>,
    invalid: &BTreeSet<String>,
    depth: usize,
    work: &mut usize,
) -> Result<String, String> {
    if depth > 32 {
        return Err("Variable substitution exceeds 32 levels".into());
    }
    expand_tokens(&mut Parser::new(source), source, variables, invalid, depth, work)
}

fn expand_tokens(
    parser: &mut Parser<'_>,
    source: &str,
    variables: &BTreeMap<String, String>,
    invalid: &BTreeSet<String>,
    depth: usize,
    work: &mut usize,
) -> Result<String, String> {
    if depth > 32 {
        return Err("Value nesting exceeds 32 levels".into());
    }
    let mut output = String::new();
    while !parser.is_exhausted() {
        if *work == 0 {
            return Err("Variable substitution exceeded its work budget".into());
        }
        *work -= 1;
        let start = parser.position().byte_index();
        let token = parser.next_including_whitespace_and_comments().map_err(|_| "Invalid value token")?.clone();
        match token {
            Token::Function(name) if name.eq_ignore_ascii_case("var") => {
                let replacement = crate::sheet::nested(parser, |nested| {
                    let name = nested.expect_ident_cloned().map_err(|_| "var() needs a name")?.to_string();
                    if !name.starts_with("--") {
                        return Err("var() needs a custom-property name".into());
                    }
                    let fallback = if nested.is_exhausted() {
                        None
                    } else {
                        nested.expect_comma().map_err(|_| "var() fallback requires a comma")?;
                        let start = nested.position();
                        crate::sheet::consume(nested)?;
                        Some(nested.slice_from(start).to_string())
                    };
                    let resolved = variables
                        .get(&name)
                        .filter(|_| !invalid.contains(&name))
                        .ok_or_else(|| format!("Unresolved custom property {name}"))
                        .and_then(|value| expand(value, variables, invalid, depth + 1, work));
                    match (resolved, fallback) {
                        (Ok(value), _) => Ok(value),
                        (Err(_), Some(value)) => expand(&value, variables, invalid, depth + 1, work),
                        (Err(error), None) => Err(error),
                    }
                })?;
                output.push_str(&replacement);
            }
            Token::Function(_) | Token::ParenthesisBlock | Token::SquareBracketBlock | Token::CurlyBracketBlock => {
                let prefix_end = parser.position().byte_index();
                output.push_str(source.get(start..prefix_end).ok_or("Invalid token range")?);
                output.push_str(&crate::sheet::nested(parser, |nested| {
                    expand_tokens(nested, source, variables, invalid, depth + 1, work)
                })?);
                output.push(match token {
                    Token::SquareBracketBlock => ']',
                    Token::CurlyBracketBlock => '}',
                    _ => ')',
                });
            }
            Token::Comment(_) => output.push(' '),
            _ => output.push_str(source.get(start..parser.position().byte_index()).ok_or("Invalid token range")?),
        }
        if output.len() > MAX_EXPANSION {
            return Err("Variable expansion exceeds 16 KiB".into());
        }
    }
    Ok(output)
}

/// Whether `source` calls `attr()`, so a value without one skips the substitution.
pub(crate) fn mentions_attr(source: &str) -> bool {
    source.as_bytes().windows(5).any(|window| window.eq_ignore_ascii_case(b"attr("))
}

/// Replaces each `attr(name [type(<syntax>)] [, fallback])` with the value of the element's own
/// attribute. A typed value that does not match its syntax, or a missing attribute, takes the
/// fallback; without one the whole value is invalid. Without `type()` the value is a string.
pub(crate) fn substitute_attributes(source: &str, element: &crate::selector::Element) -> Result<String, String> {
    if !mentions_attr(source) {
        return Ok(source.to_owned());
    }
    let mut work = MAX_WORK;
    attributes(&mut Parser::new(source), source, element, 0, &mut work)
}

fn attributes(
    parser: &mut Parser<'_>,
    source: &str,
    element: &crate::selector::Element,
    depth: usize,
    work: &mut usize,
) -> Result<String, String> {
    if depth > 32 {
        return Err("Value nesting exceeds 32 levels".into());
    }
    let mut output = String::new();
    while !parser.is_exhausted() {
        if *work == 0 {
            return Err("attr() substitution exceeded its work budget".into());
        }
        *work -= 1;
        let start = parser.position().byte_index();
        let token = parser.next_including_whitespace_and_comments().map_err(|_| "Invalid value token")?.clone();
        match token {
            Token::Function(name) if name.eq_ignore_ascii_case("attr") => {
                let replacement = crate::sheet::nested(parser, |nested| {
                    let name = nested.expect_ident_cloned().map_err(|_| "attr() needs an attribute name")?;
                    let name = name.to_ascii_lowercase();
                    if !crate::selector::valid_attribute_name(&name) {
                        return Err(format!("Invalid attribute name {name}"));
                    }
                    let syntax = if nested.try_parse(|nested| nested.expect_function_matching("type")).is_ok() {
                        let syntax = crate::sheet::nested(nested, |inner| {
                            let start = inner.position();
                            crate::sheet::consume(inner)?;
                            crate::registered::Syntax::parse(inner.slice_from(start))
                        })?;
                        if syntax == crate::registered::Syntax::Any {
                            return Err("attr() cannot take type(*)".into());
                        }
                        Some(syntax)
                    } else {
                        None
                    };
                    let fallback = if nested.is_exhausted() {
                        None
                    } else {
                        nested.expect_comma().map_err(|_| "attr() fallback requires a comma")?;
                        let start = nested.position();
                        crate::sheet::consume(nested)?;
                        Some(nested.slice_from(start).trim().to_string())
                    };
                    let value = element.attribute(&name).and_then(|value| match &syntax {
                        Some(syntax) => syntax.accepts(value).then(|| value.trim().to_owned()),
                        None => {
                            let mut quoted = String::new();
                            cssparser::serialize_string(value, &mut quoted).ok()?;
                            Some(quoted)
                        }
                    });
                    match (value, fallback) {
                        (Some(value), _) => Ok(value),
                        (None, Some(fallback)) => {
                            attributes(&mut Parser::new(&fallback), &fallback, element, depth + 1, work)
                        }
                        (None, None) => Err(format!("No usable attribute {name}")),
                    }
                })?;
                output.push_str(&replacement);
            }
            Token::Function(_) | Token::ParenthesisBlock | Token::SquareBracketBlock | Token::CurlyBracketBlock => {
                let prefix_end = parser.position().byte_index();
                output.push_str(source.get(start..prefix_end).ok_or("Invalid token range")?);
                output.push_str(&crate::sheet::nested(parser, |nested| {
                    attributes(nested, source, element, depth + 1, work)
                })?);
                output.push(match token {
                    Token::SquareBracketBlock => ']',
                    Token::CurlyBracketBlock => '}',
                    _ => ')',
                });
            }
            Token::Comment(_) => output.push(' '),
            _ => output.push_str(source.get(start..parser.position().byte_index()).ok_or("Invalid token range")?),
        }
        if output.len() > MAX_EXPANSION {
            return Err("attr() expansion exceeds 16 KiB".into());
        }
    }
    Ok(output)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::selector::Element;

    #[test]
    fn attr_reads_the_element_s_own_attribute_by_type_with_a_fallback() {
        let clip = Element::new("clip", None).with_attribute("color", "#ff8800").with_attribute("kind", "kick\"; x");
        let substitute = |source: &str| substitute_attributes(source, &clip);
        assert_eq!(substitute("1px solid attr(color type(<color>))").unwrap(), "1px solid #ff8800");
        assert_eq!(
            substitute("color-mix(in oklab, ATTR(color type(<color>)), black 60%)").unwrap(),
            "color-mix(in oklab, #ff8800, black 60%)"
        );
        assert_eq!(substitute("attr(kind type(<color>), red)").unwrap(), "red");
        assert_eq!(substitute("attr(missing type(<color>), attr(color type(<color>)))").unwrap(), "#ff8800");
        assert_eq!(substitute("attr(kind)").unwrap(), "\"kick\\\"; x\"");
        assert_eq!(substitute("'attr(color)'").unwrap(), "'attr(color)'");
        assert!(substitute("attr(missing type(<color>))").is_err());
        assert!(substitute("attr(kind type(<color>))").is_err());
        assert!(substitute("attr(color type(*))").is_err());
        assert!(substitute("attr(--color)").is_err());
    }

    #[test]
    fn cycles_are_invalid_even_with_internal_fallbacks() {
        let raw = BTreeMap::from([
            ("--a".into(), "var(--b, red)".into()),
            ("--b".into(), "var(--a, blue)".into()),
            ("--c".into(), "var(--a, #123456)".into()),
        ]);
        let resolved = resolve(&raw).unwrap();
        assert!(!resolved.contains_key("--a"));
        assert!(!resolved.contains_key("--b"));
        assert_eq!(resolved["--c"].trim(), "#123456");
        assert_eq!(substitute("var(--a, var(--c))", &resolved).unwrap().trim(), "#123456");
        assert_eq!(substitute("'var(--a)'", &resolved).unwrap(), "'var(--a)'");
    }
    #[test]
    fn exponential_substitution_has_a_budget() {
        let mut raw = BTreeMap::from([("--v0".into(), "1234567890".into())]);
        for index in 1..16 {
            raw.insert(format!("--v{index}"), format!("var(--v{}) var(--v{})", index - 1, index - 1));
        }
        assert!(!resolve(&raw).unwrap().contains_key("--v15"));
    }

    #[test]
    fn empty_expansion_is_bounded_by_work_not_only_output_bytes() {
        let mut raw = BTreeMap::from([("--v0".into(), String::new())]);
        for index in 1..30 {
            raw.insert(format!("--v{index}"), format!("var(--v{})var(--v{})", index - 1, index - 1));
        }
        assert!(resolve(&raw).unwrap_err().contains("work budget"));
    }
}
