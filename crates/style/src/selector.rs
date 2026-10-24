// Copyright (C) 2026 The Suhina contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! Native element selectors, independent of HTML and bounded at device-panel roots.
use cssparser::{Parser, ParserInput, Token};

const MAX_ELEMENTS: usize = 4096;
const MAX_DEPTH: usize = 32;
const MAX_SELECTOR_BYTES: usize = 2048;
const MAX_MATCH_WORK: usize = 16384;

/// States come from the host's controls and project snapshot, never from CSS.
pub const STATES: &[&str] = &[
    "hover",
    "active",
    "focus",
    "focus-visible",
    "focus-within",
    "enabled",
    "disabled",
    "checked",
    "open",
    "selected",
    "playing",
    "queued",
    "recording",
    "armed",
    "muted",
    "dragged",
    "drop-target",
];

/// Presentation identity only. Parameter bindings and audio data never enter this tree.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Element {
    pub kind: String,
    pub id: Option<String>,
    pub classes: Vec<String>,
    pub states: Vec<String>,
    pub parent: Option<usize>,
    /// The project object this element shows (a track's stable name), matched by
    /// `[data-object]` and `[data-object=name]`.
    pub object: Option<String>,
    /// Custom properties the host gives this element (a track's colour as `--track-colour`):
    /// inherited by its descendants, below every rule of every sheet.
    pub variables: Vec<(String, String)>,
}

impl Element {
    pub fn new(kind: &str, parent: Option<usize>) -> Self {
        Self { kind: kind.into(), parent, ..Self::default() }
    }

    pub fn with_id(mut self, id: &str) -> Self {
        self.id = Some(id.into());
        self
    }
}

/// Validated immutable topology. Parents precede their children, preventing cycles.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Tree {
    elements: Vec<Element>,
    children: Vec<Vec<usize>>,
    roots: Vec<usize>,
}

impl Tree {
    pub fn new(elements: Vec<Element>) -> Result<Self, String> {
        if elements.len() > MAX_ELEMENTS {
            return Err("Style tree exceeds 4096 elements".into());
        }
        let mut children = vec![Vec::new(); elements.len()];
        let mut roots = Vec::new();
        let mut depths = Vec::with_capacity(elements.len());
        for (index, element) in elements.iter().enumerate() {
            let name_ok = |name: &str| !name.is_empty() && name.len() <= 128 && !name.chars().any(char::is_control);
            if !name_ok(&element.kind)
                || element.id.as_deref().is_some_and(|id| !name_ok(id))
                || element.object.as_deref().is_some_and(|object| !name_ok(object))
                || element.classes.len() > 32
                || element.classes.iter().any(|name| !name_ok(name))
                || element.states.len() > STATES.len()
                || element.states.iter().any(|state| !STATES.contains(&state.as_str()))
            {
                return Err("Invalid native element identity or state".into());
            }
            let depth = if let Some(parent) = element.parent {
                let depth =
                    depths.get(parent).filter(|_| parent < index).copied().ok_or("Style parent must precede child")?;
                if let Some(siblings) = children.get_mut(parent) {
                    siblings.push(index);
                }
                depth + 1
            } else {
                roots.push(index);
                0
            };
            if depth > MAX_DEPTH {
                return Err("Style tree exceeds 32 levels".into());
            }
            depths.push(depth);
        }
        Ok(Self { elements, children, roots })
    }

    pub fn elements(&self) -> &[Element] {
        &self.elements
    }

    pub fn contains(&self, node: usize, root: usize) -> bool {
        let mut next = Some(node);
        while let Some(index) = next {
            if index == root {
                return self.elements.get(index).is_some();
            }
            next = self.elements.get(index).and_then(|element| element.parent);
        }
        false
    }

    fn parent(&self, node: usize, root: Option<usize>) -> Option<usize> {
        if Some(node) == root { None } else { self.elements.get(node)?.parent }
    }

