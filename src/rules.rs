//! The ISO Schematron subset this contract binds, and how a document is held
//! to it.
//!
//! Read: `title`, `ns` (prefix and uri, made available to every `XPath`),
//! `pattern` holding `rule` elements, each with a `context` `XPath` and any
//! number of `assert` and `report` children carrying a `test` `XPath`, an
//! optional `id` and the human text. Within one pattern a node is judged by the
//! first rule whose context selects it, as ISO 19757-3 says. Both the ISO
//! namespace and the older `ascc.net` one are accepted.
//!
//! Refused when bound, by name: `phase`, `let`, `include`, `extends`, abstract
//! patterns and `value-of`. `XPath` is 1.0, the engine's dialect; a `test` that
//! does not compile refuses the whole document, so an operator learns at
//! configuration time.
//!
//! An issue's `code` is the assertion's `id` when it has one, else `assert` or
//! `report`; its `message` is the assertion's own text; its `path` is where the
//! rule fired, `XPath`-style.

use contract::{ContractError, ValidationIssue};
use sxd_document::dom::{Document, Element};
use sxd_xpath::nodeset::Node;
use sxd_xpath::{Context, Factory, Value, XPath};

const ISO: &str = "http://purl.oclc.org/dsdl/schematron";
const OLD: &str = "http://www.ascc.net/xml/schematron";

/// One bound Schematron.
pub struct Rules {
    title: Option<String>,
    namespaces: Vec<(String, String)>,
    patterns: Vec<Pattern>,
}

struct Pattern {
    rules: Vec<Rule>,
}

/// `XPath` expressions are kept as text and compiled per check: a compiled
/// one is not `Send`, and a Contract is. They are compiled once at bind time
/// all the same, so a bad one refuses the document there.
struct Rule {
    context: String,
    assertions: Vec<Assertion>,
}

struct Assertion {
    /// `true` for `assert` (fires when the test is false), `false` for
    /// `report` (fires when the test is true).
    is_assert: bool,
    test: String,
    id: Option<String>,
    message: String,
}

/// A rule's `context` is a match pattern, judged against every node in the
/// document, not a path walked from the root: `item` means every item anywhere.
/// An absolute pattern is left as written.
fn selector(context: &str) -> String {
    if context.starts_with('/') {
        context.to_string()
    } else {
        format!("//{context}")
    }
}

fn build(factory: &Factory, expression: &str) -> Result<XPath, ContractError> {
    factory
        .build(expression)
        .map_err(|error| refuse(format!("XPath {expression:?}: {error}")))?
        .ok_or_else(|| refuse(format!("XPath {expression:?} is empty")))
}

impl Rules {
    /// Read a Schematron document.
    ///
    /// # Errors
    /// Not well-formed, not a Schematron, outside the subset, or an `XPath`
    /// that does not compile.
    pub fn parse(text: &str) -> Result<Self, ContractError> {
        let document = roxmltree::Document::parse(text).map_err(refuse)?;
        let root = document.root_element();
        if root.tag_name().name() != "schema" || !is_schematron(root) {
            return Err(refuse("the document root is not a Schematron schema"));
        }
        let factory = Factory::new();
        let compile = |expression: &str| -> Result<String, ContractError> {
            build(&factory, expression).map(|_| expression.to_string())
        };

        let mut rules = Self {
            title: None,
            namespaces: Vec::new(),
            patterns: Vec::new(),
        };
        for node in root.children().filter(roxmltree::Node::is_element) {
            match node.tag_name().name() {
                "title" => rules.title = node.text().map(|t| t.trim().to_string()),
                "ns" => {
                    let prefix = attribute(node, "prefix")?;
                    let uri = attribute(node, "uri")?;
                    rules.namespaces.push((prefix, uri));
                }
                "pattern" => {
                    if node.has_attribute("abstract") || node.has_attribute("is-a") {
                        return Err(refuse("abstract patterns are not supported"));
                    }
                    rules.patterns.push(Pattern::read(node, &compile)?);
                }
                "p" => {}
                other => return Err(refuse(format!("{other} is not supported"))),
            }
        }
        Ok(rules)
    }

    #[must_use]
    pub fn title(&self) -> Option<&str> {
        self.title.as_deref()
    }

    /// Every assertion that fires over `document`.
    ///
    /// # Errors
    /// An `XPath` that compiled but cannot be evaluated over this document.
    pub fn check(&self, document: &Document<'_>) -> Result<Vec<ValidationIssue>, ContractError> {
        let mut context = Context::new();
        for (prefix, uri) in &self.namespaces {
            context.set_namespace(prefix, uri);
        }
        let factory = Factory::new();
        let mut issues = Vec::new();
        let root: Node<'_> = document.root().into();
        for pattern in &self.patterns {
            let mut judged: Vec<Node<'_>> = Vec::new();
            for rule in &pattern.rules {
                let selector = build(&factory, &selector(&rule.context))?;
                let Value::Nodeset(selected) = selector
                    .evaluate(&context, root)
                    .map_err(|error| evaluation(&error))?
                else {
                    continue;
                };
                for node in selected.document_order() {
                    if judged.contains(&node) {
                        continue;
                    }
                    judged.push(node);
                    for assertion in &rule.assertions {
                        let held = build(&factory, &assertion.test)?
                            .evaluate(&context, node)
                            .map_err(|error| evaluation(&error))?
                            .boolean();
                        if held != assertion.is_assert {
                            issues.push(ValidationIssue {
                                code: assertion.id.clone().unwrap_or_else(|| {
                                    if assertion.is_assert {
                                        "assert"
                                    } else {
                                        "report"
                                    }
                                    .to_string()
                                }),
                                message: assertion.message.clone(),
                                path: Some(location(node)),
                            });
                        }
                    }
                }
            }
        }
        Ok(issues)
    }
}

