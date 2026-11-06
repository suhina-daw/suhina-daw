// Copyright (C) 2026 The Suhina contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! Bounded style rules with source locations and explicit native support diagnostics.
use crate::{media::Media, property::Property, selector::Selector};
use cssparser::{Parser, Token};

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
    /// Written as a shorthand: `value` is the whole shorthand, and this longhand takes its part
    /// once variables are substituted.
    pub shorthand: Option<Shorthand>,
}

/// Shorthands the native engine expands into its longhands.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Shorthand {
    Background,
    Gap,
    Padding,
    Margin,
    Flex,
}

impl Shorthand {
    fn from_name(name: &str) -> Option<Self> {
        Some(match name {
            "background" => Self::Background,
            "gap" => Self::Gap,
            "padding" => Self::Padding,
            "margin" => Self::Margin,
            "flex" => Self::Flex,
            _ => return None,
        })
    }

    fn longhands(self) -> &'static [Property] {
        match self {
            Self::Background => &[Property::BackgroundColor, Property::BackgroundImage],
            Self::Gap => &[Property::RowGap, Property::ColumnGap],
            Self::Padding => {
                &[Property::PaddingTop, Property::PaddingRight, Property::PaddingBottom, Property::PaddingLeft]
            }
            Self::Margin => &[Property::MarginTop, Property::MarginRight, Property::MarginBottom, Property::MarginLeft],
            Self::Flex => &[Property::FlexGrow, Property::FlexShrink, Property::FlexBasis],
        }
    }

    /// The text of `property`'s part of the shorthand `value`.
    pub(crate) fn part(self, property: Property, value: &str) -> Result<String, String> {
        if self == Self::Background {
            return background_part(property, value);
        }
        let words: Vec<&str> = value.split_whitespace().collect();
        let index = self.longhands().iter().position(|p| *p == property).ok_or("Not a longhand of this shorthand")?;
        let pick = |order: &[usize]| order.get(index).and_then(|i| words.get(*i)).map(|w| (*w).to_owned());
        let text = match (self, words.len()) {
            (Self::Gap, 1) => pick(&[0, 0]),
            (Self::Gap, 2) => pick(&[0, 1]),
            (Self::Padding | Self::Margin, 1) => pick(&[0, 0, 0, 0]),
            (Self::Padding | Self::Margin, 2) => pick(&[0, 1, 0, 1]),
            (Self::Padding | Self::Margin, 3) => pick(&[0, 1, 2, 1]),
            (Self::Padding | Self::Margin, 4) => pick(&[0, 1, 2, 3]),
            (Self::Flex, 1) => match words.first().copied() {
                Some("none") => Some(["0", "0", "auto"][index].to_owned()),
                Some("auto") => Some(["1", "1", "auto"][index].to_owned()),
                Some(word) if word.parse::<f32>().is_ok() => Some([word, "1", "0px"][index].to_owned()),
                Some(word) => Some(["1", "1", word][index].to_owned()),
                None => None,
            },
            (Self::Flex, 2) => match words.as_slice() {
                [grow, other] if other.parse::<f32>().is_ok() => Some([*grow, *other, "0px"][index].to_owned()),
                [grow, basis] => Some([*grow, "1", *basis][index].to_owned()),
                _ => None,
            },
            (Self::Flex, 3) => pick(&[0, 1, 2]),
            _ => None,
        };
        text.ok_or_else(|| format!("Invalid {} shorthand", self.name()))
    }

    fn name(self) -> &'static str {
        match self {
            Self::Background => "background",
            Self::Gap => "gap",
            Self::Padding => "padding",
            Self::Margin => "margin",
            Self::Flex => "flex",
        }
    }
}

