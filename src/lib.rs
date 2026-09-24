#![forbid(unsafe_code)]

//! The Schematron content contract — a technology of `xmip-core-contract`.
//!
//! Two claims, decided 2026-09-07: **well-formedness is a given** — the Stream
//! parses as XML — and **conformance is a given once a contract is named**: a
//! Receive or Send Location that refers to this contract with a Schematron
//! bound has every Stream held to its rules.
//!
//! Schematron is what the XML business world validates *rules* with, over and
//! above structure: Peppol BIS and UBL e-invoicing, ISO 20022 payments, HL7 CDA.
//! A rule is an `XPath` context and a set of assertions, and a failed assertion
//! carries the human sentence its author wrote — which is what an operator sees.
//! [`rules`] documents the subset of ISO Schematron read.

pub mod rules;

use contract::{
    Contract, ContractDescriptor, ContractError, ContractFactory, ContractId, ValidationIssue,
    ValidationResult,
};
use rules::Rules;
use stream::Stream;

/// The Schematron contract, bare or bound to rules.
pub struct Schematron {
    descriptor: ContractDescriptor,
    rules: Option<Rules>,
}

impl Schematron {
    /// Well-formedness only.
    #[must_use]
    pub fn new() -> Self {
        Self {
            descriptor: descriptor("schematron"),
            rules: None,
        }
    }

    /// Well-formedness and the rules in the Schematron document `text`.
    ///
    /// # Errors
    /// The document must be well-formed, rooted at `schema` in an ISO
    /// Schematron namespace, and every `XPath` in it must compile.
    pub fn with_rules(text: &str) -> Result<Self, ContractError> {
        let rules = Rules::parse(text)?;
        let name = rules.title().unwrap_or("bound").to_string();
        Ok(Self {
            descriptor: descriptor(&format!("schematron:{name}")),
            rules: Some(rules),
        })
    }

    /// Whether rules are bound.
    #[must_use]
    pub fn is_bound(&self) -> bool {
        self.rules.is_some()
    }
}

impl Default for Schematron {
    fn default() -> Self {
        Self::new()
    }
}

fn descriptor(id: &str) -> ContractDescriptor {
    ContractDescriptor {
        id: ContractId(id.to_string()),
        version: "1".to_string(),
        representation: "application/xml".to_string(),
    }
}

impl Contract for Schematron {
    fn descriptor(&self) -> &ContractDescriptor {
        &self.descriptor
    }

    fn identify(&self, stream: &Stream) -> Result<bool, ContractError> {
        let essence = stream
            .media_type()
            .and_then(|m| m.split(';').next())
            .map_or("", str::trim);
        if essence.eq_ignore_ascii_case("application/xml")
            || essence.eq_ignore_ascii_case("text/xml")
            || essence.ends_with("+xml")
        {
            return Ok(true);
        }
        let first = stream
            .bytes()
            .iter()
            .copied()
            .find(|byte| !byte.is_ascii_whitespace());
        Ok(first == Some(b'<'))
    }

    fn validate(&self, stream: &Stream) -> Result<ValidationResult, ContractError> {
        let text = match std::str::from_utf8(stream.bytes()) {
            Ok(text) => text,
            Err(error) => return Ok(malformed(&format!("not UTF-8 text: {error}"))),
        };
        let package = match sxd_document::parser::parse(text) {
            Ok(package) => package,
            Err(error) => return Ok(malformed(&format!("not well-formed XML: {error}"))),
        };
        let issues = match &self.rules {
            Some(rules) => rules.check(&package.as_document())?,
            None => Vec::new(),
        };
        Ok(ValidationResult::of(issues))
    }
}

fn malformed(message: &str) -> ValidationResult {
    ValidationResult::of(vec![ValidationIssue::malformed(message)])
}

/// Loads the contract a Location names: an empty reference is the bare
/// contract, anything else is the path of a Schematron file.
pub struct SchematronFactory;

