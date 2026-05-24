//! Parsed test-source domain types — the data shape every adapter
//! crate produces and every detector consumes.
//!
//! ## Invariants
//!
//! 1. **POD-only**. No lifetimes, no `&'a` references, no `Cow<'_, T>`.
//!    Every field is owned data so the v0.6+ scrap4ts FFI layer can
//!    re-export these without lifetime gymnastics. Additional napi-rs
//!    derive/wrapping lands with the scrap4ts adapter; this crate stays
//!    FFI-agnostic.
//! 2. **Semantic Facts pattern** — *adapter says what, core says is-bad*.
//!    Detector PRs add typed flags (`is_tautological`, `behavioral_facts:
//!    Vec<BehavioralFact>`, etc.) onto these structs via the constructor
//!    pattern (additive; no envelope schema bump). The adapter
//!    pre-computes the language-specific classification; detectors read
//!    the fact and emit a `Finding`. AST shape never crosses the port
//!    boundary.
//! 3. **`#[non_exhaustive]` on enums only** (per
//!    [`adr-nested-json-envelope`](https://github.com/breezy-bays-labs/ops/blob/main/decisions/scrap4rs/adr-nested-json-envelope.md)).
//!    Result structs evolve via constructor pattern + serde versioning.
//! 4. **No raw body source carried** — structural facts only. Body-line
//!    count is enough for `large-example`; future detectors receive
//!    typed fact fields, not the raw `String`.

use crate::domain::assertion_sources::AssertionSource;
use crate::domain::opt_outs::OptOut;
use crate::domain::types::{FilePath, Span, TestIdentity};
use serde::{Deserialize, Serialize};
use std::collections::BTreeSet;

/// Adapter output for a single source file — the test-suite-shaped
/// data every detector consumes.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ParsedTestFile {
    /// Source file this projection was parsed from.
    pub path: FilePath,
    /// Tests recovered from the file.
    pub tests: Vec<ParsedTest>,
    /// Per-file **non-fatal** parse-time observations. Empty for clean
    /// syn parses; reserved for future TS adapters whose parsers (oxc,
    /// swc) recover from partial syntax errors. A non-empty
    /// `diagnostics` does NOT permit silently truncating `tests` —
    /// either every discoverable test appears in `tests`, or the
    /// adapter returns `Err(ParseError::Syntax)`. See [`ParseDiagnostic`]
    /// and the `Result::Err vs ParseDiagnostic contract` in
    /// [`super::super::ports::parser`].
    pub diagnostics: Vec<ParseDiagnostic>,
}

impl ParsedTestFile {
    /// Canonical constructor. Detector PRs that add semantic-fact fields
    /// extend this signature additively (per the Semantic Facts pattern
    /// — see module-level invariants).
    #[must_use]
    pub fn new(path: FilePath, tests: Vec<ParsedTest>, diagnostics: Vec<ParseDiagnostic>) -> Self {
        Self {
            path,
            tests,
            diagnostics,
        }
    }
}

