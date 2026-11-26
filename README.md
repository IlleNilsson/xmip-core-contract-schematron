# xmip-core-contract-schematron

The Schematron content contract, a technology of
[xmip-core-contract](https://github.com/IlleNilsson/xmip-core-contract).

Two claims. **Well-formedness is a given**: the Stream parses as XML.
**Conformance is a given once the contract is named**: a Receive or Send
Location that refers to this contract with a Schematron document bound has
every Stream held to its rules, and each assertion that fires is reported with
the words its author wrote and the XPath of where it fired.

The claims are this crate's when it is called, and the System Tests call it.
A node holding a Location's Streams to it is
[decided, not built](../../../../../doc/architecture/estate-map.md#arrival-validation): a node refuses to start a
Location that names a contract until it does.

Schematron is how the XML business world states rules over and above structure:
Peppol BIS and UBL e-invoicing, ISO 20022 payments, HL7 CDA. `src/rules.rs`
lists the ISO Schematron subset read; a document outside it is refused when
bound, by name. The Schematron document is read by the estate's one XML
reader (`xmip-core-library-codec`), and so is a document the bare contract
holds to well-formedness. XPath is 1.0, the estate's one XPath, the `xpath`
path technology (`xmip-core-path-xpath`): every expression compiles when the
document is bound, so a bad one, or one using a prefix no `ns` binds, refuses
it there, and is built once more on each thread on its first check, because a
compiled XPath cannot move between threads; every later check on that thread
reuses it.

## The declaration

What a Location naming this contract is configured with is two `const`s in
its `declaration` module, `src/declaration.rs`, beside the code, which its
`ContractFactory` reads (ADR-0064, amendment 2026-11-26):
`declaration::SETTINGS`, `reference`, the path of the Schematron rules
documents are held to, and `NAME`, `schematron`, the name a Validate step's
`format` names it by. Built with its `declaration` feature alone
(`--no-default-features --features declaration`), the crate is those and
nothing more: its code and every crate the code is built on are behind the
default feature. So the library the surfaces load can carry it without the
code, for the language server and the desktop to suggest and check a
Location's contract settings though no node in their process loaded the
technology, once the gathering crate of open problem 48 links it.
`test/Declaration.Test.ps1` in the estate holds that the declaration builds
nothing the contract capability does not.

## Toolchain

`rust-toolchain.toml` pins the toolchain for the whole estate. Do not change it
here.

## Verification

The included workflow is manual-only and calls the versioned shared workflow at
`IlleNilsson/.github@v1`.
