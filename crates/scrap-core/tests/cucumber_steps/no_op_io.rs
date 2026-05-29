//! Cucumber step impls for the `no-op-io` detector BDD spec
//! (`tests/features/no_op_io.feature`).
//!
//! Sibling step file under the harness in `tests/cucumber.rs`, mirroring
//! the `tautological.rs` precedent. The shared `World` struct lives in
//! `tests/cucumber.rs`; this module adds step defs that read/mutate the
//! shared `parsed_test` / `detect_result` fields.
//!
//! Scope: pure detector-contract scenarios. Fixture-driven scenarios
//! (`let _ = ...;` source → ResultDiscarded; `let _: () = ...;` FP
//! guard) live in `crates/scrap4rs/tests/features/parser.feature`
//! instead — scrap-core must stay AST-pure (no `scrap4rs` import here).

// Per the scrap-core/tests/cucumber.rs preamble: workspace lints lift
// surfaced pre-existing clippy::pedantic nits that aren't worth scoping
// here. Same allow-list applies.
#![allow(clippy::needless_pass_by_value)]

use cucumber::{given, then, when};
use scrap_core::cli::config::DetectorConfig;
use scrap_core::detectors::no_op_io;
use scrap_core::domain::behavioral_fact::{BehavioralFact, ResultDiscardKind};
use scrap_core::domain::parsed::{ParsedAssertion, ParsedTest};
use scrap_core::domain::smell::SmellCategory;
use scrap_core::domain::types::{FilePath, QualifiedName, Span, TestIdentity};
use std::collections::BTreeSet;

use super::World;

// ─── Fixtures ───────────────────────────────────────────────────────

fn fixture_identity() -> TestIdentity {
    TestIdentity::new(
        FilePath::new("a.rs"),
        QualifiedName::new("a::tests::t"),
        Span::new(10, 20, 1, 1),
    )
}

/// Build a `ParsedTest` with the given assertions + behavioral facts.
fn parsed_test(assertions: Vec<ParsedAssertion>, facts: BTreeSet<BehavioralFact>) -> ParsedTest {
    ParsedTest::new(
        fixture_identity(),
        Vec::new(),
        assertions,
        5,
        Vec::new(),
        BTreeSet::new(),
        facts,
    )
}

fn call_discard_fact() -> BehavioralFact {
    BehavioralFact::ResultDiscarded {
        kind: ResultDiscardKind::Call,
    }
}

fn real_assertion() -> ParsedAssertion {
    ParsedAssertion::new("assert_eq", None, Span::new(15, 15, 1, 1), false, None)
}

// ─── Given ──────────────────────────────────────────────────────────

#[given(regex = r"^the no-op-io detector$")]
fn given_detector(w: &mut World) {
    w.parsed_test = None;
    w.detect_result = None;
}

// ─── When (ParsedTest builders) ─────────────────────────────────────

#[when(regex = r"^a ParsedTest is built with one ResultDiscarded fact of kind Call$")]
fn when_one_discard(w: &mut World) {
    let facts = [call_discard_fact()].into();
    w.parsed_test = Some(parsed_test(Vec::new(), facts));
    invoke_detect(w);
}

#[when(regex = r"^a ParsedTest is built with one ResultDiscarded fact and one real assertion$")]
fn when_discard_plus_assertion(w: &mut World) {
    let facts = [call_discard_fact()].into();
    w.parsed_test = Some(parsed_test(vec![real_assertion()], facts));
    invoke_detect(w);
}

#[when(regex = r"^a ParsedTest is built with one ResultDiscarded fact and a ResultAsserted fact$")]
fn when_discard_plus_result_asserted(w: &mut World) {
    let facts = [call_discard_fact(), BehavioralFact::ResultAsserted].into();
    w.parsed_test = Some(parsed_test(Vec::new(), facts));
    invoke_detect(w);
}

#[when(regex = r"^a ParsedTest is built with no behavioral facts$")]
fn when_no_facts(w: &mut World) {
    w.parsed_test = Some(parsed_test(Vec::new(), BTreeSet::new()));
    invoke_detect(w);
}

fn invoke_detect(w: &mut World) {
    let parsed = w.parsed_test.as_ref().expect("ParsedTest built");
    w.detect_result = Some(no_op_io::detect(parsed, &DetectorConfig::default()));
}

// ─── Then ───────────────────────────────────────────────────────────

#[then(regex = r"^the detector emits a Finding with one NoOpIo smell of penalty 8$")]
fn then_one_noop_smell(w: &mut World) {
    let finding = w
        .detect_result
        .as_ref()
        .expect("detect invoked")
        .as_ref()
        .expect("Some(Finding)");
    assert_eq!(finding.smells.len(), 1);
    assert_eq!(finding.smells[0].category, SmellCategory::NoOpIo);
    assert_eq!(finding.smells[0].penalty, 8);
}

#[then(regex = r"^the no-op-io Finding's scrap_score is ([0-9]+(?:\.[0-9]+)?)$")]
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

#[then(regex = r"^the no-op-io detector emits no Finding$")]
fn then_no_finding(w: &mut World) {
    let result = w.detect_result.as_ref().expect("detect invoked");
    assert!(result.is_none(), "expected None, got {result:?}");
}
