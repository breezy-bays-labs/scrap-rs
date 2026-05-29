//! `tautological-assertion` detector (scrap-rs#24).
//!
//! Flags assertions whose form guarantees they cannot fail and thus
//! carry no information:
//!
//! - `assert!(true)` — single-arg `Bool(true)` literal.
//! - `assert_eq!(x, x)` / `assert_ne!(x, x)` — token-identical
//!   two-argument shape.
//! - Literal-vs-literal compare (`assert_eq!(1, 1)`) — same
//!   token-identical mechanism.
//!
//! `assert!(false)` is deliberately NOT flagged — Uncle Bob's
//! convention ([`unclebob/scrap`](https://github.com/unclebob/scrap))
//! treats deliberate-failure assertions as informational, not smell.
//!
//! ## Semantic Facts pattern
//!
//! Per the `feedback_semantic-facts-vs-statement-projection` memory
//! and the [`adr-hexagonal-layout`](https://github.com/breezy-bays-labs/ops/blob/main/decisions/scrap-rs/adr-hexagonal-layout.md)
//! ADR, the adapter answers "what is happening?" (the typed
//! [`crate::domain::parsed::ParsedAssertion`] fields
//! `arguments_identical` and `single_arg_value`) and this core
//! detector answers "is this bad?" (the policy: emit a
//! [`SmellCategory::TautologicalAssertion`] smell with penalty 10).
//! AST shape never crosses the port boundary.
//!
//! TODO(scrap-rs#73): when `adr-port-surface-and-domain-conventions`
//! lands, link D8 (POD-only domain) and D10 (Semantic Facts
//! constructor extension) here.
//!
//! ## Explicit non-responsibilities
//!
//! The detector emits unconditionally when the facts indicate
//! tautology. It does NOT:
//!
//! - Consult [`crate::domain::parsed::ParsedTest::opt_outs`] —
//!   per-test `#[allow(scrap::tautology)]` suppression lives in the
//!   pipeline driver (scrap-rs#72; Christopher's locked Option B at
//!   /shape gate).
//! - Consult [`crate::domain::parsed::ParsedTest::implicit_assertion_sources`]
//!   for `#[should_panic]` suppression — same; the pipeline routes
//!   findings on `should_panic`-attributed tests through the policy
//!   layer.
//! - Consult any `DetectorConfig` for `enabled` / `penalty`
//!   overrides — that's also pipeline-driver territory; the detector
//!   ships v0.1 defaults.
//!
//! The pipeline driver (scrap-rs#72) calls `detect`, receives
//! `Option<Finding>`, and applies the project's `[opt_outs]` policy
//! plus the configured Skip/Advisory mode before the finding lands on
//! the wire envelope.

use crate::domain::classification::{Actionability, Severity};
use crate::domain::finding::Finding;
use crate::domain::literal_value::LiteralValue;
use crate::domain::parsed::{ParsedAssertion, ParsedTest};
use crate::domain::smell::{Smell, SmellCategory};

/// Penalty contribution per tautological-assertion smell. Pinned at
/// v0.1 per the kickstart-plan detection table; tunable via
/// `[detectors.tautological_assertion]` in `scrap4rs.toml` once the
/// pipeline driver consumes
/// [`crate::cli::config::DetectorConfig`] (scrap-rs#21).
pub const PENALTY: u32 = 10;

/// Detector entry point. Returns `Some(Finding)` when one or more
/// assertions on the test trip the tautology rule
/// (`arguments_identical` OR `single_arg_value == Some(Bool(true))`),
/// or `None` when no assertions match.
///
/// Each emitted [`Smell`] carries the offending assertion's
/// [`crate::domain::types::Span`] via `Smell::span` (SHAPE-Q1=(ii) at
/// the pipeline shape gate) so downstream consumers (SARIF reporter
/// at scrap-rs#17, mokumo scorecard) get per-instance line
/// attribution. N tautological assertions on one test produce 1
/// `Finding` with N `Smell`s; `Finding::scrap_score` aggregates
/// (10 × N).
#[must_use]
pub fn detect(parsed: &ParsedTest) -> Option<Finding> {
    let smells: Vec<Smell> = parsed
        .assertions
        .iter()
        .filter(|a| is_tautological(a))
        .map(|a| {
            Smell::new(
                SmellCategory::TautologicalAssertion,
                Severity::High,
                Actionability::AutoRefactor,
                PENALTY,
                Some(a.span),
            )
        })
        .collect();
    // `bool::then` keeps the "build Finding iff smells exist" intent
    // local to the predicate. Gemini MED on PR #83 flagged the prior
    // if/else block as less idiomatic; the refactor preserves the
    // exact-equivalent control flow.
    (!smells.is_empty()).then(|| Finding::new(parsed.identity.clone(), smells))
}