    fn siblings(&self, node: usize) -> &[usize] {
        self.elements
            .get(node)
            .and_then(|element| element.parent)
            .and_then(|parent| self.children.get(parent))
            .map_or(&self.roots, Vec::as_slice)
    }
}

/// CSS specificity: ids, classes/states, element types.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, PartialOrd, Ord)]
pub struct Specificity(pub u16, pub u16, pub u16);

impl Specificity {
    fn add(self, other: Self) -> Self {
        Self(self.0.saturating_add(other.0), self.1.saturating_add(other.1), self.2.saturating_add(other.2))
    }
}

#[derive(Clone, Copy, Debug)]
enum Relation {
    Descendant,
    Child,
    Next,
    Following,
}

#[derive(Clone, Debug)]
enum Test {
    Kind(String),
    Id(String),
    Class(String),
    State(String),
    Root,
    Empty,
    Nth {
        a: i32,
        b: i32,
        reverse: bool,
    },
    Only,
    Any(Vec<Selector>, bool),
    /// `[data-object]` (any object) or `[data-object=name]`.
    Object(Option<String>),
}

#[derive(Clone, Debug)]
struct Part {
    tests: Vec<Test>,
    relation: Relation,
}

#[derive(Clone, Debug)]
pub struct Selector {
    parts: Vec<Part>,
    pub specificity: Specificity,
}

impl Selector {
    /// Parse a selector list. Unknown states are errors instead of silent non-matches.
    pub fn parse_list(source: &str) -> Result<Vec<Self>, String> {
        if source.len() > MAX_SELECTOR_BYTES {
            return Err("Selector exceeds 2048 bytes".into());
        }
        crate::syntax::validate(source)?;
        let mut input = ParserInput::new(source);
        parse_list(&mut Parser::new(&mut input), 0)
    }

    /// A device sheet supplies its panel as `root`, so ancestor and sibling matching
    /// cannot escape into other devices. App sheets use `None`.
    pub fn matches(&self, tree: &Tree, node: usize, root: Option<usize>) -> bool {
        if root.is_some_and(|root| !tree.contains(node, root)) {
            return false;
        }
        let mut work = MAX_MATCH_WORK;
        self.matches_with_budget(tree, node, root, &mut work)
    }

    pub(crate) fn matches_with_budget(&self, tree: &Tree, node: usize, root: Option<usize>, work: &mut usize) -> bool {
        if root.is_some_and(|root| !tree.contains(node, root)) {
            return false;
        }
        let matched = self.matches_part(tree, node, root, self.parts.len().saturating_sub(1), work);
        matched && *work > 0
    }