/// The part of a `background` shorthand that sets `property` (`background-color` or
/// `background-image`). Like CSS, an omitted colour is transparent and omitted images are none.
/// Positions, sizes, repeats and URLs are not drawn natively and are an error.
pub(crate) fn background_part(property: Property, value: &str) -> Result<String, String> {
    crate::syntax::validate(value)?;
    let mut parser = Parser::new(value);
    let mut images: Vec<String> = Vec::new();
    let mut color: Option<String> = None;
    let mut layer_has_image = false;
    let mut layers = 1usize;
    while !parser.is_exhausted() {
        let start = parser.position();
        let token = parser.next().map_err(|_| "Invalid background")?.clone();
        match token {
            Token::Comma => {
                if color.is_some() {
                    return Err("A background colour belongs to the last layer only".into());
                }
                if !layer_has_image {
                    images.push("none".into());
                }
                layer_has_image = false;
                layers += 1;
                if layers > 4 {
                    return Err("At most four background layers are supported".into());
                }
            }
            Token::Function(name) if name.to_ascii_lowercase().ends_with("-gradient") => {
                nested(&mut parser, consume)?;
                if layer_has_image {
                    return Err("One image per background layer".into());
                }
                images.push(parser.slice_from(start).trim().to_owned());
                layer_has_image = true;
            }
            Token::Ident(name) if name.eq_ignore_ascii_case("none") && !layer_has_image => {
                images.push("none".into());
                layer_has_image = true;
            }
            Token::Function(_) | Token::Hash(_) | Token::IDHash(_) | Token::Ident(_) => {
                if matches!(token, Token::Function(_)) {
                    nested(&mut parser, consume)?;
                }
                let text = parser.slice_from(start).trim();
                if color.is_some() || crate::value::Color::parse(text).is_err() {
                    return Err("Only colours and gradients are drawn from the background shorthand".into());
                }
                color = Some(text.to_owned());
            }
            _ => return Err("Only colours and gradients are drawn from the background shorthand".into()),
        }
    }
    match property {
        Property::BackgroundColor => Ok(color.unwrap_or_else(|| "transparent".into())),
        Property::BackgroundImage => {
            let drawn: Vec<&String> = images.iter().filter(|image| *image != "none").collect();
            Ok(if drawn.is_empty() {
                "none".into()
            } else {
                drawn.iter().map(|s| s.as_str()).collect::<Vec<_>>().join(", ")
            })
        }
        _ => Err("Not a background longhand".into()),
    }
}

#[derive(Clone, Debug)]
pub(crate) struct Rule {
    pub selectors: Vec<Selector>,
    pub declarations: Vec<Declaration>,
    pub media: Vec<Media>,
    /// The cascade layer (`@layer`), by its index in [`Sheet::layers`]; `None` is unlayered.
    pub layer: Option<u16>,
}

/// Most named cascade layers in one sheet.
pub const MAX_LAYERS: usize = 64;

#[derive(Clone, Debug, Default)]
pub struct Sheet {
    pub(crate) rules: Vec<Rule>,
    pub notices: Vec<Notice>,
    /// Cascade layer names in the order they were first named, nested ones dotted (`a.b`).
    pub(crate) layers: Vec<String>,
    /// `@keyframes` rules by name: offsets from 0 to 1 and their declarations.
    pub(crate) keyframes: Vec<(String, KeyframeRule)>,
    /// `@property` rules in source order; a later rule for the same name replaces an earlier one.
    pub properties: Vec<crate::registered::Registered>,
    /// `@group-class` rules in source order; a later rule for the same name replaces an earlier one.
    pub group_classes: Vec<GroupClass>,
}

/// A toolbar group class a theme offers in the toolbar editor:
///
/// ```css
/// @group-class lcd { label: "LCD display"; }
/// ```
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct GroupClass {
    pub name: String,
    /// What the editor shows; the name when the rule gives none.
    pub label: Option<String>,
}

/// Most `@group-class` rules one sheet may hold.
pub const MAX_GROUP_CLASSES: usize = 64;

/// Properties the host application reads from a theme's rules itself; the engine leaves them
/// alone without a notice. Read by the app, not the engine: artwork, the name, the background material, and the
/// placement offsets `layout.css` writes (`left`, `top`, `right`, `bottom`).
pub const HOST_PROPERTIES: &[&str] =
    &["image", "name", "background-opacity", "background-blur", "left", "top", "right", "bottom"];

