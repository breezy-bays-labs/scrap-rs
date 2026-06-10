//! `zero-assertion` detector — flags `#[test]` bodies with no observable
//! check on the system under test.
//!
//! ## Detection rule (v0.1)
//!
//! Three-clause conjunction; ALL must hold for the detector to emit a
//! [`Finding`]:
//!
//! 1. `parsed.assertions.is_empty()` — no explicit assertion macro
//!    recognised by the parser (`assert!`/`assert_eq!`/`assert_ne!`/
//!    `assert_matches!`/`panic!`/`unimplemented!`/`todo!`).
//! 2. `parsed.implicit_assertion_sources.is_empty()` — no runner shell
//!    (`proptest`/`quickcheck`/`kani`/`cucumber`/`trybuild`/`insta`/
//!    `pretty_assertions`) and no `#[should_panic]` attribute on the
//!    test fn.
//! 3. `!parsed.behavioral_facts.contains(&BehavioralFact::ResultAsserted)`
//!    — no `.unwrap()`/`.expect()` method-call chain (the
//!    explicit-panic-is-the-assertion pattern).
//!
//! ## Pure-detector convention (architecture decision 2026-05-26)
//!
//! Detector does NOT read `parsed.opt_outs`. Per-test suppression
//! honor-policy is the pipeline driver's job (scrap-rs#72). When the
//! detector's facts indicate the smell, this function emits a
//! [`Finding`] regardless of any per-test `#[allow(scrap::no_asserts)]`
//! attribute the parser recorded; the driver applies
//! `apply_opt_out_policy(finding, parsed, policy)` post-emission to
//! drop or demote findings per project-level honor configuration.
//!
//! ## Penalty and config gating
//!
//! - `cfg.enabled == Some(false)` short-circuits to `None` regardless
//!   of facts (CLI / `scrap.toml` can disable per-detector).
//! - Penalty resolves to `cfg.penalty.unwrap_or(DEFAULT_PENALTY)`.
//!   The config validator in `cli/config.rs` rejects `Some(0)` so the
//!   effective floor is always >= 1.
//!
//! TODO(scrap-rs#73): once `adr-port-surface-and-domain-conventions`
//! lands, link to it here for the dumb-parser/smart-detector boundary
//! (D10) rationale.

use crate::cli::config::DetectorConfig;
use crate::domain::classification::{Actionability, Severity};
use crate::domain::finding::Finding;
use crate::domain::parsed::ParsedTest;
use crate::domain::smell::{Smell, SmellCategory};

/// Default penalty per CLAUDE.md detection rules table.
pub(crate) const DEFAULT_PENALTY: u32 = 10;

/// Default severity for the zero-assertion smell.
const DEFAULT_SEVERITY: Severity = Severity::High;

/// Default actionability classification: the smell is an
/// auto-refactor candidate (add an assertion that observes the
/// system-under-test's effect).
const DEFAULT_ACTIONABILITY: Actionability = Actionability::AutoRefactor;

