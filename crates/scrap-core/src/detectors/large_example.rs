//! `large-example` detector (scrap-rs#31) — flags `#[test]` bodies
//! that exceed a configured line threshold: the test is doing too much
//! to read as a single focused example.
//!
//! ## Detection rule (v0.1)
//!
//! Fires when, in order:
//!
//! 1. `cfg.enabled != Some(false)` — config can disable per-detector.
//! 2. `parsed.body_line_count > cfg.line_threshold.unwrap_or(DEFAULT_LINE_THRESHOLD)`
//!    — a strict `>` comparison: a body exactly AT the threshold is NOT
//!    flagged (the threshold is the largest acceptable size).
//!
//! The comparison reads [`ParsedTest::body_line_count`] — the line
//! count of the **body block interior** (between the fn's `{` and `}`),
//! which the parser adapter projects. The detector itself does no
//! counting; it is a single threshold comparison over a
//! language-agnostic structural fact.
//!
//! [`ParsedTest::body_line_count`]: crate::domain::parsed::ParsedTest::body_line_count
//!
//! ## Default threshold — 30, Rust-tuned
//!
//! The default of `30` lines is tuned higher than Uncle Bob's Clojure
//! `scrap` default of `20` (see CLAUDE.md detection table). Rust test
//! bodies are syntactically more verbose than Speclj examples — type
//! annotations, explicit `let` bindings, builder calls, and closing
//! braces all consume lines that carry no extra "behavior" — so the
//! same conceptual test reads as more physical lines. `30` keeps the
//! signal honest (genuinely sprawling tests fire) without punishing
//! Rust's natural line cost. The knob is per-project tunable via
//! `[detectors.large_example] line_threshold = N`.
//!
//! ## Orthogonal to the assertion-based smells
//!
//! `large-example` is purely **structural** (body length); it never
//! reads `parsed.assertions`, `parsed.behavioral_facts`, or the
//! implicit-assertion sources. It therefore neither suppresses nor is
//! suppressed by `zero-assertion` / `no-op-io` / `tautological-
//! assertion`: a large body that also fails to assert co-fires both
//! `large-example` (penalty 4) AND `zero-assertion` (penalty 10), and
//! their penalties STACK in `detectors::detect_all` (Option A; the
//! supersede-vs-stack precedence policy is deferred to the scrap-rs#32
//! score aggregator). This is intentional — a sprawling test that
//! checks nothing is two distinct problems, not one.
//!
//! ## Pure-detector convention
//!
//! Mirrors `zero_assertion` / `no_op_io`: the detector does NOT consult
//! `parsed.opt_outs`. Per-test `#[allow(scrap::large_example)]`
//! honor-policy is the pipeline driver's job (scrap-rs#72); the driver
//! applies opt-out suppression post-emission.

use crate::domain::classification::{Actionability, Severity};
use crate::domain::config::DetectorConfig;
use crate::domain::finding::Finding;
use crate::domain::parsed::ParsedTest;
use crate::domain::smell::{Smell, SmellCategory};

/// Default penalty per the CLAUDE.md / kickstart-plan detection table.
const DEFAULT_PENALTY: u32 = 4;

/// Default body-line threshold above which the smell fires. Tuned to
/// `30` for Rust (vs Clojure `scrap`'s `20`) — see module-level docs.
const DEFAULT_LINE_THRESHOLD: u32 = 30;

/// Default severity: below `no-op-io`'s `Moderate` (penalty 4 < 8).
const DEFAULT_SEVERITY: Severity = Severity::Low;

/// Default actionability: the fix is to split the oversized test into
/// focused examples (or extract setup helpers).
const DEFAULT_ACTIONABILITY: Actionability = Actionability::ManualSplit;