/// One test (Rust `#[test]` fn or future TS `test()` callsite),
/// projected to language-agnostic structural facts.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ParsedTest {
    /// File + qualified name + span.
    pub identity: TestIdentity,
    /// Test-attribute observations (`#[ignore]`, `#[should_panic]`,
    /// future `test.skip` etc.) as a name+args bag.
    pub attributes: Vec<ParsedAttribute>,
    /// Assertions detected in the body. Minimal v0.1 shape; detector
    /// PRs add typed semantic-fact fields via constructor.
    pub assertions: Vec<ParsedAssertion>,
    /// Source-line count of the **body block** (interior of the fn's
    /// `{ ... }`). Distinct from `identity.span.line_count()`, which
    /// covers `fn name(...) { ... }` from the signature to the closing
    /// brace; the two can differ by 1–N lines depending on signature
    /// formatting. Drives `large-example`.
    pub body_line_count: u32,
    /// Implicit-assertion sources recognized in the body (proptest,
    /// quickcheck, cucumber `.await` chain terminal, kani, trybuild,
    /// insta, `pretty_assertions`) OR on the test fn's attribute list
    /// (`#[should_panic]`). The adapter populates this via
    /// [`crate::domain::assertion_sources::recognise`] while walking
    /// the body, plus a sibling helper for attribute-sourced variants
    /// (`scrap4rs::parser::attributes::implicit_sources_from_attributes`).
    ///
    /// Emission order is the parser's natural body-walk order — useful
    /// for debugging; `Vec` (not `BTreeSet`) preserves it.
    ///
    /// Detectors in `scrap-core::detectors/` (lands at scrap-rs#19/#30)
    /// read this field; the `zero-assertion` detector skips emission
    /// when non-empty.
    pub implicit_assertion_sources: Vec<AssertionSource>,
    /// Per-test detector suppressions, projected from
    /// `#[allow(scrap::*)]` attributes on the test fn. `BTreeSet` for
    /// deterministic serialization order — see
    /// [`crate::domain::opt_outs::OptOut`].
    pub opt_outs: BTreeSet<OptOut>,
}

impl ParsedTest {
    /// Canonical constructor. Detector PRs that add semantic-fact fields
    /// extend this signature additively.
    #[must_use]
    pub fn new(
        identity: TestIdentity,
        attributes: Vec<ParsedAttribute>,
        assertions: Vec<ParsedAssertion>,
        body_line_count: u32,
        implicit_assertion_sources: Vec<AssertionSource>,
        opt_outs: BTreeSet<OptOut>,
    ) -> Self {
        Self {
            identity,
            attributes,
            assertions,
            body_line_count,
            implicit_assertion_sources,
            opt_outs,
        }
    }
}

/// Test-level attribute, language-agnostic. Stored as a name+raw-args
/// bag so the same shape absorbs Rust `#[ignore = "..."]` and TS
/// `test.skip("...")` without per-language enum variants.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ParsedAttribute {
    /// Attribute name (e.g. `"ignore"`, `"should_panic"`, `"skip"`),
    /// unqualified.
    pub name: String,
    /// Raw argument text **as it appears in source bytes**, including
    /// any surrounding quotes (e.g. `"\"flaky\""` for `#[ignore =
    /// "flaky"]`). `None` only when the attribute carried no arguments
    /// at all (e.g. bare `#[ignore]`); attributes with empty parens
    /// (`#[ignore()]`) use `Some("".into())`.
    pub raw: Option<String>,
}

impl ParsedAttribute {
    /// Canonical constructor.
    #[must_use]
    pub fn new(name: impl Into<String>, raw: Option<String>) -> Self {
        Self {
            name: name.into(),
            raw,
        }
    }
}

/// One assertion call recovered by the parser. Minimal v0.1 shape;
/// detector PRs add typed semantic-fact fields onto this struct via the
/// constructor pattern.
///
/// **No `schema_version` bump for the v0.1 `kind → name` rename:**
/// `ParsedAssertion` is NOT part of the truthful-gate wire envelope
/// (see `crates/scrap-core/tests/wire_envelope_snapshot.rs` —
/// constructs only `Report` / `Finding` / `Smell` / `Summary` /
/// `Distribution` / `TestIdentity`). The rename does not propagate
/// to the gated wire shape; `adr-nested-json-envelope` requires no
/// bump. Pre-v1.0, this PR has no external consumers.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ParsedAssertion {
    /// **Leaf segment** of the macro path (e.g. `"assert_eq"` for both
    /// `assert_eq!(...)` and `pretty_assertions::assert_eq!(...)`).
    /// Consistent with [`ParsedAttribute::name`]; the adapter strips
    /// the namespace at the boundary so detector logic doesn't have to.
    pub name: String,
    /// Verbatim argument text as it appears in source bytes (the
    /// macro's token-stream contents, e.g. `"1, 1"` for
    /// `assert_eq!(1, 1)`). `None` for empty macros (`assert!()`);
    /// `Some("")` would only appear if a macro accepted whitespace-only
    /// tokens, which the v0.1 set does not.
    ///
    /// Detectors use this for tautology detection (`assert_eq!(x, x)`
    /// vs `assert_eq!(x, 1)`) etc. — the parser does NOT classify;
    /// it carries the raw text so the detector can.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub raw_args: Option<String>,
    /// Source span of the assertion call.
    pub span: Span,
}