    fn matches_part(&self, tree: &Tree, node: usize, root: Option<usize>, part: usize, work: &mut usize) -> bool {
        if *work == 0 {
            return false;
        }
        *work -= 1;
        let Some(element) = tree.elements.get(node) else {
            return false;
        };
        let Some(current) = self.parts.get(part) else {
            return false;
        };
        for test in &current.tests {
            let matches = match test {
                Test::Kind(kind) => &element.kind == kind,
                Test::Id(id) => element.id.as_ref() == Some(id),
                Test::Class(class) => element.classes.contains(class),
                Test::Object(None) => element.object.is_some(),
                Test::Object(Some(name)) => element.object.as_ref() == Some(name),
                Test::State(state) => element.states.contains(state),
                Test::Root => tree.parent(node, root).is_none(),
                Test::Empty => tree.children.get(node).is_none_or(Vec::is_empty),
                Test::Only => Some(node) == root || tree.siblings(node).len() == 1,
                Test::Nth { a, b, reverse } => {
                    let siblings = if Some(node) == root { std::slice::from_ref(&node) } else { tree.siblings(node) };
                    let position = siblings.iter().position(|index| *index == node).unwrap_or(0);
                    let index = if *reverse { siblings.len().saturating_sub(position) } else { position + 1 } as i64;
                    let difference = index - i64::from(*b);
                    if *a == 0 {
                        difference == 0
                    } else {
                        difference % i64::from(*a) == 0 && difference / i64::from(*a) >= 0
                    }
                }
                Test::Any(selectors, negate) => {
                    let found = selectors.iter().any(|selector| {
                        selector.matches_part(tree, node, root, selector.parts.len().saturating_sub(1), work)
                    });
                    found != *negate
                }
            };
            if !matches {
                return false;
            }
        }
        if part == 0 {
            return true;
        }
        match current.relation {
            Relation::Child => {
                tree.parent(node, root).is_some_and(|parent| self.matches_part(tree, parent, root, part - 1, work))
            }
            Relation::Descendant => {
                let mut parent = tree.parent(node, root);
                while let Some(index) = parent {
                    if self.matches_part(tree, index, root, part - 1, work) {
                        return true;
                    }
                    if *work == 0 {
                        return false;
                    }
                    parent = tree.parent(index, root);
                }
                false
            }
            Relation::Next | Relation::Following => {
                if Some(node) == root {
                    return false;
                }
                let previous = tree.siblings(node).iter().take_while(|index| **index != node);
                if matches!(current.relation, Relation::Next) {
                    previous.last().is_some_and(|index| self.matches_part(tree, *index, root, part - 1, work))
                } else {
                    previous.into_iter().any(|index| self.matches_part(tree, *index, root, part - 1, work))
                }
            }
        }
    }
}

