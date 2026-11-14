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
//! patterns and `value-of`. `XPath` is 1.0, evaluated by the `xpath` path
//! technology (`xmip-core-path-xpath`), the estate's one `XPath`; a `test`
//! that does not compile, or uses a prefix no `ns` binds, refuses the whole
//! document, so an operator learns at configuration time. The Schematron
//! document is read by the estate's one XML reader (`codec::xml`).
//!
//! An issue's `code` is the assertion's `id` when it has one, else `assert` or
//! `report`; its `message` is the assertion's own text; its `path` is where the
//! rule fired, `XPath`-style.

use std::collections::HashSet;

use codec::xml::tree::{Document as Tree, Node};
use contract::{ContractError, ValidationIssue};
use xpath::{Document, Expression, Namespaces, Selected};

const ISO: &str = "http://purl.oclc.org/dsdl/schematron";
const OLD: &str = "http://www.ascc.net/xml/schematron";

/// One bound Schematron.
pub struct Rules {
    title: Option<String>,
    patterns: Vec<Pattern>,
}

struct Pattern {
    rules: Vec<Rule>,
}

/// Each expression is compiled as the document is bound, so a bad one
/// refuses the document there; the `xpath` technology builds it once per
/// thread that checks, and every later check on that thread reuses it.
struct Rule {
    selector: Expression,
    assertions: Vec<Assertion>,
}