impl ParsedAssertion {
    /// Canonical constructor. Detector PRs that add semantic-fact fields
    /// (e.g. `is_tautological`, `arguments_identical`) extend this
    /// signature additively.
    #[must_use]
    pub fn new(name: impl Into<String>, raw_args: Option<String>, span: Span) -> Self {
        Self {
            name: name.into(),
            raw_args,
            span,
        }
    }
}

/// One parse-time observation — non-fatal warnings or partial-recovery
/// diagnostics from the adapter parser. I/O failures NEVER appear here:
/// they are owned by the wrapper that opens the file (see
/// `ports::parser` module docs).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ParseDiagnostic {
    /// What kind of diagnostic this is.
    pub kind: ParseDiagnosticKind,
    /// Source span if the diagnostic localizes; `None` means the
    /// observation applies to the whole file (or position is
    /// unrecoverable — adapters that genuinely cannot recover a
    /// position MUST note this in `message`).
    pub span: Option<Span>,
    /// Human-readable detail string.
    pub message: String,
}

impl ParseDiagnostic {
    /// Canonical constructor.
    #[must_use]
    pub fn new(kind: ParseDiagnosticKind, span: Option<Span>, message: impl Into<String>) -> Self {
        Self {
            kind,
            span,
            message: message.into(),
        }
    }
}

