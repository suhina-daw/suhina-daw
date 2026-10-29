// Copyright (C) 2026 The Suhina contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! Native element selectors, independent of HTML and bounded at device-panel roots.
use cssparser::{Parser, Token};

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
    /// Attributes the host gives this element (`color`, `kind`, `selected`), tested by attribute
    /// selectors and read by `attr()`. Names are lowercase; an attribute without a value is `""`.
    pub attributes: Vec<(String, String)>,
}

impl Element {
    pub fn new(kind: &str, parent: Option<usize>) -> Self {
        Self { kind: kind.into(), parent, ..Self::default() }
    }

    pub fn with_id(mut self, id: &str) -> Self {
        self.id = Some(id.into());
        self
    }

    pub fn with_attribute(mut self, name: &str, value: &str) -> Self {
        self.attributes.push((name.into(), value.into()));
        self
    }

    /// An attribute's value; `data-object` is the element's [`Element::object`].
    pub fn attribute(&self, name: &str) -> Option<&str> {
        if name == "data-object" {
            return self.object.as_deref();
        }
        self.attributes.iter().find(|(own, _)| own == name).map(|(_, value)| value.as_str())
    }
}

/// Most attributes one element may carry.
pub const MAX_ATTRIBUTES: usize = 32;
/// Longest attribute value, in bytes.
pub const MAX_ATTRIBUTE_BYTES: usize = 256;

/// An attribute name as the host and selectors write it: lowercase ASCII letters, digits and
/// hyphens, starting with a letter.
pub fn valid_attribute_name(name: &str) -> bool {
    !name.is_empty()
        && name.len() <= 64
        && name.starts_with(|c: char| c.is_ascii_lowercase())
        && name.bytes().all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'-')
}

/// Validated immutable topology. Parents precede their children, preventing cycles.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Tree {
    elements: Vec<Element>,
    children: Vec<Vec<usize>>,
    roots: Vec<usize>,
    /// A detached leaf the style cache styles for a moment: it has a parent, but its parent does
    /// not list it, and it is its own only sibling.
    leaf: Option<[usize; 1]>,
}

