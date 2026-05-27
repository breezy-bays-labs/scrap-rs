//! Detector step definitions for the cucumber harness — exercises
//! `scrap_core::detectors::zero_assertion::detect` end-to-end through
//! the cucumber-rs runner.
//!
//! Pulled out of `tests/cucumber.rs` per the W5.1 mod-block split
//! convention. The entry point in `cucumber.rs` includes this file
//! via `#[path = "cucumber_steps/detectors_zero_assertion.rs"] mod
//! detectors_zero_assertion_steps;`; cucumber-rs registers the step
//! fns into the same global registry as the file-walker / config
//! steps.
//!
//! ## Scenario surface
//!
//! 5 scenarios in `tests/features/detectors_zero_assertion.feature`:
//! - 1 positive (empty facts → `Some(Finding)`)
//! - 1 negative via explicit assertion
//! - 1 negative via implicit-assertion source (`should_panic`)
//! - 1 config gate (`enabled = false` short-circuits)
//! - 1 config gate (custom penalty override flows through)
//!
//! ## Lint allowances
//!
//! Inherits the same pedantic-relax battery as `tests/cucumber.rs`
//! (workspace `[lints]` propagates to integration tests). Per
//! tracked: scrap-rs#50, the file-walker harness carries
//! `#[allow(clippy::needless_pass_by_value)]` etc.; the same
//! decisions hold here.

#![allow(clippy::needless_pass_by_value)]

use cucumber::{given, then, when};
use scrap_core::cli::config::DetectorConfig;
use scrap_core::detectors::zero_assertion;
use scrap_core::domain::assertion_sources::AssertionSource;
use scrap_core::domain::classification::{Actionability, Severity};
use scrap_core::domain::finding::Finding;
use scrap_core::domain::parsed::{ParsedAssertion, ParsedTest};
use scrap_core::domain::smell::SmellCategory;
use scrap_core::domain::types::{FilePath, QualifiedName, Span, TestIdentity};
use std::collections::BTreeSet;

use super::World;

// ── Given builders ──────────────────────────────────────────────────

/// Construct a smelly baseline `ParsedTest`: empty assertions,
/// empty implicit sources, empty behavioral facts. Detector triggers
/// under default config.
fn smelly_test() -> ParsedTest {
    ParsedTest::new(
        TestIdentity::new(
            FilePath::new("a.rs"),
            QualifiedName::new("a::tests::t"),
            Span::new(1, 5),
        ),
        Vec::new(),
        Vec::new(),
        3,
        Vec::new(),
        BTreeSet::new(),
        BTreeSet::new(),
    )
}

#[given(regex = r"^a `ParsedTest` with no assertions and no implicit assertion sources$")]
fn parsed_test_empty(w: &mut World) {
    w.parsed_test = Some(smelly_test());
}

#[given(regex = r"^a `ParsedTest` with one `assert_eq` assertion$")]
fn parsed_test_with_assert_eq(w: &mut World) {
    let mut pt = smelly_test();
    pt.assertions.push(ParsedAssertion::new(
        "assert_eq",
        Some("1, 1".into()),
        Span::new(2, 2),
        // `arguments_identical` + `single_arg_value` added to
        // `ParsedAssertion::new` at scrap-rs#24. This zero-assertion
        // fixture uses a non-tautological assertion, so both default
        // to `false` / `None`.
        false,
        None,
    ));
    w.parsed_test = Some(pt);
}

#[given(regex = r"^a `ParsedTest` with implicit assertion source `should_panic`$")]
fn parsed_test_with_should_panic(w: &mut World) {
    let mut pt = smelly_test();
    pt.implicit_assertion_sources
        .push(AssertionSource::ShouldPanic);
    w.parsed_test = Some(pt);
}

// ── When invocations ────────────────────────────────────────────────

#[when(
    regex = r"^the caller invokes `zero_assertion::detect\(\)` with the default `DetectorConfig`$"
)]
fn invoke_with_default_config(w: &mut World) {
    let pt = w.parsed_test.as_ref().expect("ParsedTest given");
    let cfg = DetectorConfig::default();
    w.detect_result = Some(zero_assertion::detect(pt, &cfg));
    w.detector_config = Some(cfg);
}

#[when(
    regex = r"^the caller invokes `zero_assertion::detect\(\)` with a `DetectorConfig` where `enabled = false`$"
)]
fn invoke_with_disabled_config(w: &mut World) {
    let pt = w.parsed_test.as_ref().expect("ParsedTest given");
    let cfg = DetectorConfig {
        enabled: Some(false),
        penalty: None,
        line_threshold: None,
    };
    w.detect_result = Some(zero_assertion::detect(pt, &cfg));
    w.detector_config = Some(cfg);
}

#[when(
    regex = r"^the caller invokes `zero_assertion::detect\(\)` with a `DetectorConfig` where `penalty = (\d+)`$"
)]
fn invoke_with_custom_penalty(w: &mut World, penalty: u32) {
    let pt = w.parsed_test.as_ref().expect("ParsedTest given");
    let cfg = DetectorConfig {
        enabled: None,
        penalty: Some(penalty),
        line_threshold: None,
    };
    w.detect_result = Some(zero_assertion::detect(pt, &cfg));
    w.detector_config = Some(cfg);
}

// ── Then assertions ─────────────────────────────────────────────────

fn detect_outcome(w: &World) -> Option<&Finding> {
    w.detect_result
        .as_ref()
        .expect("detect was invoked")
        .as_ref()
}

#[then(regex = r"^the result is `None`$")]
fn assert_result_is_none(w: &mut World) {
    let outcome = detect_outcome(w);
    assert!(outcome.is_none(), "expected None, got Some({outcome:?})");
}

#[then(
    regex = r"^the result is `Some\(Finding\)` with category `zero_assertion`, severity `high`, actionability `auto_refactor`, and penalty (\d+)$"
)]
fn assert_finding_full_shape(w: &mut World, expected_penalty: u32) {
    // Direct enum comparisons (CodeRabbit nitpick 2026-05-27 on PR #82):
    // brittleness of `format!("{:?}", smell.severity) == "High"` was that
    // a future `#[derive(Debug)]` reshape (or a `Debug` impl override)
    // could silently change the string without changing the semantic
    // variant. Direct `==` on the enum variants is correct + faster +
    // doesn't tie the test to Debug formatting choices.
    let finding = detect_outcome(w).expect("expected Some(Finding)");
    assert_eq!(finding.smells.len(), 1, "expected exactly one Smell");
    let smell = &finding.smells[0];
    assert_eq!(smell.category, SmellCategory::ZeroAssertion);
    assert_eq!(smell.severity, Severity::High);
    assert_eq!(smell.actionability, Actionability::AutoRefactor);
    assert_eq!(smell.penalty, expected_penalty);
}

#[then(
    regex = r"^the result is `Some\(Finding\)` with category `zero_assertion` and penalty (\d+)$"
)]
fn assert_finding_category_and_penalty(w: &mut World, expected_penalty: u32) {
    let finding = detect_outcome(w).expect("expected Some(Finding)");
    assert_eq!(finding.smells.len(), 1);
    let smell = &finding.smells[0];
    assert_eq!(smell.category, SmellCategory::ZeroAssertion);
    assert_eq!(smell.penalty, expected_penalty);
}
