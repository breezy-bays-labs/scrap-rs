//! `no-op-io` detector (scrap-rs#25) — flags `#[test]` bodies that
//! **run but check nothing**: a discarded `Result`-shaped value
//! (`let _ = <call>;`) with no follow-up assertion or check.
//!
//! ## Detection rule (v0.1)
//!
//! Conjunction; ALL must hold for the detector to emit a [`Finding`]:
//!
//! 1. `cfg.enabled != Some(false)` — config can disable per-detector.
//! 2. The body carries **at least one**
//!    [`BehavioralFact::ResultDiscarded`] (a `let _ = <Result-shaped
//!    expr>;` discard the parser projected — see
//!    [`crate::domain::behavioral_fact::ResultDiscardKind`] for the
//!    recognised shapes).
//! 3. `detectors::has_positive_check` is `false` — the test holds NO
//!    positive evidence it observes the system-under-test (no explicit
//!    assertion, no implicit-assertion source, no `.unwrap()`/`.expect()`
//!    [`BehavioralFact::ResultAsserted`] chain).
//!
//! ### "ALL body statements discard", modeled as the fact bag
//!
//! The issue AC phrases the rule as "ALL body statements are
//! `Result`-discarding". Rather than push a statement-count projection
//! across the port boundary (statement-projection tarpit — see
//! `feedback_semantic-facts-vs-statement-projection`), the detector
//! models "checks nothing" as **"≥1 discard ∧ zero positive-check
//! facts"**. A body that discards AND asserts is suppressed by clause 3;
//! a body that only discards (no asserts) fires. A body that discards
//! plus does other non-asserting work (`let _ = f(); g();`) still fires
//! — correct, because it still checks nothing.
//!
//! ## v0.1 over-fire — honest-signal note
//!
//! `ResultDiscardKind::Call` fires on ANY discarded call, not just I/O
//! (`let _ = pure_fn();` projects). So `no-op-**io**` is broader than
//! its name in v0.1. This is tolerable: it co-fires with
//! `zero-assertion` (their penalties STACK — Option A, 10 + 8 = 18) and
//! only fires when there are zero positive checks, so it never falsely
//! accuses a test that actually asserts. An I/O-narrowing refinement
//! (distinguishing filesystem/network/process calls from pure calls)
//! is a v0.3+ follow-up, not v0.1.
//!
//! ## Precedence vs. stacking (Option A, locked at scrap-rs#25 cabinet)
//!
//! `no-op-io` is a strict **subset** of `zero-assertion`: an all-discard
//! body has no assertions, so both detectors fire. In v0.1 their
//! penalties **stack** in `detectors::detect_all` (the `Finding`'s
//! `scrap_score` sums to 18). Whether the more-specific smell should
//! *supersede* the general one is a scoring-layer policy deferred to the
//! scrap-rs#32 `score_example` aggregator.
//!
//! ## Pure-detector convention
//!
//! Mirrors `zero_assertion`: the detector does NOT consult
//! `parsed.opt_outs`. Per-test `#[allow(scrap::no_op)]` honor-policy is
//! the pipeline driver's job (scrap-rs#72); the driver applies opt-out
//! suppression post-emission.

use crate::cli::config::DetectorConfig;
use crate::domain::behavioral_fact::BehavioralFact;
use crate::domain::classification::{Actionability, Severity};
use crate::domain::finding::Finding;
use crate::domain::parsed::ParsedTest;
use crate::domain::smell::{Smell, SmellCategory};

/// Default penalty per the CLAUDE.md / kickstart-plan detection table.
pub(crate) const DEFAULT_PENALTY: u32 = 8;

/// Default severity: below `zero-assertion`'s `High` (penalty 8 < 10).
const DEFAULT_SEVERITY: Severity = Severity::Moderate;

/// Default actionability: the smell suggests a mechanical fix (inspect
/// or assert on the discarded value).
const DEFAULT_ACTIONABILITY: Actionability = Actionability::AutoRefactor;