fn parse_list(parser: &mut Parser<'_, '_>, depth: usize) -> Result<Vec<Selector>, String> {
    if depth > 8 {
        return Err("Selector logic exceeds eight nested functions".into());
    }
    let mut result = Vec::new();
    let mut parts = Vec::new();
    let mut tests = Vec::new();
    let mut specificity = Specificity::default();
    let mut relation = Relation::Descendant;
    let mut spaced = false;
    let mut universal = false;
    while !parser.is_exhausted() {
        let token = parser.next_including_whitespace_and_comments().map_err(|e| format!("Selector: {e:?}"))?.clone();
        match token {
            Token::Comment(_) => continue,
            Token::WhiteSpace(_) => {
                spaced = !tests.is_empty() || universal;
                continue;
            }
            Token::Comma => {
                finish_part(&mut parts, &mut tests, relation, &mut universal)?;
                result.push(Selector { parts: std::mem::take(&mut parts), specificity });
                if result.len() >= 32 {
                    return Err("Selector list exceeds 32 entries".into());
                }
                specificity = Specificity::default();
                relation = Relation::Descendant;
                spaced = false;
                continue;
            }
            Token::Delim('>' | '+' | '~') => {
                finish_part(&mut parts, &mut tests, relation, &mut universal)?;
                relation = match token {
                    Token::Delim('>') => Relation::Child,
                    Token::Delim('+') => Relation::Next,
                    _ => Relation::Following,
                };
                spaced = false;
                continue;
            }
            _ => {}
        }
        if spaced {
            finish_part(&mut parts, &mut tests, relation, &mut universal)?;
            relation = Relation::Descendant;
            spaced = false;
        }
        let (test, weight) = match token {
            Token::Delim('*') if tests.is_empty() && !universal => {
                universal = true;
                continue;
            }
            Token::Ident(kind) if tests.is_empty() && !universal => {
                (Test::Kind(kind.to_string()), Specificity(0, 0, 1))
            }
            Token::IDHash(id) => (Test::Id(id.to_string()), Specificity(1, 0, 0)),
            Token::SquareBracketBlock => {
                let mut attribute = None;
                parser
                    .parse_nested_block(|parser| {
                        attribute = Some(object_attribute(parser));
                        Ok::<(), cssparser::ParseError<'_, ()>>(())
                    })
                    .map_err(|e| format!("Attribute: {e:?}"))?;
                (Test::Object(attribute.ok_or("Missing attribute")??), Specificity(0, 1, 0))
            }
            Token::Delim('.') => (
                Test::Class(parser.expect_ident_cloned().map_err(|e| format!("Class: {e:?}"))?.to_string()),
                Specificity(0, 1, 0),
            ),
            Token::Colon => match parser.next().map_err(|e| format!("State: {e:?}"))?.clone() {
                Token::Ident(name) => {
                    let test = match name.as_ref() {
                        "root" => Test::Root,
                        "empty" => Test::Empty,
                        "only-child" => Test::Only,
                        "first-child" => Test::Nth { a: 0, b: 1, reverse: false },
                        "last-child" => Test::Nth { a: 0, b: 1, reverse: true },
                        state if STATES.contains(&state) => Test::State(state.into()),
                        _ => return Err(format!("Unknown native state :{name}")),
                    };
                    (test, Specificity(0, 1, 0))
                }
                Token::Function(name) if matches!(name.as_ref(), "is" | "not" | "where") => {
                    let mut inner = None;
                    parser
                        .parse_nested_block(|parser| {
                            inner = Some(parse_list(parser, depth + 1));
                            Ok::<(), cssparser::ParseError<'_, ()>>(())
                        })
                        .map_err(|e| format!("Selector logic: {e:?}"))?;
                    let selectors = inner.ok_or("Missing selector arguments")??;
                    let weight = if name == "where" {
                        Specificity::default()
                    } else {
                        selectors.iter().map(|s| s.specificity).max().unwrap_or_default()
                    };
                    (Test::Any(selectors, name == "not"), weight)
                }
                Token::Function(name) if matches!(name.as_ref(), "nth-child" | "nth-last-child") => {
                    let (a, b) = parser
                        .parse_nested_block(|parser| {
                            cssparser::parse_nth(parser).map_err(cssparser::ParseError::<()>::from)
                        })
                        .map_err(|e| format!("Child index: {e:?}"))?;
                    (Test::Nth { a, b, reverse: name == "nth-last-child" }, Specificity(0, 1, 0))
                }
                _ => return Err("Unsupported native pseudo-class".into()),
            },
            _ => return Err("Unsupported native selector".into()),
        };
        tests.push(test);
        if tests.len() > 32 {
            return Err("Selector compound exceeds 32 tests".into());
        }
        specificity = specificity.add(weight);
    }
    finish_part(&mut parts, &mut tests, relation, &mut universal)?;
    result.push(Selector { parts, specificity });
    Ok(result)
}

/// The inside of `[...]`: only `data-object`, the host's stable object name, exists.
fn object_attribute(parser: &mut Parser<'_, '_>) -> Result<Option<String>, String> {
    let name = parser.expect_ident_cloned().map_err(|_| "Expected an attribute name")?;
    if !name.eq_ignore_ascii_case("data-object") {
        return Err("Only [data-object] is a native attribute".into());
    }
    if parser.is_exhausted() {
        return Ok(None);
    }
    parser.expect_delim('=').map_err(|_| "Only [data-object=name] compares an attribute")?;
    let value = match parser.next().map_err(|_| "Expected an attribute value")?.clone() {
        Token::Ident(value) | Token::QuotedString(value) => value.to_string(),
        _ => return Err("Expected an attribute value".into()),
    };
    parser.expect_exhausted().map_err(|_| "Unexpected attribute suffix")?;
    if value.is_empty() || value.len() > 128 {
        return Err("Invalid attribute value".into());
    }
    Ok(Some(value))
}