/// Kind of parse-time diagnostic. `#[non_exhaustive]` so future TS
/// adapters can add `RecoveredSyntax`, `MalformedAttribute`, etc.,
/// without an envelope schema bump. Wire format is `snake_case`,
/// matching the rest of `domain/`. Per-variant `#[serde(rename)]` is
/// belt-and-suspenders against future serde-version drift (see
/// `classification.rs` module docs for rationale).
///
/// No catch-all `Other` variant: `#[non_exhaustive]` already provides
/// the forward-compat hatch — adapters that hit an unknown case file a
/// PR adding the right typed variant rather than dumping detail into a
/// stringly-typed message.
#[non_exhaustive]
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ParseDiagnosticKind {
    /// A syntax error the parser surfaced via partial recovery.
    #[serde(rename = "syntax")]
    Syntax,
    /// An attribute the parser saw but does not yet understand.
    #[serde(rename = "unsupported_attribute")]
    UnsupportedAttribute,
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::domain::types::QualifiedName;
    use proptest::prelude::*;

    fn sample_test() -> ParsedTest {
        ParsedTest::new(
            TestIdentity::new(
                FilePath::new("a.rs"),
                QualifiedName::new("a::tests::t"),
                Span::new(1, 4),
            ),
            vec![ParsedAttribute::new("ignore", Some("\"flaky\"".into()))],
            vec![ParsedAssertion::new(
                "assert_eq",
                Some("1, 1".into()),
                Span::new(2, 2),
            )],
            3,
            Vec::new(),
            BTreeSet::new(),
        )
    }

    fn sample_file() -> ParsedTestFile {
        ParsedTestFile::new(
            FilePath::new("a.rs"),
            vec![sample_test()],
            vec![ParseDiagnostic::new(
                ParseDiagnosticKind::UnsupportedAttribute,
                Some(Span::new(1, 1)),
                "unrecognized attribute foo",
            )],
        )
    }

    #[test]
    fn parsed_test_file_serde_round_trips() {
        let file = sample_file();
        let json = serde_json::to_string(&file).unwrap();
        let back: ParsedTestFile = serde_json::from_str(&json).unwrap();
        assert_eq!(file, back);
    }

    #[test]
    fn parse_diagnostic_kind_serializes_snake_case() {
        for (variant, wire) in [
            (ParseDiagnosticKind::Syntax, "syntax"),
            (
                ParseDiagnosticKind::UnsupportedAttribute,
                "unsupported_attribute",
            ),
        ] {
            let json = serde_json::to_value(variant).unwrap();
            assert_eq!(json, serde_json::Value::String(wire.into()));
            let back: ParseDiagnosticKind = serde_json::from_value(json).unwrap();
            assert_eq!(back, variant);
        }
    }

    // Wire-key pins. Round-trip tests are symmetric — a `#[serde(rename)]`
    // slipped onto a field would round-trip fine while silently breaking
    // the napi-rs FFI consumer (D8). These pin the JSON top-level keys so
    // a rename or `rename_all` change is caught at test time.

    #[test]
    fn parsed_test_file_wire_keys() {
        let json = serde_json::to_value(sample_file()).unwrap();
        for key in ["path", "tests", "diagnostics"] {
            assert!(json.get(key).is_some(), "missing wire key: {key}");
        }
    }

    #[test]
    fn parsed_test_wire_keys() {
        let json = serde_json::to_value(sample_test()).unwrap();
        for key in [
            "identity",
            "attributes",
            "assertions",
            "body_line_count",
            "implicit_assertion_sources",
            "opt_outs",
        ] {
            assert!(json.get(key).is_some(), "missing wire key: {key}");
        }
    }

    #[test]
    fn parsed_attribute_wire_keys() {
        let json =
            serde_json::to_value(ParsedAttribute::new("ignore", Some("\"flaky\"".into()))).unwrap();
        for key in ["name", "raw"] {
            assert!(json.get(key).is_some(), "missing wire key: {key}");
        }
    }

    #[test]
    fn parsed_assertion_wire_keys() {
        // Always-present keys (raw_args is conditional via
        // skip_serializing_if; tested separately below).
        let json =
            serde_json::to_value(ParsedAssertion::new("assert_eq", None, Span::new(2, 2))).unwrap();
        for key in ["name", "span"] {
            assert!(json.get(key).is_some(), "missing wire key: {key}");
        }
        // `raw_args` MUST NOT appear when None — preserves bytewise
        // round-trip identity with consumers that compiled without
        // the optional field.
        assert!(
            json.get("raw_args").is_none(),
            "raw_args should be omitted when None (skip_serializing_if)",
        );
    }

    #[test]
    fn parsed_assertion_raw_args_present_when_some() {
        let json = serde_json::to_value(ParsedAssertion::new(
            "assert_eq",
            Some("1, 1".into()),
            Span::new(2, 2),
        ))
        .unwrap();
        assert_eq!(
            json.get("raw_args"),
            Some(&serde_json::Value::String("1, 1".into())),
        );
    }

    #[test]
    fn parse_diagnostic_wire_keys() {
        let json = serde_json::to_value(ParseDiagnostic::new(
            ParseDiagnosticKind::Syntax,
            Some(Span::new(1, 1)),
            "msg",
        ))
        .unwrap();
        for key in ["kind", "span", "message"] {
            assert!(json.get(key).is_some(), "missing wire key: {key}");
        }
    }

    proptest! {
        #[test]
        fn parsed_attribute_round_trip(
            name in "[a-z][a-z_]{0,15}",
            raw in proptest::option::of("\"[^\"]{0,30}\""),
        ) {
            let attr = ParsedAttribute::new(name.clone(), raw.clone());
            let back: ParsedAttribute =
                serde_json::from_str(&serde_json::to_string(&attr).unwrap()).unwrap();
            prop_assert_eq!(back.name, name);
            prop_assert_eq!(back.raw, raw);
        }
    }
}