/// Detect the `no-op-io` smell on a parsed test.
///
/// See module-level docs for the detection rule + the over-fire and
/// stacking notes. Returns:
/// - `None` when the detector is disabled, when the body carries no
///   [`BehavioralFact::ResultDiscarded`], or when
///   `detectors::has_positive_check` holds.
/// - `Some(Finding)` carrying one [`Smell`] whose
///   `category = SmellCategory::NoOpIo`, `severity = Severity::Moderate`,
///   `actionability = Actionability::AutoRefactor`, and
///   `penalty = cfg.penalty.unwrap_or(DEFAULT_PENALTY)`.
#[must_use]
pub fn detect(parsed: &ParsedTest, cfg: &DetectorConfig) -> Option<Finding> {
    if cfg.enabled == Some(false) {
        return None;
    }
    if !has_result_discard(parsed) {
        return None;
    }
    if super::has_positive_check(parsed) {
        return None;
    }

    let penalty = cfg.penalty.unwrap_or(DEFAULT_PENALTY);
    // Whole-test span: "checks nothing" is a fn-level verdict, so the
    // smell points at `parsed.identity.span` (the full `fn name(...) {
    // .. }`) like `zero-assertion`, not at any single discard line.
    let smell = Smell::new(
        SmellCategory::NoOpIo,
        DEFAULT_SEVERITY,
        DEFAULT_ACTIONABILITY,
        penalty,
        Some(parsed.identity.span),
    );
    Some(Finding::new(parsed.identity.clone(), vec![smell]))
}