/// Tautology predicate composed from the two parser-supplied
/// structural facts. Private — callers go through [`detect`].
fn is_tautological(a: &ParsedAssertion) -> bool {
    a.arguments_identical || matches!(a.single_arg_value, Some(LiteralValue::Bool(true)))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::domain::assertion_sources::AssertionSource;
    use crate::domain::opt_outs::OptOut;
    use crate::domain::parsed::ParsedAttribute;
    use crate::domain::types::{FilePath, QualifiedName, Span, TestIdentity};
    use proptest::prelude::*;
    use std::collections::BTreeSet;

    // ── Fixtures ────────────────────────────────────────────────────

    fn identity() -> TestIdentity {
        TestIdentity::new(
            FilePath::new("a.rs"),
            QualifiedName::new("a::tests::t"),
            Span::new(10, 20, 1, 1),
        )
    }

    /// Build a `ParsedTest` with a single assertion shaped by the two
    /// structural facts.
    fn parsed_with_one(
        arguments_identical: bool,
        single_arg_value: Option<LiteralValue>,
    ) -> ParsedTest {
        parsed_with_assertions(vec![ParsedAssertion::new(
            "assert",
            None,
            Span::new(15, 15, 1, 1),
            arguments_identical,
            single_arg_value,
        )])
    }

    fn parsed_with_assertions(assertions: Vec<ParsedAssertion>) -> ParsedTest {
        ParsedTest::new(
            identity(),
            Vec::new(),
            assertions,
            5,
            Vec::new(),
            BTreeSet::new(),
            // `behavioral_facts` (added at scrap-rs#30) — empty for
            // tautological-assertion fixtures; the detector does not
            // consult this field.
            BTreeSet::new(),
        )
    }

    // ── Unit: filter clauses ────────────────────────────────────────

    #[test]
    fn detect_returns_none_for_empty_assertions() {
        let parsed = parsed_with_assertions(vec![]);
        assert!(detect(&parsed).is_none());
    }

    #[test]
    fn detect_returns_none_for_real_assertion() {
        // arguments_identical=false, single_arg_value=None — both
        // clauses fail; no smell.
        let parsed = parsed_with_one(false, None);
        assert!(detect(&parsed).is_none());
    }

    #[test]
    fn detect_returns_finding_for_assert_eq_x_x() {
        let parsed = parsed_with_one(true, None);
        let finding = detect(&parsed).expect("Some(Finding)");
        assert_eq!(finding.smells.len(), 1);
        assert_eq!(
            finding.smells[0].category,
            SmellCategory::TautologicalAssertion
        );
        assert_eq!(finding.smells[0].penalty, PENALTY);
        assert!(
            (finding.scrap_score - 10.0).abs() < f64::EPSILON,
            "scrap_score should be 10.0, got {}",
            finding.scrap_score
        );
    }

    #[test]
    fn detect_returns_finding_for_assert_true() {
        let parsed = parsed_with_one(false, Some(LiteralValue::Bool(true)));
        let finding = detect(&parsed).expect("Some(Finding)");
        assert_eq!(finding.smells.len(), 1);
        assert_eq!(finding.smells[0].penalty, PENALTY);
    }

    #[test]
    fn detect_returns_none_for_assert_false() {
        // SHAPE-Q3 lock: Bool(false) is NOT a tautology trigger.
        let parsed = parsed_with_one(false, Some(LiteralValue::Bool(false)));
        assert!(detect(&parsed).is_none());
    }

    #[test]
    fn detect_returns_none_for_int_literal() {
        // `assert!(0)` is ill-typed in Rust but the helper handles it
        // cleanly. Int literals are NOT a tautology trigger.
        let parsed = parsed_with_one(false, Some(LiteralValue::Int(0)));
        assert!(detect(&parsed).is_none());
    }

    #[test]
    fn detect_returns_none_for_str_literal() {
        let parsed = parsed_with_one(false, Some(LiteralValue::Str(String::new())));
        assert!(detect(&parsed).is_none());
    }

    #[test]
    fn detect_returns_none_for_verbatim() {
        let parsed = parsed_with_one(false, Some(LiteralValue::Verbatim("3.14".into())));
        assert!(detect(&parsed).is_none());
    }

    // ── Aggregation: N tautological asserts → 1 Finding, N Smells ──

    #[test]
    fn detect_aggregates_multiple_tautological_assertions() {
        let parsed = parsed_with_assertions(vec![
            ParsedAssertion::new("assert_eq", None, Span::new(11, 11, 1, 1), true, None),
            ParsedAssertion::new(
                "assert",
                None,
                Span::new(12, 12, 1, 1),
                false,
                Some(LiteralValue::Bool(true)),
            ),
            ParsedAssertion::new("assert_ne", None, Span::new(13, 13, 1, 1), true, None),
        ]);
        let finding = detect(&parsed).expect("Some(Finding)");
        assert_eq!(finding.smells.len(), 3);
        assert!(
            (finding.scrap_score - 30.0).abs() < f64::EPSILON,
            "scrap_score should be 30.0 (3 × 10), got {}",
            finding.scrap_score
        );
        for smell in &finding.smells {
            assert_eq!(smell.category, SmellCategory::TautologicalAssertion);
            assert_eq!(smell.penalty, PENALTY);
        }
    }

    // ── Per-Smell span attribution (SHAPE-Q1=(ii)) ──────────────────

    #[test]
    fn detect_emitted_smell_carries_assertion_span() {
        let assertion_span = Span::new(42, 42, 1, 1);
        let parsed = parsed_with_assertions(vec![ParsedAssertion::new(
            "assert",
            None,
            assertion_span,
            false,
            Some(LiteralValue::Bool(true)),
        )]);
        let finding = detect(&parsed).expect("Some(Finding)");
        assert_eq!(finding.smells[0].span, Some(assertion_span));
    }

    #[test]
    fn detect_each_smell_in_aggregate_carries_its_own_span() {
        let parsed = parsed_with_assertions(vec![
            ParsedAssertion::new("assert_eq", None, Span::new(11, 11, 1, 1), true, None),
            ParsedAssertion::new("assert_eq", None, Span::new(22, 22, 1, 1), true, None),
        ]);
        let finding = detect(&parsed).expect("Some(Finding)");
        assert_eq!(finding.smells[0].span, Some(Span::new(11, 11, 1, 1)));
        assert_eq!(finding.smells[1].span, Some(Span::new(22, 22, 1, 1)));
    }

    // ── Explicit non-responsibilities (pipeline-side at scrap-rs#72)

    #[test]
    fn detect_does_not_consult_opt_outs() {
        // ParsedTest with OptOut::Tautology in opt_outs and one
        // tautological assertion — detector STILL emits a Finding.
        // The pipeline driver (scrap-rs#72) is responsible for the
        // policy-driven suppression / demotion.
        let mut opt_outs = BTreeSet::new();
        opt_outs.insert(OptOut::Tautology);
        let parsed = ParsedTest::new(
            identity(),
            Vec::new(),
            vec![ParsedAssertion::new(
                "assert_eq",
                None,
                Span::new(15, 15, 1, 1),
                true,
                None,
            )],
            5,
            Vec::new(),
            opt_outs,
            BTreeSet::new(),
        );
        assert!(detect(&parsed).is_some());
    }

    #[test]
    fn detect_does_not_consult_should_panic_implicit_source() {
        // ParsedTest with ShouldPanic in implicit_assertion_sources
        // and one tautological assertion — detector STILL emits a
        // Finding. Pipeline at scrap-rs#72 handles the suppression
        // policy.
        let parsed = ParsedTest::new(
            identity(),
            vec![ParsedAttribute::new("should_panic", None)],
            vec![ParsedAssertion::new(
                "assert",
                None,
                Span::new(15, 15, 1, 1),
                false,
                Some(LiteralValue::Bool(true)),
            )],
            5,
            vec![AssertionSource::ShouldPanic],
            BTreeSet::new(),
            BTreeSet::new(),
        );
        assert!(detect(&parsed).is_some());
    }

    // ── Property test: determinism (AC #7 — see PR body for the
    //    AC-text translation note. The literal AC reads
    //    `detect(detect(ast)) == detect(ast)` which doesn't compose
    //    given `detect : &ParsedTest -> Option<Finding>`; the
    //    pure-function intent is what `detect_is_deterministic`
    //    captures.) ────────────────────────────────────────────────

    /// Generator: arbitrary `ParsedAssertion` shaped enough to exercise
    /// both fact clauses without panicking on inverted spans
    /// (`Span::new`'s `debug_assert!`).
    fn arb_parsed_assertion() -> impl Strategy<Value = ParsedAssertion> {
        let arb_literal = prop_oneof![
            Just(LiteralValue::Bool(true)),
            Just(LiteralValue::Bool(false)),
            (any::<i64>()).prop_map(|n| LiteralValue::Int(i128::from(n))),
            "[ -~]{0,16}".prop_map(LiteralValue::Str),
        ];
        (
            "(assert|assert_eq|assert_ne)",
            any::<bool>(),
            prop::option::of(arb_literal),
            (1u32..1000, 0u32..100),
        )
            .prop_map(|(name, ident, val, (start, len))| {
                ParsedAssertion::new(name, None, Span::new(start, start + len, 1, 1), ident, val)
            })
    }

    fn arb_parsed_test() -> impl Strategy<Value = ParsedTest> {
        prop::collection::vec(arb_parsed_assertion(), 0..6).prop_map(parsed_with_assertions)
    }

    proptest! {
        #[test]
        fn detect_is_deterministic(parsed in arb_parsed_test()) {
            let first = detect(&parsed);
            let second = detect(&parsed);
            // PartialEq on Finding covers everything we care about
            // (test identity, smells, scrap_score, exceeds_threshold,
            // opt_outs). f64 scrap_score: comparing the SAME computation
            // twice is safe; this is exactly equality, not approximate.
            prop_assert_eq!(first.is_some(), second.is_some());
            prop_assert_eq!(first, second);
        }

        #[test]
        fn detect_smell_count_equals_tautological_count(parsed in arb_parsed_test()) {
            let expected = parsed.assertions.iter().filter(|a| is_tautological(a)).count();
            let actual = detect(&parsed).map_or(0, |f| f.smells.len());
            prop_assert_eq!(expected, actual);
        }
    }
}
