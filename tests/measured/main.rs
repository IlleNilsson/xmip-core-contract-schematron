//! The module's timing program (`doc/governance/rust-style.md` section
//! 5c): Schematron rules held to a time. A plain `cargo test` leaves it
//! out; the landing runs it after every other test of the module, alone,
//! one test at a time (`cargo test --test measured --all-features --
//! --test-threads=1`), so what is measured is Xmip's work and not its
//! neighbors':
//!
//! - rules compiled once a thread check a document of 200 nodes in under
//!   five milliseconds.

use contract::ValidationIssue;
use xmip_core_contract_schematron::rules::Rules;

#[test]
fn a_document_of_two_hundred_nodes_is_checked_in_under_five_milliseconds() {
    let text = r#"<schema xmlns="http://purl.oclc.org/dsdl/schematron">
  <pattern><rule context="item"><assert test="@id">an id</assert></rule></pattern>
</schema>"#;
    let rules = Rules::parse(text).expect("parses");
    let items = "<item id='1'/>".repeat(200);
    let package = sxd_document::parser::parse(&format!("<r>{items}</r>")).expect("xml");
    let document = package.as_document();
    let started = std::time::Instant::now();
    for _ in 0..100 {
        assert_eq!(
            rules.check(&document).expect("checks"),
            [] as [ValidationIssue; 0]
        );
    }
    let each = started.elapsed() / 100;
    assert!(
        each < std::time::Duration::from_millis(5),
        "{each:?} a document"
    );
}