struct Assertion {
    /// `true` for `assert` (fires when the test is false), `false` for
    /// `report` (fires when the test is true).
    is_assert: bool,
    test: Expression,
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

impl Rules {
    /// Read a Schematron document.
    ///
    /// # Errors
    /// Not well-formed, not a Schematron, outside the subset, or an `XPath`
    /// that does not compile.
    pub fn parse(text: &str) -> Result<Self, ContractError> {
        let document = Tree::parse(text).map_err(refuse)?;
        let root = document.root_element();
        if root.local() != "schema" || !is_schematron(root) {
            return Err(refuse("the document root is not a Schematron schema"));
        }
        let mut namespaces = Namespaces::new();
        for node in root.elements().filter(|node| node.local() == "ns") {
            namespaces = namespaces.bind(attribute(node, "prefix")?, attribute(node, "uri")?);
        }
        let compile = |text: &str| Expression::compile(text, &namespaces).map_err(refuse);
        let mut rules = Self {
            title: None,
            patterns: Vec::new(),
        };
        for node in root.elements() {
            match node.local() {
                "title" => rules.title = node.text().map(|t| t.trim().to_string()),
                "pattern" => {
                    if node.has_attribute("abstract") || node.has_attribute("is-a") {
                        return Err(refuse("abstract patterns are not supported"));
                    }
                    rules.patterns.push(Pattern::read(node, &compile)?);
                }
                "ns" | "p" => {}
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
    pub fn check(&self, document: &Document) -> Result<Vec<ValidationIssue>, ContractError> {
        let mut issues = Vec::new();
        for pattern in &self.patterns {
            // A node is judged by the first rule of the pattern that selects
            // it. A node hashes and compares by its address in the document,
            // so the interior mutability the lint sees never moves a key.
            #[allow(clippy::mutable_key_type)]
            let mut judged: HashSet<Selected<'_>> = HashSet::new();
            for rule in &pattern.rules {
                let selector = rule.selector.ready().map_err(evaluation)?;
                let tests = rule
                    .assertions
                    .iter()
                    .map(|assertion| assertion.test.ready())
                    .collect::<Result<Vec<_>, _>>()
                    .map_err(evaluation)?;
                for node in selector.select(document).map_err(evaluation)? {
                    if !judged.insert(node) {
                        continue;
                    }
                    for (assertion, test) in rule.assertions.iter().zip(&tests) {
                        if test.holds(node).map_err(evaluation)? != assertion.is_assert {
                            issues.push(assertion.issue(node));
                        }
                    }
                }
            }
        }
        Ok(issues)
    }
}

impl Assertion {
    /// The issue this assertion raises where it fired: coded by its `id`,
    /// else by what it is.
    fn issue(&self, node: Selected<'_>) -> ValidationIssue {
        let code = match &self.id {
            Some(id) => id.clone().into(),
            None if self.is_assert => "assert".into(),
            None => "report".into(),
        };
        ValidationIssue {
            code,
            message: self.message.clone(),
            path: Some(node.location()),
        }
    }
}

/// How an expression is compiled with the document's namespaces.
type Compile<'a> = dyn Fn(&str) -> Result<Expression, ContractError> + 'a;

impl Pattern {
    fn read(node: Node<'_>, compile: &Compile<'_>) -> Result<Self, ContractError> {
        let mut rules = Vec::new();
        for child in node.elements() {
            match child.local() {
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
    fn read(node: Node<'_>, compile: &Compile<'_>) -> Result<Self, ContractError> {
        let selector = compile(&selector(&attribute(node, "context")?))?;
        let mut assertions = Vec::new();
        for child in node.elements() {
            let is_assert = match child.local() {
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
                    .filter(|descendant| descendant.is_text())
                    .filter_map(Node::text)
                    .collect::<String>()
                    .split_whitespace()
                    .collect::<Vec<_>>()
                    .join(" "),
            });
        }
        Ok(Self {
            selector,
            assertions,
        })
    }
}

fn is_schematron(node: Node<'_>) -> bool {
    matches!(node.namespace(), Some(ISO | OLD))
}

fn attribute(node: Node<'_>, name: &str) -> Result<String, ContractError> {
    node.attribute(name)
        .map(str::to_string)
        .ok_or_else(|| refuse(format!("{} without {name}", node.local())))
}

fn refuse(reason: impl std::fmt::Display) -> ContractError {
    ContractError {
        message: format!("rules refused: {reason}"),
    }
}

fn evaluation(mut error: ContractError) -> ContractError {
    error.message.insert_str(0, "rule evaluation failed: ");
    error
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
    fn a_prefix_no_ns_binds_refuses_the_document() {
        let text = r#"<schema xmlns="http://purl.oclc.org/dsdl/schematron">
  <pattern><rule context="inv:Invoice"><assert test="true()">ok</assert></rule></pattern>
</schema>"#;
        let error = Rules::parse(text).err().expect("refused");
        assert!(error.message.contains("prefix inv"), "{}", error.message);
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
        let document = Document::parse("<r><item kind='a'/><item/></r>").expect("xml");
        let issues = rules.check(&document).expect("checks");
        let fired: Vec<(&str, &str)> = issues
            .iter()
            .map(|i| (i.message.as_str(), i.path.as_deref().unwrap_or("")))
            .collect();
        assert_eq!(fired, [("first", "/r/item[1]"), ("second", "/r/item[2]")]);
    }

    #[test]
    fn one_set_of_rules_checks_many_documents_and_nodes_alike() {
        let text = r#"<schema xmlns="http://purl.oclc.org/dsdl/schematron">
  <pattern><rule context="item"><assert test="@id">an id</assert></rule></pattern>
</schema>"#;
        let rules = Rules::parse(text).expect("parses");
        let items = "<item id='1'/>".repeat(200);
        let document = Document::parse(&format!("<r>{items}</r>")).expect("xml");
        for _ in 0..100 {
            assert_eq!(
                rules.check(&document).expect("checks"),
                [] as [ValidationIssue; 0]
            );
        }
        let missing = Document::parse("<r><item/><item id='2'/></r>").expect("xml");
        let fired = rules.check(&missing).expect("checks");
        assert_eq!(fired.len(), 1);
        assert_eq!(fired[0].path.as_deref(), Some("/r/item[1]"));
    }

    #[test]
    fn a_document_that_is_not_schematron_is_refused() {
        assert!(Rules::parse("<schema/>").is_err());
        assert!(Rules::parse("<broken").is_err());
    }
}
