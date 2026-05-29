//! End-to-end test of the `tautological-assertion` detector against
//! real Rust source files via the syn parser, routed through the
//! production aggregator [`scrap_core::detectors::detect_all`].
//!
//! **Why `detect_all`, not `tautological_assertion::detect`** — the
//! detector's `detect` fn was correct since scrap-rs#24, but it was
//! never invoked by `detect_all`, so it never ran in the pipeline (the
//! dead-wire bug, scrap-rs#99). A test that called `detect` directly
//! would pass even on the dead-wired tree and prove nothing. These
//! tests run the SAME aggregator the analyzer pipeline uses, so they
//! fail RED on the dead-wired tree and pass GREEN once tautological is
//! wired into `detect_all`.
//!
//! - **Positive**: `true_positives/tautological.rs` MUST surface a
//!   [`SmellCategory::TautologicalAssertion`] smell when run through
//!   `detect_all`.
//! - **Cross-detector**: a tautological body carries a (tautological)
//!   assertion, so `has_positive_check` is true → `zero-assertion` and
//!   `no-op-io` both suppress. `detect_all` therefore emits ONLY the
//!   tautological smell (no co-fire) — pinned here so a future
//!   suppression-predicate regression surfaces.

use scrap4rs::detectors::detect_all;
use scrap4rs::domain::config::FileConfig;
use scrap4rs::domain::parsed::ParsedTest;
use scrap4rs::domain::smell::SmellCategory;
use scrap4rs::domain::types::FilePath;
use scrap4rs::parser::SynTestParser;
use scrap4rs::ports::parser::TestParserPort;

fn parse_fixture(rel: &str) -> Vec<ParsedTest> {
    let abs = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join(rel);
    let source = std::fs::read_to_string(&abs)
        .unwrap_or_else(|e| panic!("read fixture {}: {e}", abs.display()));
    SynTestParser::new()
        .parse_test_source(&source, &FilePath::new(rel))
        .unwrap_or_else(|e| panic!("fixture {rel} must parse cleanly: {e:?}"))
        .tests
}

#[test]
fn e2e_detect_all_emits_tautological_on_true_positive() {
    let tests = parse_fixture("tests/fixtures/true_positives/tautological.rs");
    assert_eq!(
        tests.len(),
        1,
        "expected one test in the true-positive fixture",
    );

    let smells = detect_all(&tests[0], &FileConfig::default());
    let tautological_count = smells
        .iter()
        .filter(|s| s.category == SmellCategory::TautologicalAssertion)
        .count();
    assert!(
        tautological_count > 0,
        "detect_all MUST emit at least one TautologicalAssertion smell for \
         true_positives/tautological.rs (the scrap-rs#99 dead-wire regression guard); \
         got smells: {smells:?}",
    );
}

#[test]
fn e2e_detect_all_does_not_co_fire_zero_assertion_or_no_op_io_on_tautology() {
    // A tautological assertion is still a recorded assertion, so
    // `has_positive_check` is true and both zero-assertion and no-op-io
    // suppress. `detect_all` must emit ONLY TautologicalAssertion smells.
    let tests = parse_fixture("tests/fixtures/true_positives/tautological.rs");
    let smells = detect_all(&tests[0], &FileConfig::default());

    let non_tautological: Vec<SmellCategory> = smells
        .iter()
        .map(|s| s.category)
        .filter(|c| *c != SmellCategory::TautologicalAssertion)
        .collect();
    assert!(
        non_tautological.is_empty(),
        "tautology body must not co-fire other detectors (has_positive_check is true); \
         got non-tautological categories: {non_tautological:?}",
    );
}
