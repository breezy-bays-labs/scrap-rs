//! Cucumber step impls for the `tautological-assertion` detector
//! BDD spec (`tests/features/tautological_assertion.feature`).
//!
//! Sibling step file under the harness in `tests/cucumber.rs`, mirroring
//! the `config.rs` and `json_reporter.rs` precedents. The shared
//! `World` struct lives in `tests/cucumber.rs`; this module adds step
//! defs that read/mutate the `tautological_*` fields.
//!
//! Scope: pure detector-contract scenarios. Fixture-driven scenarios
//! (proptest body / `#[should_panic]` parser facts) live in
//! `crates/scrap4rs/tests/features/parser.feature` instead — scrap-core
//! must stay AST-pure (no `scrap4rs` import in step impls).

// Per the scrap-core/tests/cucumber.rs preamble: workspace lints lift
// surfaced pre-existing clippy::pedantic nits that aren't worth scoping
// here. Same allow-list applies.
#![allow(clippy::needless_pass_by_value)]

use cucumber::{given, then, when};
use scrap_core::detectors::tautological_assertion;
use scrap_core::domain::config::DetectorConfig;
use scrap_core::domain::literal_value::LiteralValue;
use scrap_core::domain::parsed::{ParsedAssertion, ParsedTest};
use scrap_core::domain::smell::SmellCategory;
use scrap_core::domain::types::{FilePath, QualifiedName, Span, TestIdentity};
use std::collections::BTreeSet;

use super::World;

// ─── Fixtures ───────────────────────────────────────────────────────

/// Canonical `TestIdentity` for the synthetic `ParsedTest` fixtures.
fn fixture_identity() -> TestIdentity {
    TestIdentity::new(
        FilePath::new("a.rs"),
        QualifiedName::new("a::tests::t"),
        Span::new(10, 20, 1, 1),
    )
}

fn empty_parsed_test() -> ParsedTest {
    parsed_test_with_assertions(Vec::new())
}

fn parsed_test_with_assertions(assertions: Vec<ParsedAssertion>) -> ParsedTest {
    ParsedTest::new(
        fixture_identity(),
        Vec::new(),
        assertions,
        5,
        Vec::new(),
        BTreeSet::new(),
        // `behavioral_facts` added to ParsedTest::new at scrap-rs#30,
        // migrated to `Vec` at scrap-rs#112; the tautological-assertion
        // detector does not consult this field — pass empty.
        Vec::new(),
    )
}

fn make_assertion(
    line: u32,
    arguments_identical: bool,
    single_arg_value: Option<LiteralValue>,
) -> ParsedAssertion {
    ParsedAssertion::new(
        "assert",
        None,
        Span::new(line, line, 1, 1),
        arguments_identical,
        single_arg_value,
    )
}

// ─── Given ──────────────────────────────────────────────────────────

#[given(regex = r"^the tautological-assertion detector$")]
fn given_detector(w: &mut World) {
    // No-op: the detector is a pure free function. The Given exists
    // for readability — it scopes the scenarios to the detector
    // contract.
    w.parsed_test = None;
    w.detect_result = None;
}

// ─── When (ParsedTest builders) ─────────────────────────────────────

#[when(regex = r"^a ParsedTest is built with no assertions$")]
fn when_no_assertions(w: &mut World) {
    w.parsed_test = Some(empty_parsed_test());
    invoke_detect(w);
}

#[when(
    regex = r"^a ParsedTest is built with one assertion whose single_arg_value is Bool\((true|false)\)$"
)]
fn when_one_single_arg_bool(w: &mut World, val: String) {
    let assertion = make_assertion(15, false, Some(LiteralValue::Bool(val == "true")));
    w.parsed_test = Some(parsed_test_with_assertions(vec![assertion]));
    invoke_detect(w);
}

#[when(regex = r"^a ParsedTest is built with one assertion whose arguments_identical is true$")]
fn when_one_arguments_identical(w: &mut World) {
    let assertion = make_assertion(15, true, None);
    w.parsed_test = Some(parsed_test_with_assertions(vec![assertion]));
    invoke_detect(w);
}

#[when(
    regex = r"^a ParsedTest is built with one assertion whose arguments_identical is false and single_arg_value is None$"
)]
fn when_one_real_assertion(w: &mut World) {
    let assertion = make_assertion(15, false, None);
    w.parsed_test = Some(parsed_test_with_assertions(vec![assertion]));
    invoke_detect(w);
}

#[when(regex = r"^a ParsedTest is built with two assertions both with arguments_identical true$")]
fn when_two_tautological_assertions(w: &mut World) {
    let assertions = vec![
        make_assertion(11, true, None),
        make_assertion(12, true, None),
    ];
    w.parsed_test = Some(parsed_test_with_assertions(assertions));
    invoke_detect(w);
}

// Helper invoked by every When to populate `detect_result`. Uses the
// default `DetectorConfig` (enabled, default penalty 10) — these
// scenarios assert the v0.1 default contract; per-config gating is
// exercised in the in-module unit tests (scrap-rs#99 signature align).
fn invoke_detect(w: &mut World) {
    let parsed = w.parsed_test.as_ref().expect("ParsedTest built");
    w.detect_result = Some(tautological_assertion::detect(
        parsed,
        &DetectorConfig::default(),
    ));
}

// ─── Then ───────────────────────────────────────────────────────────

#[then(
    regex = r"^the detector emits a Finding with one TautologicalAssertion smell of penalty 10$"
)]
fn then_one_smell_penalty_10(w: &mut World) {
    let finding = w
        .detect_result
        .as_ref()
        .expect("detect invoked")
        .as_ref()
        .expect("Some(Finding)");
    assert_eq!(finding.smells.len(), 1);
    assert_eq!(
        finding.smells[0].category,
        SmellCategory::TautologicalAssertion
    );
    assert_eq!(finding.smells[0].penalty, 10);
}

#[then(regex = r"^the detector emits a Finding with two TautologicalAssertion smells$")]
fn then_two_smells(w: &mut World) {
    let finding = w
        .detect_result
        .as_ref()
        .expect("detect invoked")
        .as_ref()
        .expect("Some(Finding)");
    assert_eq!(finding.smells.len(), 2);
    for smell in &finding.smells {
        assert_eq!(smell.category, SmellCategory::TautologicalAssertion);
        assert_eq!(smell.penalty, 10);
    }
}

#[then(regex = r"^the Finding's scrap_score is ([0-9]+(?:\.[0-9]+)?)$")]
fn then_scrap_score_is(w: &mut World, expected: String) {
    let expected: f64 = expected.parse().expect("scrap_score parses as f64");
    let finding = w
        .detect_result
        .as_ref()
        .expect("detect invoked")
        .as_ref()
        .expect("Some(Finding)");
    assert!(
        (finding.scrap_score - expected).abs() < f64::EPSILON,
        "expected scrap_score {expected}, got {}",
        finding.scrap_score,
    );
}

#[then(regex = r"^the detector emits no Finding$")]
fn then_no_finding(w: &mut World) {
    let result = w.detect_result.as_ref().expect("detect invoked");
    assert!(result.is_none(), "expected None, got {result:?}");
}