impl Pattern {
    fn read(
        node: roxmltree::Node<'_, '_>,
        compile: &dyn Fn(&str) -> Result<String, ContractError>,
    ) -> Result<Self, ContractError> {
        let mut rules = Vec::new();
        for child in node.children().filter(roxmltree::Node::is_element) {
            match child.tag_name().name() {
                "rule" => {
                    if child.has_attribute("abstract") {
                        return Err(refuse("abstract rules are not supported"));
                    }
                    rules.push(Rule::read(child, compile)?);
                }
                "title" | "p" => {}
                other => return Err(refuse(format!("{other} inside a pattern is not supported"))),
            }
        }
        Ok(Self { rules })
    }
}

impl Rule {
    fn read(
        node: roxmltree::Node<'_, '_>,
        compile: &dyn Fn(&str) -> Result<String, ContractError>,
    ) -> Result<Self, ContractError> {
        let context = attribute(node, "context")?;
        compile(&selector(&context))?;
        let mut assertions = Vec::new();
        for child in node.children().filter(roxmltree::Node::is_element) {
            let is_assert = match child.tag_name().name() {
                "assert" => true,
                "report" => false,
                "p" => continue,
                other => return Err(refuse(format!("{other} inside a rule is not supported"))),
            };
            assertions.push(Assertion {
                is_assert,
                test: compile(&attribute(child, "test")?)?,
                id: child.attribute("id").map(str::to_string),
                message: child
                    .descendants()
                    .filter(roxmltree::Node::is_text)
                    .filter_map(|n| n.text())
                    .collect::<String>()
                    .split_whitespace()
                    .collect::<Vec<_>>()
                    .join(" "),
            });
        }
        Ok(Self {
            context,
            assertions,
        })
    }
}

fn is_schematron(node: roxmltree::Node<'_, '_>) -> bool {
    matches!(node.tag_name().namespace(), Some(ISO | OLD))
}

fn attribute(node: roxmltree::Node<'_, '_>, name: &str) -> Result<String, ContractError> {
    node.attribute(name)
        .map(str::to_string)
        .ok_or_else(|| refuse(format!("{} without {name}", node.tag_name().name())))
}

/// Where a node is, as an `XPath` an operator can paste: `/Invoice/Line[2]`.
fn location(node: Node<'_>) -> String {
    let mut steps = Vec::new();
    let mut current = Some(node);
    while let Some(here) = current {
        match here {
            Node::Element(element) => steps.push(step(element)),
            Node::Attribute(attribute) => {
                steps.push(format!("@{}", attribute.name().local_part()));
            }
            Node::Root(_) => break,
            _ => {}
        }
        current = here.parent();
    }
    steps.reverse();
    format!("/{}", steps.join("/"))
}

fn step(element: Element<'_>) -> String {
    let name = element.name().local_part();
    let Some(parent) = element
        .parent()
        .and_then(sxd_document::dom::ParentOfChild::element)
    else {
        return name.to_string();
    };
    let siblings: Vec<Element<'_>> = parent
        .children()
        .into_iter()
        .filter_map(sxd_document::dom::ChildOfElement::element)
        .filter(|e| e.name().local_part() == name)
        .collect();
    if siblings.len() == 1 {
        return name.to_string();
    }
    let ordinal = siblings
        .iter()
        .position(|e| *e == element)
        .map_or(1, |i| i + 1);
    format!("{name}[{ordinal}]")
}

fn refuse(reason: impl std::fmt::Display) -> ContractError {
    ContractError {
        message: format!("rules refused: {reason}"),
    }
}

fn evaluation(error: &sxd_xpath::ExecutionError) -> ContractError {
    ContractError {
        message: format!("rule evaluation failed: {error}"),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_rule_outside_the_subset_is_refused_by_name() {
        let text = r#"<schema xmlns="http://purl.oclc.org/dsdl/schematron">
  <let name="x" value="1"/>
  <pattern><rule context="/"><assert test="true()">ok</assert></rule></pattern>
</schema>"#;
        let error = Rules::parse(text).err().expect("refused");
        assert!(error.message.contains("let"), "{}", error.message);
    }

    #[test]
    fn an_xpath_that_does_not_compile_refuses_the_document() {
        let text = r#"<schema xmlns="http://purl.oclc.org/dsdl/schematron">
  <pattern><rule context="/"><assert test="count(">bad</assert></rule></pattern>
</schema>"#;
        let error = Rules::parse(text).err().expect("refused");
        assert!(error.message.contains("XPath"), "{}", error.message);
    }

    #[test]
    fn the_first_matching_rule_in_a_pattern_judges_a_node() {
        let text = r#"<schema xmlns="http://purl.oclc.org/dsdl/schematron">
  <pattern>
    <rule context="item[@kind='a']"><assert test="false()">first</assert></rule>
    <rule context="item"><assert test="false()">second</assert></rule>
  </pattern>
</schema>"#;
        let rules = Rules::parse(text).expect("parses");
        let package = sxd_document::parser::parse("<r><item kind='a'/><item/></r>").expect("xml");
        let issues = rules.check(&package.as_document()).expect("checks");
        let fired: Vec<(&str, &str)> = issues
            .iter()
            .map(|i| (i.message.as_str(), i.path.as_deref().unwrap_or("")))
            .collect();
        assert_eq!(fired, [("first", "/r/item[1]"), ("second", "/r/item[2]")]);
    }

    #[test]
    fn a_document_that_is_not_schematron_is_refused() {
        assert!(Rules::parse("<schema/>").is_err());
        assert!(Rules::parse("<broken").is_err());
    }
}