/// The frames of one `@keyframes` rule: offsets from 0 to 1 and their declarations.
pub(crate) type KeyframeRule = Vec<(f32, Vec<Declaration>)>;

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
    /// Classes the sheet declares with `@group-class` but no rule styles as `group.<class>`:
    /// probably a typo, which an import warns about.
    pub fn unstyled_group_classes(&self) -> Vec<&str> {
        self.group_classes
            .iter()
            .map(|class| class.name.as_str())
            .filter(|class| {
                !self.rules.iter().any(|rule| rule.selectors.iter().any(|selector| selector.styles("group", class)))
            })
            .collect()
    }

    pub fn parse(source: &str) -> Result<Self, String> {
        crate::syntax::validate(source)?;
        let mut sheet = Self::default();
        rules(&mut Parser::new(source), source, &Scope::default(), &mut sheet, 0)?;
        sheet.rank_layers();
        Ok(sheet)
    }

    /// The index of layer `name`, naming it if it is new.
    fn layer(&mut self, name: &str) -> Result<u16, String> {
        // `@layer a.b;` names `a` first, as nesting would.
        if let Some((parent, _)) = name.rsplit_once('.') {
            self.layer(parent)?;
        }
        if let Some(index) = self.layers.iter().position(|layer| layer == name) {
            return u16::try_from(index).map_err(|_| "Too many cascade layers".into());
        }
        if self.layers.len() >= MAX_LAYERS {
            return Err("Style sheet exceeds 64 cascade layers".into());
        }
        self.layers.push(name.into());
        u16::try_from(self.layers.len() - 1).map_err(|_| "Too many cascade layers".into())
    }

    /// Replaces each rule's layer index by its rank: siblings in the order they were first
    /// named, and a layer's nested layers before the layer itself, whose own rules win over them.
    fn rank_layers(&mut self) {
        fn visit(prefix: &str, layers: &[String], ranks: &mut Vec<(usize, u16)>) {
            for (index, name) in layers.iter().enumerate() {
                let Some(rest) = (if prefix.is_empty() {
                    Some(name.as_str())
                } else {
                    name.strip_prefix(prefix).and_then(|r| r.strip_prefix('.'))
                }) else {
                    continue;
                };
                if rest.contains('.') {
                    continue;
                }
                visit(name, layers, ranks);
                let rank = u16::try_from(ranks.len()).unwrap_or(u16::MAX - 2);
                ranks.push((index, rank));
            }
        }
        let mut ranks = Vec::with_capacity(self.layers.len());
        visit("", &self.layers, &mut ranks);
        for rule in &mut self.rules {
            rule.layer = rule
                .layer
                .and_then(|index| ranks.iter().find(|(i, _)| *i == usize::from(index)).map(|(_, rank)| *rank));
        }
    }

    /// The rank of a layer in the cascade for a declaration: unlayered normal declarations win
    /// over every layer and later layers over earlier ones; for `!important` the order reverses.
    pub(crate) fn layer_rank(layer: Option<u16>, important: bool) -> u16 {
        match (layer, important) {
            (None, false) => u16::MAX,
            (Some(index), false) => index,
            (None, true) => 0,
            (Some(index), true) => u16::MAX - 1 - index.min(u16::MAX - 2),
        }
    }

    fn notice(&mut self, at: cssparser::SourceLocation, message: String) {
        if self.notices.len() < 128 {
            self.notices.push(Notice { line: at.line + 1, column: at.column, message });
        }
    }
}