impl ContractFactory for SchematronFactory {
    fn technology(&self) -> &'static str {
        "schematron"
    }

    fn load(&self, reference: &str) -> Result<Box<dyn Contract>, ContractError> {
        if reference.trim().is_empty() {
            return Ok(Box::new(Schematron::new()));
        }
        let text = std::fs::read_to_string(reference).map_err(|error| ContractError {
            message: format!("cannot read rules {reference}: {error}"),
        })?;
        Ok(Box::new(Schematron::with_rules(&text)?))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use contract::fixture::stream;

    const INVOICE: &str = r#"<sch:schema xmlns:sch="http://purl.oclc.org/dsdl/schematron">
  <sch:title>Invoice rules</sch:title>
  <sch:ns prefix="inv" uri="urn:example:invoice"/>
  <sch:pattern id="totals">
    <sch:rule context="inv:Invoice">
      <sch:assert test="inv:Line" id="BR-01">An invoice has at least one line.</sch:assert>
      <sch:assert test="number(inv:Total) = sum(inv:Line/inv:Amount)" id="BR-02"
        >The total equals the sum of the lines.</sch:assert>
      <sch:report test="inv:Note and string-length(inv:Note) > 20"
        >A long note is unusual.</sch:report>
    </sch:rule>
    <sch:rule context="inv:Line">
      <sch:assert test="number(inv:Amount) >= 0">A line amount is not negative.</sch:assert>
    </sch:rule>
  </sch:pattern>
</sch:schema>"#;

    #[test]
    fn bare_contract_holds_well_formed_xml_only() {
        let bare = Schematron::new();
        assert!(
            bare.validate(&stream("<a><b/></a>"))
                .expect("validates")
                .valid
        );
        let broken = bare.validate(&stream("<a><b></a>")).expect("validates");
        assert_eq!(broken.issues[0].code, "malformed");
    }

    #[test]
    fn bound_contract_holds_a_conforming_invoice() {
        let bound = Schematron::with_rules(INVOICE).expect("rules");
        assert_eq!(bound.descriptor().id.0, "schematron:Invoice rules");
        let text = r#"<Invoice xmlns="urn:example:invoice"><Line><Amount>10</Amount></Line>
          <Line><Amount>5</Amount></Line><Total>15</Total></Invoice>"#;
        let held = bound.validate(&stream(text)).expect("validates");
        assert!(held.valid, "issues: {:?}", held.issues);
    }

    #[test]
    fn bound_contract_reports_each_failed_assertion_with_its_words() {
        let bound = Schematron::with_rules(INVOICE).expect("rules");
        let text = r#"<Invoice xmlns="urn:example:invoice"><Line><Amount>-1</Amount></Line>
          <Total>9</Total><Note>This note is definitely longer than twenty</Note></Invoice>"#;
        let held = bound.validate(&stream(text)).expect("validates");
        assert!(!held.valid);
        let seen: Vec<(&str, &str, Option<&str>)> = held
            .issues
            .iter()
            .map(|i| (i.code.as_str(), i.message.as_str(), i.path.as_deref()))
            .collect();
        assert!(
            seen.contains(&(
                "BR-02",
                "The total equals the sum of the lines.",
                Some("/Invoice")
            )),
            "{seen:?}"
        );
        assert!(
            seen.contains(&(
                "assert",
                "A line amount is not negative.",
                Some("/Invoice/Line")
            )),
            "{seen:?}"
        );
        assert!(
            seen.contains(&("report", "A long note is unusual.", Some("/Invoice"))),
            "{seen:?}"
        );
    }

    #[test]
    fn the_factory_loads_bare_and_bound() {
        let factory = SchematronFactory;
        assert_eq!(factory.technology(), "schematron");
        assert_eq!(
            factory.load("").expect("bare").descriptor().id.0,
            "schematron"
        );
        let dir = std::env::temp_dir().join("xmip-schematron-test");
        std::fs::create_dir_all(&dir).expect("temp dir");
        let file = dir.join("invoice.sch");
        std::fs::write(&file, INVOICE).expect("write rules");
        let bound = factory
            .load(file.to_str().expect("utf-8 path"))
            .expect("bound");
        assert_eq!(bound.descriptor().id.0, "schematron:Invoice rules");
        assert!(
            factory
                .load(dir.join("missing.sch").to_str().expect("path"))
                .is_err()
        );
    }
}
