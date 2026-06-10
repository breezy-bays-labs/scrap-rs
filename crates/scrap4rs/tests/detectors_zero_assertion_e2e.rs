//! End-to-end test of the `zero-assertion` detector against real Rust
//! source files via the syn parser. Each fixture parses through
//! [`SynTestParser`]; the resulting `ParsedTest`(s) feed
//! [`scrap_core::detectors::zero_assertion::detect`].
//!
//! - **Positive**: `true_positives/zero_assertion.rs` MUST trigger
//!   the detector (one finding emitted).
//! - **Negative**: every fixture under `runner_shells/` MUST NOT
//!   trigger — each is a runner-shell or attribute-source that the
//!   detector's three-clause suppression catches.

use scrap_core::detectors::zero_assertion::detect;
use scrap_core::domain::config::DetectorConfig;
use scrap_core::domain::parsed::ParsedTest;
use scrap_core::domain::types::FilePath;
use scrap_core::ports::parser::TestParserPort;
use scrap4rs::parser::SynTestParser;

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
fn e2e_zero_assertion_triggers_on_true_positive() {
    let tests = parse_fixture("tests/fixtures/true_positives/zero_assertion.rs");
    assert_eq!(
        tests.len(),
        1,
        "expected one test in the true-positive fixture"
    );
    let finding = detect(&tests[0], &DetectorConfig::default());
    assert!(
        finding.is_some(),
        "true_positives/zero_assertion.rs MUST trigger the detector",
    );
}

#[test]
fn e2e_zero_assertion_does_not_trigger_on_runner_shells() {
    // CQO FOLD-REQUIRED 2026-05-26 (per memory `feedback_pristine-test-output`):
    // collect ALL failures before asserting, so one bad fixture doesn't
    // hide the others from downstream agentic loops. The naive
    // `for f in &FIXTURES { assert!(...) }` short-circuits on first
    // failure and obscures which other fixtures might also have regressed.
    let fixtures = [
        "tests/fixtures/runner_shells/cucumber_shell.rs",
        "tests/fixtures/runner_shells/insta_shell.rs",
        "tests/fixtures/runner_shells/kani_shell.rs",
        "tests/fixtures/runner_shells/pretty_assertions_shell.rs",
        "tests/fixtures/runner_shells/proptest_macro_suffix.rs",
        "tests/fixtures/runner_shells/proptest_shell.rs",
        "tests/fixtures/runner_shells/quickcheck_shell.rs",
        "tests/fixtures/runner_shells/should_panic_shell.rs",
        "tests/fixtures/runner_shells/trybuild_shell.rs",
    ];

    let mut unexpected_triggers: Vec<String> = Vec::new();
    for f in fixtures {
        for parsed in parse_fixture(f) {
            if detect(&parsed, &DetectorConfig::default()).is_some() {
                unexpected_triggers.push(format!(
                    "{f}::{name}",
                    name = parsed.identity.qualified_name.as_str(),
                ));
            }
        }
    }

    assert!(
        unexpected_triggers.is_empty(),
        "zero-assertion fired on runner shells that should suppress it:\n  - {}",
        unexpected_triggers.join("\n  - "),
    );
}

#[test]
fn e2e_zero_assertion_does_not_trigger_on_behavioral_facts() {
    // Cross-validate the BehavioralFact::ResultAsserted suppression
    // path against each panic-chain method-ident fixture. Same
    // collect-then-assert pattern (CQO FOLD-REQUIRED) for stderr clarity.
    //
    // Adding a new panic-chain method-ident to `PANIC_CHAIN_METHOD_NAMES`
    // (`crates/scrap4rs/src/parser/body.rs`) should land alongside a
    // new fixture here so this list stays exhaustive.
    let fixtures = [
        "tests/fixtures/behavioral_facts/unwrap_chain.rs",
        "tests/fixtures/behavioral_facts/expect_chain.rs",
        "tests/fixtures/behavioral_facts/unwrap_err_chain.rs",
        "tests/fixtures/behavioral_facts/expect_err_chain.rs",
    ];

    let mut unexpected_triggers: Vec<String> = Vec::new();
    for f in fixtures {
        for parsed in parse_fixture(f) {
            if detect(&parsed, &DetectorConfig::default()).is_some() {
                unexpected_triggers.push(format!(
                    "{f}::{name}",
                    name = parsed.identity.qualified_name.as_str(),
                ));
            }
        }
    }

    assert!(
        unexpected_triggers.is_empty(),
        "zero-assertion fired on behavioral_facts fixtures that should suppress it:\n  - {}",
        unexpected_triggers.join("\n  - "),
    );
}