/// Detect the `zero-assertion` smell on a parsed test.
///
/// See module-level docs for the three-clause detection rule + the
/// pure-detector convention. Returns:
/// - `None` when the detector is disabled, when any clause fails, or
///   when the parser recognised the explicit-panic-is-the-assertion
///   pattern via [`crate::domain::behavioral_fact::BehavioralFact::ResultAsserted`].
/// - `Some(Finding)` carrying one [`Smell`] whose
///   `category = SmellCategory::ZeroAssertion`,
///   `severity = Severity::High`,
///   `actionability = Actionability::AutoRefactor`, and
///   `penalty = cfg.penalty.unwrap_or(DEFAULT_PENALTY)`.
#[must_use]
pub fn detect(parsed: &ParsedTest, cfg: &DetectorConfig) -> Option<Finding> {
    if cfg.enabled == Some(false) {
        return None;
    }
    // Three-clause suppression (explicit assertion ∨ implicit source ∨
    // `ResultAsserted` chain) is the shared `no-op-io` ⊂ `zero-assertion`
    // subset predicate — factored into `detectors::has_positive_check`
    // (scrap-rs#25 SHOULD-FIX #7) so both detectors can't drift.
    if super::has_positive_check(parsed) {
        return None;
    }

    let penalty = cfg.penalty.unwrap_or(DEFAULT_PENALTY);
    // Whole-test span: absence-of-assertion is a fn-level fact, so the
    // detector emits `parsed.identity.span` (covers `fn name(...) { .. }`
    // from signature to closing brace) rather than `body_line_count`
    // (body-block-only, useful for `large-example` at scrap-rs#27).
    // `Span` is `Copy`-derived so no `.clone()` needed.
    let smell = Smell::new(
        SmellCategory::ZeroAssertion,
        DEFAULT_SEVERITY,
        DEFAULT_ACTIONABILITY,
        penalty,
        Some(parsed.identity.span),
    );
    Some(Finding::new(parsed.identity.clone(), vec![smell]))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::domain::assertion_sources::AssertionSource;
    use crate::domain::behavioral_fact::BehavioralFact;
    use crate::domain::parsed::{ParsedAssertion, ParsedAttribute};
    use crate::domain::types::{FilePath, QualifiedName, Span, TestIdentity};
    use proptest::prelude::*;
    use std::collections::BTreeSet;

    // ── Test helpers ────────────────────────────────────────────────────

    /// Build a baseline smelly `ParsedTest`: empty assertions, empty
    /// `implicit_assertion_sources`, empty `behavioral_facts`. Detector
    /// triggers on this baseline unless cfg disables it.
    fn smelly_test() -> ParsedTest {
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
            Vec::new(),
        )
    }

    /// Build a smelly `ParsedTest` with one explicit assertion already
    /// recorded — detector must NOT fire.
    fn test_with_assertion() -> ParsedTest {
        let mut pt = smelly_test();
        pt.assertions.push(ParsedAssertion::new(
            "assert_eq",
            Some("1, 1".into()),
            Span::new(2, 2, 1, 1),
            // scrap-rs#24 — non-tautological assertion in this
            // zero-assertion-detector fixture; defaults are correct.
            false,
            None,
        ));
        pt
    }

    /// Build a smelly `ParsedTest` carrying a single implicit-assertion
    /// source — detector must NOT fire.
    fn test_with_implicit_source(src: AssertionSource) -> ParsedTest {
        let mut pt = smelly_test();
        pt.implicit_assertion_sources.push(src);
        pt
    }

    /// Build a smelly `ParsedTest` carrying the `ResultAsserted`
    /// behavioral fact — detector must NOT fire.
    fn test_with_result_asserted() -> ParsedTest {
        let mut pt = smelly_test();
        pt.behavioral_facts.push(BehavioralFact::ResultAsserted);
        pt
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
    fn detect_returns_none_with_explicit_assertion() {
        assert!(detect(&test_with_assertion(), &DetectorConfig::default()).is_none());
    }

    #[test]
    fn detect_returns_none_with_implicit_source_should_panic() {
        assert!(
            detect(
                &test_with_implicit_source(AssertionSource::ShouldPanic),
                &DetectorConfig::default(),
            )
            .is_none()
        );
    }

    #[test]
    fn detect_returns_none_with_implicit_source_proptest() {
        assert!(
            detect(
                &test_with_implicit_source(AssertionSource::Proptest),
                &DetectorConfig::default(),
            )
            .is_none()
        );
    }

    #[test]
    fn detect_returns_none_with_implicit_source_quickcheck() {
        assert!(
            detect(
                &test_with_implicit_source(AssertionSource::Quickcheck),
                &DetectorConfig::default(),
            )
            .is_none()
        );
    }

    #[test]
    fn detect_returns_none_with_implicit_source_kani() {
        assert!(
            detect(
                &test_with_implicit_source(AssertionSource::Kani),
                &DetectorConfig::default(),
            )
            .is_none()
        );
    }

    #[test]
    fn detect_returns_none_with_implicit_source_cucumber() {
        assert!(
            detect(
                &test_with_implicit_source(AssertionSource::Cucumber),
                &DetectorConfig::default(),
            )
            .is_none()
        );
    }

    #[test]
    fn detect_returns_none_with_implicit_source_trybuild() {
        assert!(
            detect(
                &test_with_implicit_source(AssertionSource::Trybuild),
                &DetectorConfig::default(),
            )
            .is_none()
        );
    }

    #[test]
    fn detect_returns_none_with_implicit_source_insta() {
        assert!(
            detect(
                &test_with_implicit_source(AssertionSource::Insta),
                &DetectorConfig::default(),
            )
            .is_none()
        );
    }

    #[test]
    fn detect_returns_none_with_implicit_source_pretty_assertions() {
        assert!(
            detect(
                &test_with_implicit_source(AssertionSource::PrettyAssertions),
                &DetectorConfig::default(),
            )
            .is_none()
        );
    }

    #[test]
    fn detect_returns_none_with_result_asserted_behavioral_fact() {
        assert!(detect(&test_with_result_asserted(), &DetectorConfig::default()).is_none());
    }

    #[test]
    fn detect_returns_none_when_all_three_suppression_branches_populated() {
        // Defense-in-depth: explicit + implicit + behavioral all populated.
        let mut pt = smelly_test();
        pt.assertions.push(ParsedAssertion::new(
            "assert",
            None,
            Span::new(2, 2, 1, 1),
            false,
            None,
        ));
        pt.implicit_assertion_sources
            .push(AssertionSource::ShouldPanic);
        pt.behavioral_facts.push(BehavioralFact::ResultAsserted);
        // Add a non-empty `attributes` for completeness (detector doesn't
        // read it, but exercises that vec-bearing builders still pass).
        pt.attributes.push(ParsedAttribute::new("test", None));
        assert!(detect(&pt, &DetectorConfig::default()).is_none());
    }

    // ── Positive branch: custom-penalty override ────────────────────────

    #[test]
    #[allow(clippy::float_cmp)]
    fn detect_applies_custom_penalty_override() {
        let cfg = DetectorConfig {
            enabled: None,
            penalty: Some(25),
            line_threshold: None,
        };
        let finding = detect(&smelly_test(), &cfg).expect("smelly test fires under override");
        assert_eq!(finding.smells.len(), 1);
        assert_eq!(finding.smells[0].penalty, 25);
        // Finding::scrap_score is f64 but sum of u32 penalties — direct
        // equality is sound here (single-Smell finding, integer-derived).
        assert_eq!(finding.scrap_score, 25.0);
    }

    // ── Property tests (deterministic + cardinality + monotonicity) ────

    /// Proptest strategy: arbitrary `ParsedTest` with bounded vec
    /// sizes. Identity span is fixed for cheap construction; the
    /// detector doesn't read `body_line_count` or attributes, so those
    /// stay constant.
    fn arb_parsed_test() -> impl Strategy<Value = ParsedTest> {
        (
            proptest::collection::vec(any::<u8>(), 0..5).prop_map(|v| v.len()),
            proptest::collection::vec(0u8..8, 0..3),
            any::<bool>(),
        )
            .prop_map(|(assertion_count, implicit_indices, has_behavioral)| {
                let assertions = (0..assertion_count)
                    .map(|i| {
                        ParsedAssertion::new(
                            "assert",
                            None,
                            Span::new(
                                u32::try_from(i).unwrap_or(0) + 1,
                                u32::try_from(i).unwrap_or(0) + 1,
                                1,
                                1,
                            ),
                            // scrap-rs#24 — zero-assertion proptest uses
                            // non-tautological assertions; defaults stand.
                            false,
                            None,
                        )
                    })
                    .collect();
                let implicit_assertion_sources = implicit_indices
                    .into_iter()
                    .map(|idx| match idx {
                        0 => AssertionSource::Proptest,
                        1 => AssertionSource::Quickcheck,
                        2 => AssertionSource::Kani,
                        3 => AssertionSource::Cucumber,
                        4 => AssertionSource::Trybuild,
                        5 => AssertionSource::Insta,
                        6 => AssertionSource::PrettyAssertions,
                        _ => AssertionSource::ShouldPanic,
                    })
                    .collect();
                let mut behavioral_facts: Vec<BehavioralFact> = Vec::new();
                if has_behavioral {
                    behavioral_facts.push(BehavioralFact::ResultAsserted);
                }
                ParsedTest::new(
                    TestIdentity::new(
                        FilePath::new("a.rs"),
                        QualifiedName::new("a::tests::t"),
                        Span::new(1, 5, 1, 1),
                    ),
                    Vec::new(),
                    assertions,
                    3,
                    implicit_assertion_sources,
                    BTreeSet::new(),
                    behavioral_facts,
                )
            })
    }

    /// Proptest strategy: arbitrary `DetectorConfig`. Skips
    /// `penalty == Some(0)` because the config validator in
    /// `cli/config.rs::validate_detector_config` rejects it; producing
    /// 0 here would let the detector run on an input the loader would
    /// have already rejected upstream, weakening the property.
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
        /// Deterministic + total + panic-free over the proptest input
        /// space. Calling `detect` twice with the same inputs returns
        /// equal results.
        #[test]
        fn proptest_detect_is_deterministic(
            pt in arb_parsed_test(),
            cfg in arb_detector_config(),
        ) {
            let a = detect(&pt, &cfg);
            let b = detect(&pt, &cfg);
            prop_assert_eq!(a, b);
        }

        /// Cardinality contract (CQO FOLD-REQUIRED): result is either
        /// `None` OR `Some(finding)` where `finding.smells.len() == 1`.
        /// Pins the one-Smell-per-emission invariant; catches future
        /// mutations that vec-extend the Smell list.
        #[test]
        fn proptest_detect_emits_at_most_one_smell(
            pt in arb_parsed_test(),
            cfg in arb_detector_config(),
        ) {
            if let Some(finding) = detect(&pt, &cfg) {
                prop_assert_eq!(finding.smells.len(), 1);
            }
        }

        /// Suppression monotonicity (CQO FOLD-REQUIRED): if `detect`
        /// returns `Some(_)`, then ANY of these mutations flips it to
        /// `None` — (a) push an assertion, (b) push an implicit
        /// source, (c) insert `BehavioralFact::ResultAsserted`. Catches
        /// `&&`→`||` mutations on the three-clause predicate.
        #[test]
        fn proptest_detect_suppression_is_monotonic(
            pt in arb_parsed_test(),
            cfg in arb_detector_config(),
        ) {
            // Only meaningful when the baseline triggers (so cfg.enabled
            // != Some(false) too).
            if detect(&pt, &cfg).is_none() {
                return Ok(());
            }

            // (a) push an assertion.
            let mut pt_a = pt.clone();
            pt_a.assertions
                .push(ParsedAssertion::new("assert", None, Span::new(1, 1, 1, 1), false, None));
            prop_assert!(detect(&pt_a, &cfg).is_none(), "adding an assertion must suppress");

            // (b) push an implicit source.
            let mut pt_b = pt.clone();
            pt_b.implicit_assertion_sources
                .push(AssertionSource::ShouldPanic);
            prop_assert!(detect(&pt_b, &cfg).is_none(), "adding an implicit source must suppress");

            // (c) push ResultAsserted (Vec storage, scrap-rs#112) only if
            // absent, mirroring the parser's projection-time dedup.
            let mut pt_c = pt;
            if !pt_c
                .behavioral_facts
                .contains(&BehavioralFact::ResultAsserted)
            {
                pt_c.behavioral_facts.push(BehavioralFact::ResultAsserted);
            }
            prop_assert!(detect(&pt_c, &cfg).is_none(), "adding ResultAsserted must suppress");
        }
    }
}
