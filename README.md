# xmip-core-contract-schematron

The Schematron content contract, a technology of
[xmip-core-contract](https://github.com/IlleNilsson/xmip-core-contract).

Two claims. **Well-formedness is a given**: the Stream parses as XML.
**Conformance is a given once the contract is named**: a Receive or Send
Location that refers to this contract with a Schematron document bound has
every Stream held to its rules, and each assertion that fires is reported with
the words its author wrote and the XPath of where it fired.

The claims are this crate's when it is called, and the Playground calls it.
A node holding a Location's Streams to it is
[decided, not built](../../../../../doc/architecture/estate-map.md#arrival-validation): a node refuses to start a
Location that names a contract until it does.

Schematron is how the XML business world states rules over and above structure:
Peppol BIS and UBL e-invoicing, ISO 20022 payments, HL7 CDA. `src/rules.rs`
lists the ISO Schematron subset read; a document outside it is refused when
bound, by name. XPath is 1.0. Every expression compiles when the document is
bound, so a bad one refuses it there, and once more on each thread on its first
check, because a compiled XPath cannot move between threads; every later
check on that thread reuses them.

## Toolchain

`rust-toolchain.toml` pins the toolchain for the whole estate. Do not change it
here.

## Verification

The included workflow is manual-only and calls the versioned shared workflow at
`IlleNilsson/.github@v1`.