pub(crate) fn nested<T>(
    parser: &mut Parser<'_>,
    read: impl FnOnce(&mut Parser<'_>) -> Result<T, String>,
) -> Result<T, String> {
    let mut outcome = None;
    parser
        .parse_nested_block(|inner| {
            outcome = Some(read(inner));
            Ok::<(), cssparser::ParseError<()>>(())
        })
        .map_err(|e| format!("CSS block: {e:?}"))?;
    outcome.ok_or("Missing CSS block")?
}

pub(crate) fn consume(parser: &mut Parser<'_>) -> Result<(), String> {
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

/// The `name: value;` pairs of an at-rule's block, without comments; names lowercase.
fn descriptors(parser: &mut Parser<'_>, source: &str) -> Result<Vec<(String, String)>, String> {
    let body = nested(parser, |inner| {
        let start = inner.position().byte_index();
        consume(inner)?;
        Ok(source.get(start..inner.position().byte_index()).unwrap_or_default().to_owned())
    })?;
    let mut text = String::new();
    let mut rest = body.as_str();
    while let Some(open) = rest.find("/*") {
        text.push_str(&rest[..open]);
        rest = rest[open..].find("*/").map_or("", |close| &rest[open + close + 2..]);
    }
    text.push_str(rest);
    Ok(text
        .split(';')
        .filter_map(|d| d.split_once(':'))
        .map(|(d, v)| (d.trim().to_ascii_lowercase(), v.trim().to_owned()))
        .collect())
}

fn group_class_rule(name: &str, descriptors: &[(String, String)]) -> Result<GroupClass, String> {
    let valid = !name.is_empty()
        && name.len() <= 64
        && name.bytes().all(|b| b.is_ascii_alphanumeric() || b == b'-' || b == b'_')
        && !name.starts_with(|c: char| c.is_ascii_digit() || c == '-');
    if !valid {
        return Err(format!("Invalid group class name {name}"));
    }
    let mut class = GroupClass { name: name.to_owned(), label: None };
    for (descriptor, value) in descriptors {
        match descriptor.as_str() {
            "label" => {
                let inner = value
                    .strip_prefix('"')
                    .and_then(|v| v.strip_suffix('"'))
                    .or_else(|| value.strip_prefix('\'').and_then(|v| v.strip_suffix('\'')))
                    .filter(|inner| inner.chars().count() <= 64 && !inner.chars().any(char::is_control))
                    .ok_or("@group-class label: a quoted string of at most 64 characters")?;
                class.label = Some(inner.to_owned());
            }
            other => return Err(format!("Unknown @group-class descriptor {other}")),
        }
    }
    Ok(class)
}

/// The conditions a nested group puts on its rules: media queries and the cascade layer.
#[derive(Clone, Default)]
struct Scope {
    media: Vec<Media>,
    layer: Option<(String, u16)>,
}

/// A layer name: identifiers joined by dots.
fn layer_name(text: &str) -> Option<&str> {
    let text = text.trim();
    (!text.is_empty()
        && text.len() <= 128
        && text.split('.').all(|part| {
            !part.is_empty()
                && part.bytes().all(|b| b.is_ascii_alphanumeric() || b == b'-' || b == b'_')
                && !part.starts_with(|c: char| c.is_ascii_digit())
        }))
    .then_some(text)
}

fn rules(parser: &mut Parser<'_>, source: &str, scope: &Scope, sheet: &mut Sheet, depth: usize) -> Result<(), String> {
    let media = scope.media.as_slice();
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
                let header = source.get(start.unwrap_or(at)..at).unwrap_or_default().trim();
                if let Some(names) = header.strip_prefix("@layer").filter(|s| s.starts_with(char::is_whitespace)) {
                    // `@layer a, b;` fixes the order of layers before their rules appear.
                    for name in names.split(',') {
                        let Some(name) = layer_name(name) else {
                            sheet.notice(location, "Invalid cascade layer name".into());
                            continue;
                        };
                        let full = scope.layer.as_ref().map_or_else(|| name.to_owned(), |(p, _)| format!("{p}.{name}"));
                        sheet.layer(&full)?;
                    }
                } else {
                    sheet.notice(location, "Unsupported statement; imports are not loaded by the style engine".into());
                }
                start = None;
            }
            Token::CurlyBracketBlock => {
                let header = source.get(start.unwrap_or(at)..at).ok_or("Invalid rule span")?.trim();
                let layer = header
                    .strip_prefix("@layer")
                    .filter(|s| s.is_empty() || s.starts_with(char::is_whitespace))
                    .map(str::trim);
                let keyframes =
                    header.strip_prefix("@keyframes").filter(|s| s.starts_with(char::is_whitespace)).map(str::trim);
                let property =
                    header.strip_prefix("@property").filter(|s| s.starts_with(char::is_whitespace)).map(str::trim);
                let group_class =
                    header.strip_prefix("@group-class").filter(|s| s.starts_with(char::is_whitespace)).map(str::trim);
                if let Some(name) = group_class {
                    let descriptors = descriptors(parser, source)?;
                    match group_class_rule(name, &descriptors) {
                        Ok(class) => {
                            sheet.group_classes.retain(|existing| existing.name != class.name);
                            if sheet.group_classes.len() >= MAX_GROUP_CLASSES {
                                return Err("Style sheet exceeds 64 @group-class rules".into());
                            }
                            sheet.group_classes.push(class);
                        }
                        Err(message) => sheet.notice(location, message),
                    }
                } else if let Some(name) = property {
                    let descriptors = descriptors(parser, source)?;
                    match crate::registered::Registered::from_descriptors(name, &descriptors) {
                        Ok(registered) => {
                            sheet.properties.retain(|p| p.name != registered.name);
                            if sheet.properties.len() >= crate::registered::MAX_REGISTERED {
                                return Err("Style sheet exceeds 256 @property rules".into());
                            }
                            sheet.properties.push(registered);
                        }
                        Err(message) => sheet.notice(location, message),
                    }
                } else if let Some(name) = keyframes {
                    if !crate::motion::valid_name(name) {
                        sheet.notice(location, "Invalid keyframes name".into());
                        nested(parser, consume)?;
                    } else {
                        let frames = nested(parser, |parser| frames(parser, source, sheet))?;
                        if sheet.keyframes.len() >= crate::motion::MAX_KEYFRAMES {
                            return Err("Style sheet exceeds 64 keyframes rules".into());
                        }
                        // A later rule of the same name replaces an earlier one.
                        sheet.keyframes.retain(|(existing, _)| existing != name);
                        sheet.keyframes.push((name.to_owned(), frames));
                    }
                } else if let Some(name) = layer {
                    // An anonymous layer gets a name no author can write.
                    let own = if name.is_empty() {
                        Some(format!("{}", sheet.layers.len()))
                    } else {
                        layer_name(name).map(str::to_owned)
                    };
                    match own {
                        Some(own) => {
                            let full = scope
                                .layer
                                .as_ref()
                                .map_or_else(|| own.clone(), |(parent, _)| format!("{parent}.{own}"));
                            let index = sheet.layer(&full)?;
                            let inner = Scope { media: media.to_vec(), layer: Some((full, index)) };
                            nested(parser, |parser| rules(parser, source, &inner, sheet, depth + 1))?;
                        }
                        None => {
                            sheet.notice(location, "Invalid cascade layer name".into());
                            nested(parser, consume)?;
                        }
                    }
                } else if let Some(condition) =
                    header.strip_prefix("@media").filter(|s| s.starts_with(char::is_whitespace) || s.starts_with('('))
                {
                    match Media::parse(condition.trim()) {
                        Ok(condition) => {
                            let mut conditions = media.to_vec();
                            conditions.push(condition);
                            let inner = Scope { media: conditions, layer: scope.layer.clone() };
                            nested(parser, |parser| rules(parser, source, &inner, sheet, depth + 1))?;
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
                            sheet.rules.push(Rule {
                                selectors,
                                declarations,
                                media: media.to_vec(),
                                layer: scope.layer.as_ref().map(|(_, index)| *index),
                            });
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

/// The inside of `@keyframes`: offset lists (`from`, `to`, percentages) and declarations.
fn frames(parser: &mut Parser<'_>, source: &str, sheet: &mut Sheet) -> Result<Vec<(f32, Vec<Declaration>)>, String> {
    let mut result: Vec<(f32, Vec<Declaration>)> = Vec::new();
    let mut start = None;
    while !parser.is_exhausted() {
        let at = parser.position().byte_index();
        let location = parser.current_source_location();
        let token = parser.next_including_whitespace_and_comments().map_err(|e| format!("Keyframes: {e:?}"))?.clone();
        match token {
            Token::WhiteSpace(_) | Token::Comment(_) if start.is_none() => {}
            Token::CurlyBracketBlock => {
                let header = source.get(start.unwrap_or(at)..at).ok_or("Invalid keyframe span")?.trim().to_owned();
                start = None;
                let declarations = nested(parser, |parser| declarations(parser, source, sheet))?;
                let mut offsets = Vec::new();
                for offset in header.split(',').map(str::trim) {
                    let value = match offset {
                        "from" => Some(0.0),
                        "to" => Some(1.0),
                        percent => percent
                            .strip_suffix('%')
                            .and_then(|n| n.trim().parse::<f32>().ok())
                            .filter(|n| n.is_finite() && (0.0..=100.0).contains(n))
                            .map(|n| n / 100.0),
                    };
                    match value {
                        Some(value) => offsets.push(value),
                        None => sheet.notice(location, format!("Invalid keyframe offset {offset}")),
                    }
                }
                for offset in offsets {
                    if result.len() >= crate::motion::MAX_FRAMES {
                        return Err("Keyframes exceed 32 frames".into());
                    }
                    result.push((offset, declarations.clone()));
                }
            }
            _ => {
                if start.is_none() {
                    start = Some(at);
                }
            }
        }
    }
    result.sort_by(|a, b| a.0.total_cmp(&b.0));
    Ok(result)
}

fn declarations(parser: &mut Parser<'_>, source: &str, sheet: &mut Sheet) -> Result<Vec<Declaration>, String> {
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
            Ok::<_, cssparser::BasicParseError>(name)
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
        if let Some(shorthand) = Shorthand::from_name(&name) {
            let raw = source.get(value_start..value_end).ok_or("Invalid value span")?.trim();
            let (value, important) = raw.strip_suffix("!important").map_or((raw, false), |s| (s.trim_end(), true));
            if !value.contains("var(")
                && !crate::variables::mentions_attr(value)
                && !matches!(value, "inherit" | "initial" | "unset")
            {
                let failure = shorthand.longhands().iter().find_map(|property| {
                    shorthand
                        .part(*property, value)
                        .and_then(|part| {
                            property.parse(&crate::value::viewport_units(&part, 1000.0, 1000.0)).map(|_| ())
                        })
                        .err()
                });
                if let Some(message) = failure {
                    sheet.notice(at, format!("{name}: {message}"));
                    continue;
                }
            }
            if declarations.len() + shorthand.longhands().len() > 256 {
                return Err("Style rule exceeds 256 declarations".into());
            }
            for longhand in shorthand.longhands() {
                declarations.push(Declaration {
                    name: longhand.name().into(),
                    value: value.into(),
                    important,
                    shorthand: (!matches!(value, "inherit" | "initial" | "unset")).then_some(shorthand),
                });
            }
            continue;
        }
        if name.len() > 128 {
            sheet.notice(at, "Property name exceeds 128 bytes".into());
            continue;
        }
        if HOST_PROPERTIES.contains(&name.as_str()) {
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
            && !crate::variables::mentions_attr(value)
            && !matches!(value, "inherit" | "initial" | "unset")
            && let Some(property) = Property::from_name(&name)
            && let Err(message) = property.parse(&crate::value::viewport_units(value, 1000.0, 1000.0))
        {
            sheet.notice(at, format!("{name}: {message}"));
            continue;
        }
        declarations.push(Declaration { name, value: value.into(), important, shorthand: None });
    }
    Ok(declarations)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn group_classes_are_declared_with_a_label_and_checked_for_a_rule() {
        let sheet = Sheet::parse(
            "@group-class lcd { label: \"LCD display\"; } \
             @group-class pill { /* no label */ } \
             @group-class typo { label: 'Typo'; } \
             @group-class lcd { label: \"LCD\"; } \
             @group-class 9lives { label: \"Bad\"; } \
             @group-class bad { colour: red; } \
             toolbar group.lcd { background-color: #1a2a1a; } \
             :is(group.pill, button) { radius: 9px; } \
             group.helper { radius: 2px; }",
        )
        .unwrap();
        assert_eq!(
            sheet.group_classes,
            [
                GroupClass { name: "pill".into(), label: None },
                GroupClass { name: "typo".into(), label: Some("Typo".into()) },
                GroupClass { name: "lcd".into(), label: Some("LCD".into()) },
            ]
        );
        assert_eq!(sheet.notices.len(), 2, "{:?}", sheet.notices);
        assert_eq!(sheet.unstyled_group_classes(), ["typo"]);
    }
}