fn finish_part(
    parts: &mut Vec<Part>,
    tests: &mut Vec<Test>,
    relation: Relation,
    universal: &mut bool,
) -> Result<(), String> {
    if tests.is_empty() && !*universal {
        return Err("Missing selector component".into());
    }
    if parts.len() >= MAX_DEPTH {
        return Err("Selector exceeds 32 components".into());
    }
    parts.push(Part { tests: std::mem::take(tests), relation });
    *universal = false;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn devices() -> Tree {
        let mut cutoff = Element::new("knob", Some(1)).with_id("cutoff");
        cutoff.states.push("hover".into());
        Tree::new(vec![
            Element::new("window", None),
            Element::new("device-panel", Some(0)).with_id("filter-a"),
            cutoff,
            Element::new("knob", Some(1)).with_id("resonance"),
            Element::new("device-panel", Some(0)).with_id("filter-b"),
            Element::new("knob", Some(4)).with_id("cutoff"),
        ])
        .unwrap()
    }

    #[test]
    fn same_control_id_is_local_to_its_device_sheet() {
        let tree = devices();
        let selectors = Selector::parse_list("device-panel > knob#cutoff:hover").unwrap();
        assert!(selectors[0].matches(&tree, 2, Some(1)));
        assert!(!selectors[0].matches(&tree, 5, Some(1)));
        assert!(!Selector::parse_list("window knob").unwrap()[0].matches(&tree, 2, Some(1)));
        assert!(Selector::parse_list("window knob").unwrap()[0].matches(&tree, 2, None));
        assert!(!Selector::parse_list("device-panel + device-panel").unwrap()[0].matches(&tree, 4, Some(4)));
    }

    #[test]
    fn structure_states_and_specificity_follow_the_native_tree() {
        let tree = devices();
        for source in [
            "knob:first-child",
            "knob:nth-child(2n + 1)",
            "device-panel knob:not(:disabled)",
            ":is(button, #cutoff)",
            ":where(window) knob:empty",
        ] {
            assert!(Selector::parse_list(source).unwrap()[0].matches(&tree, 2, None), "{source}");
        }
        for source in ["knob + knob", "knob ~ #resonance", "knob:last-child", "knob:nth-last-child(1)"] {
            assert!(Selector::parse_list(source).unwrap()[0].matches(&tree, 3, None), "{source}");
        }
        assert_eq!(
            Selector::parse_list(":where(#filter-a) > knob:is(.small, #cutoff)").unwrap()[0].specificity,
            Specificity(1, 0, 1)
        );
    }

    #[test]
    fn object_attribute_selects_a_named_project_object() {
        let mut first = Element::new("mixer", Some(0));
        first.object = Some("track-1".into());
        let mut second = Element::new("mixer", Some(0));
        second.object = Some("track-2".into());
        let tree =
            Tree::new(vec![Element::new("window", None), first, second, Element::new("mixer", Some(0))]).unwrap();
        let named = &Selector::parse_list("mixer[data-object=track-2]").unwrap()[0];
        let quoted = &Selector::parse_list("[data-object='track-1']").unwrap()[0];
        let any = &Selector::parse_list("[data-object]").unwrap()[0];
        assert!(!named.matches(&tree, 1, None) && named.matches(&tree, 2, None));
        assert!(quoted.matches(&tree, 1, None));
        assert!(any.matches(&tree, 1, None) && !any.matches(&tree, 3, None));
        assert_eq!(named.specificity, Specificity(0, 1, 1));
        for bad in ["[href]", "[data-object~=a]", "[data-object=]", "[data-object=a b]"] {
            assert!(Selector::parse_list(bad).is_err(), "{bad}");
        }
    }

    #[test]
    fn invalid_tree_and_hostile_selectors_are_rejected() {
        assert!(Tree::new(vec![Element::new("knob", Some(0))]).is_err());
        for source in
            ["", "> knob", "knob >", "knob,,button", "knob:visitted", "[secret]", "knob::before", "knob:nth-child(NaN)"]
        {
            assert!(Selector::parse_list(source).is_err(), "{source}");
        }
        assert!(Selector::parse_list(&format!("{}knob{}", ":not(".repeat(10), ")".repeat(10))).is_err());
    }
}