/// `true` when the body carries at least one
/// [`BehavioralFact::ResultDiscarded`] fact (of any
/// [`crate::domain::behavioral_fact::ResultDiscardKind`]).
fn has_result_discard(parsed: &ParsedTest) -> bool {
    parsed
        .behavioral_facts
        .iter()
        .any(|f| matches!(f, BehavioralFact::ResultDiscarded { .. }))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::domain::assertion_sources::AssertionSource;
    use crate::domain::behavioral_fact::ResultDiscardKind;
    use crate::domain::parsed::ParsedAssertion;
    use crate::domain::types::{FilePath, QualifiedName, Span, TestIdentity};
    use proptest::prelude::*;
    use std::collections::BTreeSet;

    // ── Test helpers ────────────────────────────────────────────────────

    /// Build a baseline smelly `ParsedTest`: one `ResultDiscarded`
    /// fact, no assertions, no implicit sources, no `ResultAsserted`.
    /// Detector fires on this baseline unless cfg disables it.
    fn smelly_test() -> ParsedTest {
        let facts = vec![BehavioralFact::ResultDiscarded {
            kind: ResultDiscardKind::Call,
        }];
        ParsedTest::new(
            TestIdentity::new(
                FilePath::new("a.rs"),
                QualifiedName::new("a::tests::t"),
                Span::new(1, 5, 1, 1),
            ),
            Vec::new(),
            Vec::new(),
            3,
            Vec::new(),
            BTreeSet::new(),
            facts,
        )
    }

    // ── Negative branches: detector returns None ────────────────────────

    #[test]
    fn detect_returns_none_when_disabled_via_config() {
        let cfg = DetectorConfig {
            enabled: Some(false),
            penalty: None,
            line_threshold: None,
        };
        assert!(detect(&smelly_test(), &cfg).is_none());
    }

    #[test]
    fn detect_returns_none_without_any_result_discard() {
        let mut pt = smelly_test();
        pt.behavioral_facts.clear();
        assert!(detect(&pt, &DetectorConfig::default()).is_none());
    }

    #[test]
    fn detect_returns_none_with_explicit_assertion() {
        let mut pt = smelly_test();
        pt.assertions.push(ParsedAssertion::new(
            "assert_eq",
            Some("1, 1".into()),
            Span::new(2, 2, 1, 1),
            false,
            None,
        ));
        assert!(detect(&pt, &DetectorConfig::default()).is_none());
    }

    #[test]
    fn detect_returns_none_with_implicit_source() {
        let mut pt = smelly_test();
        pt.implicit_assertion_sources
            .push(AssertionSource::ShouldPanic);
        assert!(detect(&pt, &DetectorConfig::default()).is_none());
    }

    #[test]
    fn detect_returns_none_with_result_asserted() {
        // `let _ = x.unwrap();` style: the discard co-exists with a
        // ResultAsserted fact, which is positive evidence → suppressed.
        let mut pt = smelly_test();
        pt.behavioral_facts.push(BehavioralFact::ResultAsserted);
        assert!(detect(&pt, &DetectorConfig::default()).is_none());
    }

    // ── Positive branches ────────────────────────────────────────────────

    #[test]
    #[allow(clippy::float_cmp)]
    fn detect_fires_on_bare_discard() {
        let finding = detect(&smelly_test(), &DetectorConfig::default()).expect("baseline fires");
        assert_eq!(finding.smells.len(), 1);
        assert_eq!(finding.smells[0].category, SmellCategory::NoOpIo);
        assert_eq!(finding.smells[0].penalty, 8);
        assert_eq!(finding.smells[0].severity, Severity::Moderate);
        assert_eq!(finding.scrap_score, 8.0);
        // Whole-test span attribution.
        assert_eq!(finding.smells[0].span, Some(Span::new(1, 5, 1, 1)));
    }

    #[test]
    fn detect_fires_on_each_discard_kind() {
        for kind in [
            ResultDiscardKind::Call,
            ResultDiscardKind::ResultCtor,
            ResultDiscardKind::ResultAdapter,
        ] {
            let mut pt = smelly_test();
            pt.behavioral_facts.clear();
            pt.behavioral_facts
                .push(BehavioralFact::ResultDiscarded { kind });
            assert!(
                detect(&pt, &DetectorConfig::default()).is_some(),
                "kind {kind:?} should fire",
            );
        }
    }

    #[test]
    fn detect_applies_custom_penalty_override() {
        let cfg = DetectorConfig {
            enabled: None,
            penalty: Some(25),
            line_threshold: None,
        };
        let finding = detect(&smelly_test(), &cfg).expect("smelly test fires under override");
        assert_eq!(finding.smells.len(), 1);
        assert_eq!(finding.smells[0].penalty, 25);
    }

    // ── Property tests ────────────────────────────────────────────────────

    /// Arbitrary `ParsedTest` exercising the discard + positive-check
    /// fact space the detector reads.
    fn arb_parsed_test() -> impl Strategy<Value = ParsedTest> {
        (
            // 0..3 discard facts of varying kind.
            proptest::collection::vec(0u8..3, 0..3),
            // assertion count.
            0usize..3,
            // implicit-source count.
            0usize..3,
            // ResultAsserted present?
            any::<bool>(),
        )
            .prop_map(|(discard_kinds, n_assert, n_impl, has_asserted)| {
                // `Vec` storage (scrap-rs#112) with projection-mirroring
                // dedup — equal presence facts collapse exactly as the
                // parser's guarded push does, so the strategy never
                // produces a fact bag the real parser couldn't emit.
                let mut facts: Vec<BehavioralFact> = Vec::new();
                for k in discard_kinds {
                    let kind = match k {
                        0 => ResultDiscardKind::Call,
                        1 => ResultDiscardKind::ResultCtor,
                        _ => ResultDiscardKind::ResultAdapter,
                    };
                    let fact = BehavioralFact::ResultDiscarded { kind };
                    if !facts.contains(&fact) {
                        facts.push(fact);
                    }
                }
                if has_asserted {
                    facts.push(BehavioralFact::ResultAsserted);
                }
                let assertions = (0..n_assert)
                    .map(|i| {
                        let line = u32::try_from(i).unwrap_or(0) + 1;
                        ParsedAssertion::new(
                            "assert",
                            None,
                            Span::new(line, line, 1, 1),
                            false,
                            None,
                        )
                    })
                    .collect();
                let implicit = (0..n_impl).map(|_| AssertionSource::Proptest).collect();
                ParsedTest::new(
                    TestIdentity::new(
                        FilePath::new("a.rs"),
                        QualifiedName::new("a::tests::t"),
                        Span::new(1, 5, 1, 1),
                    ),
                    Vec::new(),
                    assertions,
                    3,
                    implicit,
                    BTreeSet::new(),
                    facts,
                )
            })
    }

    fn arb_detector_config() -> impl Strategy<Value = DetectorConfig> {
        (
            proptest::option::of(any::<bool>()),
            proptest::option::of(1u32..1_000),
        )
            .prop_map(|(enabled, penalty)| DetectorConfig {
                enabled,
                penalty,
                line_threshold: None,
            })
    }

    proptest! {
        /// Determinism (the AC's idempotence intent — the literal
        /// `detect(detect(ast))` doesn't typecheck given
        /// `detect : &ParsedTest -> Option<Finding>`; the pure-function
        /// contract is what this captures, matching the
        /// `tautological_assertion` PR-note translation).
        #[test]
        fn proptest_detect_is_deterministic(
            pt in arb_parsed_test(),
            cfg in arb_detector_config(),
        ) {
            prop_assert_eq!(detect(&pt, &cfg), detect(&pt, &cfg));
        }

        /// Cardinality: result is `None` or a single-Smell `Finding`.
        #[test]
        fn proptest_detect_emits_at_most_one_smell(
            pt in arb_parsed_test(),
            cfg in arb_detector_config(),
        ) {
            if let Some(finding) = detect(&pt, &cfg) {
                prop_assert_eq!(finding.smells.len(), 1);
            }
        }

        /// Suppression monotonicity: when the baseline fires, adding any
        /// positive-check fact (assertion / implicit source /
        /// ResultAsserted) flips it to `None`.
        #[test]
        fn proptest_detect_suppression_is_monotonic(
            pt in arb_parsed_test(),
            cfg in arb_detector_config(),
        ) {
            if detect(&pt, &cfg).is_none() {
                return Ok(());
            }
            let mut pt_a = pt.clone();
            pt_a.assertions
                .push(ParsedAssertion::new("assert", None, Span::new(1, 1, 1, 1), false, None));
            prop_assert!(detect(&pt_a, &cfg).is_none(), "adding an assertion must suppress");

            let mut pt_b = pt.clone();
            pt_b.implicit_assertion_sources.push(AssertionSource::ShouldPanic);
            prop_assert!(detect(&pt_b, &cfg).is_none(), "adding an implicit source must suppress");

            let mut pt_c = pt;
            // `Vec` storage (scrap-rs#112): push the presence fact only
            // if absent, mirroring the parser's projection-time dedup.
            if !pt_c
                .behavioral_facts
                .contains(&BehavioralFact::ResultAsserted)
            {
                pt_c.behavioral_facts.push(BehavioralFact::ResultAsserted);
            }
            prop_assert!(detect(&pt_c, &cfg).is_none(), "adding ResultAsserted must suppress");
        }
    }

    // ── Subset-relationship pin (SHOULD-FIX #7) ──────────────────────────

    #[test]
    fn no_op_io_is_subset_of_zero_assertion() {
        // Every input that fires no-op-io must ALSO fire zero-assertion
        // (the strict-subset relationship). An all-discard body has no
        // positive check, so zero-assertion fires too — proving the
        // 18-point stack is the co-fire, not a bug.
        use crate::detectors::zero_assertion;
        let pt = smelly_test();
        assert!(detect(&pt, &DetectorConfig::default()).is_some());
        assert!(
            zero_assertion::detect(&pt, &DetectorConfig::default()).is_some(),
            "a no-op-io firing input must also fire zero-assertion (subset proof)",
        );
    }
}