fn valid_identity(element: &Element) -> bool {
    let name_ok = |name: &str| !name.is_empty() && name.len() <= 128 && !name.chars().any(char::is_control);
    name_ok(&element.kind)
        && element.id.as_deref().is_none_or(name_ok)
        && element.object.as_deref().is_none_or(name_ok)
        && element.classes.len() <= 32
        && element.classes.iter().all(|name| name_ok(name))
        && element.states.len() <= STATES.len()
        && element.states.iter().all(|state| STATES.contains(&state.as_str()))
        && element.attributes.len() <= MAX_ATTRIBUTES
        && element.attributes.iter().enumerate().all(|(at, (name, value))| {
            valid_attribute_name(name)
                && name != "data-object"
                && value.len() <= MAX_ATTRIBUTE_BYTES
                && !value.chars().any(char::is_control)
                && !element.attributes[..at].iter().any(|(earlier, _)| earlier == name)
        })
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
            if !valid_identity(element) {
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
        Ok(Self { elements, children, roots, leaf: None })
    }

    /// Adds `element` as a detached leaf under `parent` until [`Tree::pop_leaf`]: sibling and
    /// child-index selectors see it as an only child, and its parent does not count it.
    pub(crate) fn push_leaf(&mut self, mut element: Element, parent: usize) -> Result<usize, String> {
        if self.leaf.is_some() || parent >= self.elements.len() || !valid_identity(&element) {
            return Err("Invalid native element identity or state".into());
        }
        let mut depth = 1;
        let mut next = self.elements.get(parent).and_then(|element| element.parent);
        while let Some(index) = next {
            depth += 1;
            next = self.elements.get(index).and_then(|element| element.parent);
        }
        if depth > MAX_DEPTH {
            return Err("Style tree exceeds 32 levels".into());
        }
        element.parent = Some(parent);
        let index = self.elements.len();
        self.elements.push(element);
        self.children.push(Vec::new());
        self.leaf = Some([index]);
        Ok(index)
    }

    pub(crate) fn pop_leaf(&mut self) {
        if self.leaf.take().is_some() {
            self.elements.pop();
            self.children.pop();
        }
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

    pub(crate) fn siblings(&self, node: usize) -> &[usize] {
        if let Some(leaf) = self.leaf.as_ref().filter(|leaf| leaf[0] == node) {
            return leaf;
        }
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
    /// `[name]`, or `[name <op> value]` with `i` for an ASCII case-insensitive comparison.
    Attribute(String, Option<(Operator, String, bool)>),
}

/// How an attribute selector compares a value.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Operator {
    /// `=`: the whole value.
    Equals,
    /// `~=`: one of its whitespace-separated words.
    Word,
    /// `|=`: the value, or the value followed by `-`.
    Prefix,
    /// `^=`
    Starts,
    /// `$=`
    Ends,
    /// `*=`
    Contains,
}

impl Operator {
    fn matches(self, actual: &str, expected: &str, fold: bool) -> bool {
        let (actual, expected) = if fold {
            (
                std::borrow::Cow::Owned(actual.to_ascii_lowercase()),
                std::borrow::Cow::Owned(expected.to_ascii_lowercase()),
            )
        } else {
            (std::borrow::Cow::Borrowed(actual), std::borrow::Cow::Borrowed(expected))
        };
        let (actual, expected) = (actual.as_ref(), expected.as_ref());
        match self {
            Self::Equals => actual == expected,
            Self::Word => !expected.is_empty() && actual.split_ascii_whitespace().any(|word| word == expected),
            Self::Prefix => {
                actual == expected || actual.strip_prefix(expected).is_some_and(|rest| rest.starts_with('-'))
            }
            // An empty value matches nothing, as in CSS.
            Self::Starts => !expected.is_empty() && actual.starts_with(expected),
            Self::Ends => !expected.is_empty() && actual.ends_with(expected),
            Self::Contains => !expected.is_empty() && actual.contains(expected),
        }
    }
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
        parse_list(&mut Parser::new(source), 0)
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

    /// Whether some part of this selector, or of one inside it, relates siblings (`+`, `~`), so
    /// that an element's style can change with a sibling's state.
    pub(crate) fn relates_siblings(&self) -> bool {
        self.parts.iter().enumerate().any(|(index, part)| {
            index > 0 && matches!(part.relation, Relation::Next | Relation::Following)
                || part.tests.iter().any(|test| match test {
                    Test::Any(selectors, _) => selectors.iter().any(Selector::relates_siblings),
                    _ => false,
                })
        })
    }

    /// Whether this is `:root` alone, a rule for the root element whatever its kind.
    pub fn is_root(&self) -> bool {
        matches!(self.parts.as_slice(), [part] if matches!(part.tests.as_slice(), [Test::Root]))
    }

    /// Whether some compound of this selector, or of a selector in its `:is()` or `:where()`,
    /// names both the element `kind` and the class (`group.lcd`).
    pub fn styles(&self, kind: &str, class: &str) -> bool {
        self.parts.iter().any(|part| {
            let named = |test: &Test| matches!(test, Test::Kind(own) if own == kind);
            let classed = |test: &Test| matches!(test, Test::Class(own) if own == class);
            part.tests.iter().any(named) && part.tests.iter().any(classed)
                || part.tests.iter().any(|test| match test {
                    Test::Any(selectors, false) => selectors.iter().any(|selector| selector.styles(kind, class)),
                    _ => false,
                })
        })
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
                Test::Attribute(name, None) => element.attribute(name).is_some(),
                Test::Attribute(name, Some((operator, expected, fold))) => {
                    element.attribute(name).is_some_and(|actual| operator.matches(actual, expected, *fold))
                }
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

fn parse_list(parser: &mut Parser<'_>, depth: usize) -> Result<Vec<Selector>, String> {
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
                        attribute = Some(attribute_test(parser));
                        Ok::<(), cssparser::ParseError<()>>(())
                    })
                    .map_err(|e| format!("Attribute: {e:?}"))?;
                (attribute.ok_or("Missing attribute")??, Specificity(0, 1, 0))
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
                            Ok::<(), cssparser::ParseError<()>>(())
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

/// The inside of `[...]`: a name the host can set, then optionally an operator, a value and `i`.
fn attribute_test(parser: &mut Parser<'_>) -> Result<Test, String> {
    let name = parser.expect_ident_cloned().map_err(|_| "Expected an attribute name")?.to_ascii_lowercase();
    if !valid_attribute_name(&name) {
        return Err(format!("Invalid attribute name {name}"));
    }
    if parser.is_exhausted() {
        return Ok(Test::Attribute(name, None));
    }
    let operator = match parser.next().map_err(|_| "Expected an attribute operator")?.clone() {
        Token::Delim('=') => Operator::Equals,
        Token::IncludeMatch => Operator::Word,
        Token::DashMatch => Operator::Prefix,
        Token::PrefixMatch => Operator::Starts,
        Token::SuffixMatch => Operator::Ends,
        Token::SubstringMatch => Operator::Contains,
        _ => return Err("Unsupported attribute operator".into()),
    };
    let value = match parser.next().map_err(|_| "Expected an attribute value")?.clone() {
        Token::Ident(value) | Token::QuotedString(value) => value.to_string(),
        _ => return Err("Expected an attribute value".into()),
    };
    if value.len() > MAX_ATTRIBUTE_BYTES {
        return Err("Attribute value exceeds 256 bytes".into());
    }
    let fold = if parser.is_exhausted() {
        false
    } else {
        match parser.next().map_err(|_| "Unexpected attribute suffix")?.clone() {
            Token::Ident(flag) if flag.eq_ignore_ascii_case("i") => true,
            Token::Ident(flag) if flag.eq_ignore_ascii_case("s") => false,
            _ => return Err("Unexpected attribute suffix".into()),
        }
    };
    parser.expect_exhausted().map_err(|_| "Unexpected attribute suffix")?;
    Ok(Test::Attribute(name, Some((operator, value, fold))))
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
        for bad in ["[data-object=a b]", "[data-object=]", "[data-object==a]", "[-x]", "[data-object=a q]"] {
            assert!(Selector::parse_list(bad).is_err(), "{bad}");
        }
    }

    #[test]
    fn attribute_selectors_test_and_compare_the_host_s_attributes() {
        let tree = Tree::new(vec![
            Element::new("sample-map", None),
            Element::new("sample-point", Some(0)).with_attribute("kind", "kick").with_attribute("selected", ""),
            Element::new("sample-point", Some(0))
                .with_attribute("kind", "snare-top")
                .with_attribute("tags", "loud short"),
            Element::new("sample-point", Some(0)).with_attribute("kind", "Kick"),
        ])
        .unwrap();
        let matching = |source: &str| -> Vec<usize> {
            let selector = &Selector::parse_list(source).unwrap()[0];
            (0..4).filter(|node| selector.matches(&tree, *node, None)).collect()
        };
        assert_eq!(matching("[selected]"), [1]);
        assert_eq!(matching("sample-point[kind=\"kick\"]"), [1]);
        assert_eq!(matching("[kind=kick i]"), [1, 3]);
        assert_eq!(matching("[kind=kick s]"), [1]);
        assert_eq!(matching("[tags~=short]"), [2]);
        assert_eq!(matching("[kind|=snare]"), [2]);
        assert_eq!(matching("[kind^=sn]"), [2]);
        assert_eq!(matching("[kind$=top]"), [2]);
        assert_eq!(matching("[kind*=are]"), [2]);
        assert_eq!(matching("[kind^='']"), Vec::<usize>::new());
        assert_eq!(matching("[selected='']"), [1]);
        assert_eq!(matching("[KIND=kick]"), [1]);
        assert_eq!(matching(":not([kind])"), [0]);
        assert_eq!(
            Selector::parse_list("sample-point[kind=kick][selected]").unwrap()[0].specificity,
            Specificity(0, 2, 1)
        );
    }

    #[test]
    fn hostile_attributes_are_refused_by_the_tree() {
        for (name, value) in [("Color", "red"), ("", "red"), ("data-object", "a"), ("color", "a\u{7}")] {
            let element = Element::new("clip", None).with_attribute(name, value);
            assert!(Tree::new(vec![element]).is_err(), "{name}");
        }
        let long = Element::new("clip", None).with_attribute("color", &"a".repeat(MAX_ATTRIBUTE_BYTES + 1));
        assert!(Tree::new(vec![long]).is_err());
        let twice = Element::new("clip", None).with_attribute("color", "red").with_attribute("color", "blue");
        assert!(Tree::new(vec![twice]).is_err());
        let mut many = Element::new("clip", None);
        for index in 0..=MAX_ATTRIBUTES {
            many = many.with_attribute(&format!("a{index}"), "");
        }
        assert!(Tree::new(vec![many]).is_err());
    }

    #[test]
    fn invalid_tree_and_hostile_selectors_are_rejected() {
        assert!(Tree::new(vec![Element::new("knob", Some(0))]).is_err());
        for source in [
            "",
            "> knob",
            "knob >",
            "knob,,button",
            "knob:visitted",
            "[se:cret]",
            "knob::before",
            "knob:nth-child(NaN)",
        ] {
            assert!(Selector::parse_list(source).is_err(), "{source}");
        }
        assert!(Selector::parse_list(&format!("{}knob{}", ":not(".repeat(10), ")".repeat(10))).is_err());
    }
}