/// Detect the `large-example` smell on a parsed test.
///
/// See module-level docs for the detection rule + the
/// orthogonal-to-assertion-smells note. Returns:
/// - `None` when the detector is disabled, or when
///   `parsed.body_line_count` is at or below the resolved threshold
///   (a `body_line_count` of `0` therefore never fires).
/// - `Some(Finding)` carrying one [`Smell`] whose
///   `category = SmellCategory::LargeExample`, `severity = Severity::Low`,
///   `actionability = Actionability::ManualSplit`, and
///   `penalty = cfg.penalty.unwrap_or(DEFAULT_PENALTY)`.
#[must_use]
pub fn detect(parsed: &ParsedTest, cfg: &DetectorConfig) -> Option<Finding> {
    if cfg.enabled == Some(false) {
        return None;
    }
    let threshold = cfg.line_threshold.unwrap_or(DEFAULT_LINE_THRESHOLD);
    if parsed.body_line_count <= threshold {
        return None;
    }

    let penalty = cfg.penalty.unwrap_or(DEFAULT_PENALTY);
    // Whole-test span: "this body is too long" is a fn-level verdict, so
    // the smell points at `parsed.identity.span` (the full `fn name(...)
    // { .. }`) like `no-op-io` / `zero-assertion`, not at any single line.
    let smell = Smell::new(
        SmellCategory::LargeExample,
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
    use crate::domain::types::{FilePath, QualifiedName, Span, TestIdentity};
    use proptest::prelude::*;
    use std::collections::BTreeSet;

    // ── Test helpers ────────────────────────────────────────────────────

    /// Build a `ParsedTest` whose only relevant field is
    /// `body_line_count` (the sole input the detector reads alongside
    /// `cfg`). All other fields are empty — the detector is structural
    /// and never touches assertions / facts / opt-outs.
    fn large_test(body_line_count: u32) -> ParsedTest {
        ParsedTest::new(
            TestIdentity::new(
                FilePath::new("a.rs"),
                QualifiedName::new("a::tests::t"),
                Span::new(1, 5, 1, 1),
            ),
            Vec::new(),
            Vec::new(),
            body_line_count,
            Vec::new(),
            BTreeSet::new(),
            BTreeSet::new(),
        )
    }

    // ── Negative branches: detector returns None ────────────────────────

    #[test]
    fn detect_returns_none_when_disabled_via_config() {
        let cfg = DetectorConfig {
            enabled: Some(false),
            penalty: None,
            // Way over the default threshold — would fire if enabled.
            line_threshold: None,
        };
        assert!(detect(&large_test(100), &cfg).is_none());
    }

    #[test]
    fn detect_returns_none_at_default_threshold_boundary() {
        // body_line_count == 30, threshold 30 → NOT flagged (`>` not `>=`).
        assert!(detect(&large_test(30), &DetectorConfig::default()).is_none());
    }

    #[test]
    fn detect_returns_none_for_empty_body() {
        // AC edge case: a zero-line body must never trigger (0 > 30 is
        // false). Pinned explicitly so a future threshold default of 0
        // can't silently start flagging empty bodies.
        assert!(detect(&large_test(0), &DetectorConfig::default()).is_none());
    }

    // ── Positive branches ────────────────────────────────────────────────

    #[test]
    #[allow(clippy::float_cmp)]
    fn detect_fires_one_past_default_threshold() {
        // body_line_count == 31, threshold 30 → fires.
        let finding = detect(&large_test(31), &DetectorConfig::default()).expect("31 > 30 fires");
        assert_eq!(finding.smells.len(), 1);
        assert_eq!(finding.smells[0].category, SmellCategory::LargeExample);
        assert_eq!(finding.smells[0].penalty, 4);
        assert_eq!(finding.smells[0].severity, Severity::Low);
        assert_eq!(finding.smells[0].actionability, Actionability::ManualSplit);
        assert_eq!(finding.scrap_score, 4.0);
        // Whole-test span attribution.
        assert_eq!(finding.smells[0].span, Some(Span::new(1, 5, 1, 1)));
    }

    #[test]
    fn detect_honors_custom_line_threshold_override() {
        let cfg = DetectorConfig {
            enabled: None,
            penalty: None,
            line_threshold: Some(10),
        };
        // count 11 > threshold 10 → fires.
        assert!(detect(&large_test(11), &cfg).is_some());
        // count 10 == threshold 10 → does NOT fire (boundary).
        assert!(detect(&large_test(10), &cfg).is_none());
    }

    #[test]
    fn detect_applies_custom_penalty_override() {
        let cfg = DetectorConfig {
            enabled: None,
            penalty: Some(25),
            line_threshold: None,
        };
        let finding = detect(&large_test(31), &cfg).expect("large test fires under override");
        assert_eq!(finding.smells.len(), 1);
        assert_eq!(finding.smells[0].penalty, 25);
    }

    // ── Property tests ────────────────────────────────────────────────────

    /// Arbitrary `(threshold, body_line_count)` pair, both bounded well
    /// below `u32::MAX` so the monotonic test's `n + 1` cannot overflow.
    fn arb_threshold_and_count() -> impl Strategy<Value = (u32, u32)> {
        (0u32..10_000, 0u32..10_000)
    }

    /// Arbitrary `ParsedTest` varying only `body_line_count`. Other
    /// fields are empty — the detector is structural.
    fn arb_parsed_test() -> impl Strategy<Value = ParsedTest> {
        (0u32..10_000).prop_map(large_test)
    }

    /// Arbitrary `DetectorConfig` that is NEVER disabled (so firing
    /// assertions don't flake to `None`); penalty + threshold vary.
    fn arb_enabled_detector_config() -> impl Strategy<Value = DetectorConfig> {
        (
            // enabled: None or Some(true) — never Some(false).
            proptest::option::of(Just(true)),
            proptest::option::of(1u32..1_000),
            proptest::option::of(0u32..10_000),
        )
            .prop_map(|(enabled, penalty, line_threshold)| DetectorConfig {
                enabled,
                penalty,
                line_threshold,
            })
    }

    proptest! {
        /// Determinism (the AC's idempotence intent — the literal
        /// `detect(detect(ast))` doesn't typecheck given `detect :
        /// &ParsedTest -> Option<Finding>`; the pure-function contract is
        /// what this captures, matching the sibling detectors' PR-note
        /// translation).
        #[test]
        fn proptest_detect_is_deterministic(
            pt in arb_parsed_test(),
            cfg in arb_enabled_detector_config(),
        ) {
            prop_assert_eq!(detect(&pt, &cfg), detect(&pt, &cfg));
        }

        /// Cardinality: result is `None` or a single-Smell `Finding`.
        #[test]
        fn proptest_detect_emits_at_most_one_smell(
            pt in arb_parsed_test(),
            cfg in arb_enabled_detector_config(),
        ) {
            if let Some(finding) = detect(&pt, &cfg) {
                prop_assert_eq!(finding.smells.len(), 1);
            }
        }

        /// Monotonic in `body_line_count` (the key AC). For an arbitrary
        /// `threshold` and count `n`:
        /// - if `n > threshold`: detect fires, AND `n + 1` also fires
        ///   (firing is upward-closed in body size);
        /// - if `n == threshold` or `n == threshold - 1`: detect is
        ///   `None` (the at-or-below-threshold region is suppressed).
        #[test]
        fn proptest_detect_is_monotonic_in_body_line_count(
            (threshold, n) in arb_threshold_and_count(),
        ) {
            let cfg = DetectorConfig {
                enabled: None,
                penalty: None,
                line_threshold: Some(threshold),
            };
            if n > threshold {
                prop_assert!(detect(&large_test(n), &cfg).is_some(), "n > threshold must fire");
                prop_assert!(
                    detect(&large_test(n + 1), &cfg).is_some(),
                    "n + 1 must also fire (upward-closed)",
                );
            }
            // At the threshold: never fires.
            prop_assert!(
                detect(&large_test(threshold), &cfg).is_none(),
                "body_line_count == threshold must not fire (`>` not `>=`)",
            );
            // Just below the threshold: never fires.
            prop_assert!(
                detect(&large_test(threshold.saturating_sub(1)), &cfg).is_none(),
                "body_line_count == threshold - 1 must not fire",
            );
        }
    }
}
